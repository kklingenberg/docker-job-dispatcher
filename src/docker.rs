//! Defines the global docker client.

use anyhow::{Context, Result};
use bollard::{
    container::{Config, CreateContainerOptions, ListContainersOptions},
    errors::Error,
    models::{ContainerCreateResponse, ContainerInspectResponse, ContainerSummary},
    Docker,
};
use clap::ValueEnum;
use once_cell::sync::OnceCell;
use std::collections::HashMap;

/// Static docker client instance.
static CURRENT: OnceCell<Docker> = OnceCell::new();

/// A means of connecting to the docker daemon.
#[derive(Clone, ValueEnum)]
pub enum Transport {
    Http,
    Tls,
    Socket,
}

/// Initialize the global docker client instance.
pub fn init(transport: Transport) -> Result<()> {
    let _ = CURRENT.set(match transport {
        Transport::Http => Docker::connect_with_http_defaults()
            .context("while connecting to the docker daemon via HTTP")?,
        Transport::Tls => Docker::connect_with_ssl_defaults()
            .context("while connecting to the docker daemon via HTTP over TLS")?,
        Transport::Socket => Docker::connect_with_unix_defaults()
            .context("while connecting to the docker daemon via socket")?,
    });
    Ok(())
}

/// Get the static docker client instance.
fn client() -> Result<&'static Docker> {
    CURRENT
        .get()
        .context("docker client has not been initialized")
}

/// Test the connection with the docker daemon.
pub async fn ping() -> Result<()> {
    client()?.ping().await?;
    Ok(())
}

/// A label key to use when annotating containers.
const JOB_LABEL_KEY: &str = concat!(env!("CARGO_PKG_NAME"), ".namespace");

/// Insert the grouping annotation into a container configuration.
fn insert_job_label(c: Config<String>, namespace: &str) -> Config<String> {
    let mut labels = c.labels.unwrap_or_default();
    labels.insert(JOB_LABEL_KEY.to_string(), namespace.to_string());
    Config {
        labels: Some(labels),
        ..c
    }
}

/// Create a job with the given name and platform option, and the
/// specified configuration. The namespace parameter is included as a
/// custom label in the container, used to group jobs created by this
/// dispatcher.
pub async fn create(
    name: String,
    platform: Option<String>,
    config: Config<String>,
    namespace: &str,
) -> Result<Option<ContainerCreateResponse>> {
    client()?
        .create_container(
            Some(CreateContainerOptions { name, platform }),
            insert_job_label(config, namespace),
        )
        .await
        .map_or_else(
            |e| match e {
                Error::DockerResponseServerError {
                    status_code: 409, ..
                } => Ok(None),
                _ => Err(anyhow::Error::new(e)),
            },
            |response| Ok(Some(response)),
        )
}

/// Start a previously created job.
pub async fn start<S: AsRef<str>>(container: S) -> Result<()> {
    client()?
        .start_container::<String>(container.as_ref(), None)
        .await?;
    Ok(())
}

/// Get a possibly non-existent job.
pub async fn get<S: AsRef<str>>(name: S, namespace: &str) -> Result<Option<ContainerSummary>> {
    let mut filters = HashMap::new();
    let name_regex = format!("^/{}$", name.as_ref());
    filters.insert("name", vec![name_regex.as_str()]);
    let label_filter = format!("{}={}", JOB_LABEL_KEY, namespace);
    filters.insert("label", vec![label_filter.as_str()]);
    let options = ListContainersOptions {
        all: true,
        limit: Some(1),
        size: false,
        filters,
    };
    Ok(client()?
        .list_containers(Some(options))
        .await
        .map(|containers| containers.into_iter().next())?)
}

/// Inspect a possibly non-existent job.
pub async fn inspect<S: AsRef<str>>(name: S) -> Result<ContainerInspectResponse> {
    Ok(client()?.inspect_container(name.as_ref(), None).await?)
}

/// Remove a job.
pub async fn remove<S: AsRef<str>>(name: S) -> Result<()> {
    Ok(client()?.remove_container(name.as_ref(), None).await?)
}

/// Count the number of currently active jobs.
pub async fn count_active(namespace: &str) -> Result<usize> {
    let mut filters = HashMap::new();
    filters.insert("status", vec!["restarting", "running"]);
    let label_filter = format!("{}={}", JOB_LABEL_KEY, namespace);
    filters.insert("label", vec![label_filter.as_str()]);
    let options = ListContainersOptions {
        all: true,
        limit: None,
        size: false,
        filters,
    };
    Ok(client()?
        .list_containers(Some(options))
        .await
        .map(|containers| containers.len())?)
}

/// Get jobs by their status, in order from oldest to newest.
async fn get_by_status(namespace: &str, status: &str) -> Result<Vec<ContainerSummary>> {
    let mut filters = HashMap::new();
    filters.insert("status", vec![status]);
    let label_filter = format!("{}={}", JOB_LABEL_KEY, namespace);
    filters.insert("label", vec![label_filter.as_str()]);
    let options = ListContainersOptions {
        all: true,
        limit: None,
        size: false,
        filters,
    };
    Ok(client()?
        .list_containers(Some(options))
        .await
        .map(|mut containers| {
            containers.sort_unstable_by_key(|container| container.created);
            containers
        })?)
}

/// Get the not-yet-started jobs.
pub async fn get_pending(namespace: &str) -> Result<Vec<ContainerSummary>> {
    get_by_status(namespace, "created").await
}

/// Get the exited jobs.
pub async fn get_exited(namespace: &str) -> Result<Vec<ContainerSummary>> {
    get_by_status(namespace, "exited").await
}
