//! Implements the poll-based cleaning task.

use crate::docker;
use anyhow::{anyhow, Context, Result};
use chrono::{offset::Utc, DateTime, Duration as ChronoDuration};
use futures::future::join_all;
use tokio::time::{self, Duration};
use tracing::{error, info};

/// Check exited containers, and remove them if they're old enough
/// according to maximum age.
async fn clean(max_age: u32, namespace: &str) -> Result<()> {
    let finished_at_threshold = Utc::now()
        .checked_sub_signed(ChronoDuration::seconds(max_age.into()))
        .ok_or_else(|| anyhow!("can't calculate exited age threshold"))?
        .timestamp();
    let containers: Vec<_> = join_all(
        docker::get_exited(namespace)
            .await
            .context("while fetching exited jobs")?
            .into_iter()
            .filter_map(|container| {
                container
                    .names
                    .and_then(|ns| ns.into_iter().next())
                    .map(|name| {
                        let name = name.strip_prefix('/').map(String::from).unwrap_or(name);
                        docker::inspect(name)
                    })
            }),
    )
    .await
    .into_iter()
    .collect::<Result<_>>()?;
    join_all(
        containers
            .into_iter()
            .filter_map(|container| {
                container.state.clone().and_then(|state| {
                    state.finished_at.and_then(|finished_at| {
                        DateTime::parse_from_rfc3339(&finished_at)
                            .ok()
                            .map(|dt| (container, dt.timestamp()))
                    })
                })
            })
            .filter(|(_, dt)| dt < &finished_at_threshold)
            .filter_map(|(container, _)| {
                container.name.map(|name| {
                    let name = name.strip_prefix('/').map(String::from).unwrap_or(name);
                    info!("Cleaning job {:?}", name);
                    docker::remove(name)
                })
            }),
    )
    .await
    .into_iter()
    .collect::<Result<_>>()?;
    Ok(())
}

/// Maximum amount of consecutive cleaning errors.
const MAX_ERRORS: u8 = 5;

/// Loop the clean function endlessly.
pub async fn cycle(
    keep_exited_for: u32,
    scheduling_interval: u16,
    namespace: String,
) -> Result<()> {
    let mut interval = time::interval(Duration::from_secs(scheduling_interval.into()));
    let mut errors: u8 = 0;
    loop {
        interval.tick().await;
        let result = clean(keep_exited_for, &namespace).await;
        if let Err(ref e) = result {
            error!("Error while cleaning jobs: {:?}", e);
            errors += 1;
            if errors >= MAX_ERRORS {
                return result.context("received 5 consecutive cleaning errors");
            }
        } else {
            errors = 0;
        }
    }
}
