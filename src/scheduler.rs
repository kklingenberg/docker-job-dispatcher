//! Implements the poll-based scheduling task.

use crate::docker;
use anyhow::{Context, Result};
use futures::future::join_all;
use tokio::time::{self, Duration};
use tracing::{error, info};

/// Check running containers, and begin starting containers if there's
/// room for them accoring to the given quota.
async fn schedule(max_concurrent: usize, namespace: &str) -> Result<()> {
    let active = docker::count_active(namespace)
        .await
        .context("while counting active jobs")?;
    if max_concurrent > active {
        join_all(
            docker::get_pending(namespace)
                .await
                .context("while fetching pending jobs")?
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
        .collect::<Result<_>>()?;
    }
    Ok(())
}

/// Maximum amount of consecutive scheduling errors.
const MAX_ERRORS: u8 = 5;

/// Loop the schedule function endlessly.
pub async fn cycle(max_concurrent: u16, scheduling_interval: u16, namespace: String) -> Result<()> {
    let mut interval = time::interval(Duration::from_secs(scheduling_interval.into()));
    let mut errors: u8 = 0;
    loop {
        interval.tick().await;
        let result = schedule(max_concurrent.into(), &namespace).await;
        if let Err(ref e) = result {
            error!("Error while scheduling jobs: {:?}", e);
            errors += 1;
            if errors >= MAX_ERRORS {
                return result.context("received 5 consecutive scheduling errors");
            }
        } else {
            errors = 0;
        }
    }
}
