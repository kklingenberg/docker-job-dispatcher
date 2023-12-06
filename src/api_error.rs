//! Provides an error type for API responses.

use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use serde::Serialize;
use serde_json::{json, to_string_pretty};
use std::fmt::{Display, Formatter, Result};

/// An error serialized as JSON and sent as a response.
#[derive(Debug, Serialize)]
pub struct APIError {
    status: u16,
    msg: String,
}

impl APIError {
    fn new<S: ToString>(status: u16, msg: S) -> Self {
        Self {
            status,
            msg: msg.to_string(),
        }
    }

    pub fn bad_request<S: ToString>(msg: S) -> Self {
        Self::new(400, msg)
    }

    pub fn bad_gateway<S: ToString>(msg: S) -> Self {
        Self::new(502, msg)
    }

    pub fn not_found<S: ToString>(msg: S) -> Self {
        Self::new(404, msg)
    }
}

impl Display for APIError {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{}", to_string_pretty(self).unwrap())
    }
}

impl ResponseError for APIError {
    fn error_response(&self) -> HttpResponse {
        let err_json = json!({ "error": { "code": self.status, "message": self.msg }});
        HttpResponse::build(StatusCode::from_u16(self.status).unwrap()).json(err_json)
    }
}
