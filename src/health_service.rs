//! Implements the liveness and readiness checks.

use crate::docker;

use actix_web::{error, get, HttpResponse, Responder, Result};

/// Liveness check: if this function can execute, the process is
/// alive.
#[get("/health/live")]
async fn liveness_check() -> impl Responder {
    HttpResponse::NoContent().finish()
}

/// Readiness check: if the docker API responds, the process is ready
/// to receive commands.
#[get("/health/ready")]
async fn readiness_check() -> Result<impl Responder> {
    docker::ping()
        .await
        .map_err(error::ErrorServiceUnavailable)?;
    Ok(HttpResponse::NoContent().finish())
}
