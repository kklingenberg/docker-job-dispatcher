//! Implements the poll-based scheduling task.

use crate::docker;
use anyhow::Result;
use futures::future::join_all;
use tokio::time::{self, Duration};
use tracing::{error, info};

/// Check running containers, and begin starting containers if there's
/// room for them accoring to the given quota.
async fn schedule(max_concurrent: usize, namespace: &str) -> Result<()> {
    let active = docker::count_active(namespace).await?;
    if max_concurrent > active {
        let result = join_all(
            docker::get_pending(namespace)
                .await?
                .into_iter()
                .take(max_concurrent - active)
                .filter_map(|container| {
                    container
                        .names
                        .and_then(|ns| ns.into_iter().next())
                        .map(|name| {
                            let name = name.strip_prefix('/').map(String::from).unwrap_or(name);
                            info!("Scheduling job {:?}", name);
                            docker::start(name)
                        })
                }),
        )
        .await
        .into_iter()
        .collect::<Result<Vec<()>>>();
        if let Err(e) = result {
            error!("Error while scheduling jobs: {:?}", e);
        }
    }
    Ok(())
}

/// Loop the schedule function endlessly.
pub async fn cycle(max_concurrent: u16, scheduling_interval: u16, namespace: String) -> Result<()> {
    let mut interval = time::interval(Duration::from_secs(scheduling_interval.into()));
    loop {
        interval.tick().await;
        schedule(max_concurrent.into(), &namespace).await?;
    }
}
