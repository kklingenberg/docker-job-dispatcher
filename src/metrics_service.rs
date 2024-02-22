//! Collects metrics from the docker events stream and exposes them in
//! OpenMetrics format.

use crate::docker;

use actix_web::{error, get, HttpResponse};
use anyhow::Result;
use futures::stream::TryStreamExt;
use once_cell::sync::OnceCell;
use prometheus_client::{
    encoding::{text::encode, EncodeLabelSet},
    metrics::{counter::Counter, family::Family},
    registry::Registry,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Static metrics registry.
static REGISTRY: OnceCell<Arc<Mutex<Registry>>> = OnceCell::new();

/// Get the mutexed registry.
fn registry() -> &'static Arc<Mutex<Registry>> {
    REGISTRY.get_or_init(|| Arc::new(Mutex::new(<Registry>::default())))
}

/// Metrics labels.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct Labels {
    namespace: String,
    action: Option<String>,
    status: Option<String>,
}

/// Expose metrics.
#[get("/metrics")]
pub async fn expose() -> actix_web::Result<HttpResponse> {
    let mut body = String::new();
    let reg = registry().lock().await;
    encode(&mut body, &reg)
        .map_err(|_| error::ErrorInternalServerError("couldn't encode metrics"))?;
    Ok(HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4")
        .body(body))
}

/// Consume the docker events stream and update metrics according to
/// the events read.
pub async fn run(namespace: String) -> Result<()> {
    let jobs = Family::<Labels, Counter>::default();
    {
        let mut reg = registry().lock().await;
        reg.register("jobs", "Number of jobs", jobs.clone());
    }
    // account for already active jobs
    let (active, created) = tokio::join!(
        docker::count_active(&namespace),
        docker::get_pending(&namespace)
    );
    let active: u64 = active?.try_into()?;
    let created: u64 = created?.len().try_into()?;
    jobs.get_or_create(&Labels {
        namespace: namespace.clone(),
        action: Some(String::from("create")),
        status: None,
    })
    .inc_by(active + created);
    jobs.get_or_create(&Labels {
        namespace: namespace.clone(),
        action: Some(String::from("start")),
        status: None,
    })
    .inc_by(active);
    // listen for new events
    // note: events in between the probe above and the start of this
    // stream are lost, oh well
    docker::job_events(&namespace)?
        .try_for_each(|event| async {
            jobs.get_or_create(&Labels {
                namespace: namespace.clone(),
                action: event.action,
                status: event
                    .actor
                    .and_then(|a| a.attributes)
                    .and_then(|map| map.get("exitCode").map(String::clone)),
            })
            .inc();
            Ok(())
        })
        .await?;
    Ok(())
}
