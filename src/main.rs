mod api_error;
mod cleaner;
mod docker;
mod docker_service;
mod health_service;
mod jq;
mod metrics_service;
mod scheduler;

use actix_web::{
    http::header::ContentType, middleware, web, App, Error, HttpResponse, HttpServer,
    Result as RouteResult,
};
use anyhow::Result;
use clap::{value_parser, Parser};
use std::path::PathBuf;
use tracing::{info, warn};
use utoipa_rapidoc::RapiDoc;

const DEFAULT_FILTER: &str = include_str!("default_filter.jq");

/// Job-dispatching interface acting as a docker container scheduler.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Filter converting requests to container manifests
    filter: Option<String>,

    /// Read filter from a file
    #[arg(short, long, env)]
    from_file: Option<PathBuf>,

    /// TCP port to listen on
    #[arg(short, long, env, default_value_t = 8000)]
    port: u16,

    /// Maximum number of concurrently-running containers; default is
    /// unlimited; set to 0 to never start jobs
    #[arg(short, long, env)]
    max_concurrent: Option<u16>,

    /// Interval in seconds to keep an exited job; default is to keep
    /// them forever
    #[arg(short, long, env)]
    keep_exited_for: Option<u32>,

    /// Interval in seconds to perform periodic scheduling and cleanup
    /// upkeep
    #[arg(short, long, env, value_parser = value_parser!(u16).range(1..), default_value_t = 3)]
    upkeep_interval: u16,

    /// Means of connection to the docker daemon
    #[arg(short, long, env, value_enum, default_value_t = docker::Transport::Socket)]
    transport: docker::Transport,

    /// Label applied to jobs created to group them
    #[arg(short, long, env, default_value_t = String::from("default"))]
    namespace: String,

    /// Log level
    #[arg(long, env, default_value_t = tracing::Level::INFO)]
    log_level: tracing::Level,
}

/// Default 404 response
async fn no_route() -> RouteResult<HttpResponse> {
    Err::<_, Error>(api_error::APIError::not_found("Route not found").into())
}

/// OpenAPI schema
const OPENAPI: &str = include_str!("openapi.json");

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_max_level(cli.log_level)
        .with_target(false)
        .without_time()
        .init();

    // Initialize application state
    let filter_source = if let Some(filter_file) = cli.from_file {
        if cli.filter.is_some() {
            warn!("Filter given both as file and argument; argument will be ignored");
        }
        std::fs::read_to_string(filter_file)
    } else if let Some(filter_str) = cli.filter {
        Ok(filter_str)
    } else {
        warn!("No filter given; the default filter will be used");
        Ok(DEFAULT_FILTER.to_string())
    }?;
    let filter = web::Data::new(jq::compile(&filter_source)?);
    let containers_can_start = web::Data::new(cli.max_concurrent.is_none());
    let namespace = web::Data::new(cli.namespace.clone());
    docker::init(cli.transport)?;

    // Prepare the HTTP server and metrics consumer
    let api = HttpServer::new(move || {
        App::new()
            .wrap(middleware::NormalizePath::trim())
            .app_data(filter.clone())
            .app_data(containers_can_start.clone())
            .app_data(namespace.clone())
            .service(health_service::liveness_check)
            .service(health_service::readiness_check)
            .service(metrics_service::expose)
            .service(docker_service::create_job)
            .service(docker_service::get_job)
            .route(
                "/openapi.json",
                web::get().to(|| async {
                    HttpResponse::Ok()
                        .content_type(ContentType::json())
                        .body(OPENAPI)
                }),
            )
            .service(RapiDoc::new("/openapi.json").path("/docs"))
            .default_service(web::route().to(no_route))
    })
    .bind(("0.0.0.0", cli.port))?;
    let metrics_task = tokio::spawn(metrics_service::run(cli.namespace.clone()));
    let core_task = || async {
        tokio::select! {
            api_result = api.run() => api_result?,
            metrics_result = metrics_task => match metrics_result {
                Ok(inner_error @ Err(_)) => inner_error?,
                Err(e) => Err(e)?,
                _ => ()
            }
        };
        Ok::<(), anyhow::Error>(())
    };

    // Start the API and optionally start the job scheduler and cleaner
    match (cli.max_concurrent, cli.keep_exited_for) {
        // full-featured: scheduler and cleaner
        (Some(max_concurrent), Some(keep_exited_for)) if max_concurrent > 0 => {
            info!(
                "Using a scheduler for {max_concurrent} concurrent containers, \
                 scheduling every {} seconds",
                cli.upkeep_interval
            );
            info!(
                "Using a cleaner for exited jobs older than {keep_exited_for} \
                 seconds, cleaning every {} seconds",
                cli.upkeep_interval
            );
            let scheduling_task = tokio::spawn(scheduler::cycle(
                max_concurrent,
                cli.upkeep_interval,
                cli.namespace.clone(),
            ));
            let cleaning_task = tokio::spawn(cleaner::cycle(
                keep_exited_for,
                cli.upkeep_interval,
                cli.namespace,
            ));
            tokio::select! {
                core_result = core_task() => core_result?,
                scheduling_result = scheduling_task => match scheduling_result {
                    Ok(inner_error @ Err(_)) => inner_error?,
                    Err(e) => Err(e)?,
                    _ => ()
                },
                cleaning_result = cleaning_task => match cleaning_result {
                    Ok(inner_error @ Err(_)) => inner_error?,
                    Err(e) => Err(e)?,
                    _ => ()
                }
            }
        }
        // only scheduler
        (Some(max_concurrent), None) if max_concurrent > 0 => {
            info!(
                "Using a scheduler for {max_concurrent} concurrent containers, \
                 scheduling every {} seconds",
                cli.upkeep_interval
            );
            warn!("Exited jobs will be kept indefinitely");
            let scheduling_task = tokio::spawn(scheduler::cycle(
                max_concurrent,
                cli.upkeep_interval,
                cli.namespace,
            ));
            tokio::select! {
                core_result = core_task() => core_result?,
                scheduling_result = scheduling_task => match scheduling_result {
                    Ok(inner_error @ Err(_)) => inner_error?,
                    Err(e) => Err(e)?,
                    _ => ()
                }
            }
        }
        // only cleaner
        (_, Some(keep_exited_for)) => {
            if matches!(cli.max_concurrent, Some(max_concurrent) if max_concurrent == 0) {
                warn!("Maximum concurrent jobs set to 0; containers won't be started");
            }
            info!(
                "Using a cleaner for exited jobs older than {keep_exited_for} \
                 seconds, cleaning every {} seconds",
                cli.upkeep_interval
            );
            let cleaning_task = tokio::spawn(cleaner::cycle(
                keep_exited_for,
                cli.upkeep_interval,
                cli.namespace,
            ));
            tokio::select! {
                core_result = core_task() => core_result?,
                cleaning_result = cleaning_task => match cleaning_result {
                    Ok(inner_error @ Err(_)) => inner_error?,
                    Err(e) => Err(e)?,
                    _ => ()
                }
            }
        }
        // neither scheduler nor cleaner
        _ => {
            if matches!(cli.max_concurrent, Some(max_concurrent) if max_concurrent == 0) {
                warn!("Maximum concurrent jobs set to 0; containers won't be started");
            }
            warn!("Exited jobs will be kept indefinitely");
            core_task().await?;
        }
    }

    Ok(())
}
