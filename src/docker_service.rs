//! Implements the creation and retrieval of jobs.

use crate::docker;
use crate::jq;

use actix_web::{error, get, post, web, HttpResponse, Responder, Result};
use bollard::container::Config;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info};

/// A representation of a job.
#[derive(Serialize)]
struct JobSummary {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

/// Additional fields from the job manifest.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CreateContainerOptions {
    name: String,
    platform: Option<String>,
}

/// Create a job by converting the request body to a job manifest.
#[post("/job")]
async fn create_job(
    body: web::Json<Value>,
    filter: web::Data<jq::Filter>,
    can_start: web::Data<bool>,
    namespace: web::Data<String>,
) -> Result<impl Responder> {
    debug!("Job creation request: {:?}", body);
    let raw_manifest = jq::first_result(&filter, body.into_inner())
        .ok_or_else(|| error::ErrorBadRequest("Filter didn't produce results"))?
        .map_err(|e| error::ErrorBadRequest(format!("Filter failed: {:?}", e)))?;
    debug!("Job raw manifest: {:?}", raw_manifest);
    let options: CreateContainerOptions = serde_json::from_value(raw_manifest.clone())
        .map_err(|e| error::ErrorBadRequest(format!("Generated manifest is invalid: {:?}", e)))?;
    let manifest: Config<String> = serde_json::from_value(raw_manifest)
        .map_err(|e| error::ErrorBadRequest(format!("Generated manifest is invalid: {:?}", e)))?;
    debug!("Job manifest: {:?} {:?}", options, manifest);
    let job_opt = docker::create(
        options.name.clone(),
        options.platform.clone(),
        manifest,
        &namespace,
    )
    .await
    .map_err(|e| error::ErrorBadRequest(format!("Server rejected job manifest: {:?}", e)))?;
    if job_opt.is_some() {
        info!("Created job with ID {:?}", options.name);
        if **can_start {
            docker::start(&options.name)
                .await
                .map_err(error::ErrorBadGateway)?;
        }
        Ok(HttpResponse::Created().json(JobSummary {
            id: options.name,
            created: None,
            status: None,
        }))
    } else {
        info!("Pre-existing job with ID {:?}", options.name);
        Ok(HttpResponse::Ok().json(JobSummary {
            id: options.name,
            created: None,
            status: None,
        }))
    }
}

/// Fetch a job by its ID.
#[get("/job/{id}")]
async fn get_job(id: web::Path<String>, namespace: web::Data<String>) -> Result<impl Responder> {
    let job = docker::get(&*id, &namespace)
        .await
        .map_err(error::ErrorBadGateway)?
        .ok_or_else(|| error::ErrorNotFound("The specified job doesn't exist"))?;
    info!("Fetched job with ID {:?}", &*id);
    Ok(web::Json(JobSummary {
        id: id.clone(),
        created: job.created,
        status: job.status,
    }))
}
