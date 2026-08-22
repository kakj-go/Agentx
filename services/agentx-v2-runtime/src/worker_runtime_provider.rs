use reqwest::header::HeaderMap;
use serde_json::Value;

use super::{WorkerProvider, WorkerProviderError, WorkerProviderResponse};
use crate::egress::{EgressRequestContext, ProviderHttpClient};

#[async_trait::async_trait]
impl WorkerProvider for ProviderHttpClient {
    async fn post_json(
        &self,
        endpoint: &str,
        context: EgressRequestContext,
        timeout: std::time::Duration,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<WorkerProviderResponse, WorkerProviderError> {
        let mut request = self
            .post(endpoint, context, timeout)
            .map_err(|error| WorkerProviderError::Denied(error.to_string()))?;
        for (name, value) in headers {
            if let Some(name) = name {
                request = request.header(name, value);
            }
        }
        response(request.json(body).send().await).await
    }

    async fn post_sandbox_manager_json(
        &self,
        endpoint: &str,
        _context: EgressRequestContext,
        timeout: std::time::Duration,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<WorkerProviderResponse, WorkerProviderError> {
        let mut request = self
            .post_sandbox_manager(endpoint, timeout)
            .map_err(|error| WorkerProviderError::Denied(error.to_string()))?;
        for (name, value) in headers {
            if let Some(name) = name {
                request = request.header(name, value);
            }
        }
        reqwest_response(request.json(body).send().await).await
    }

    async fn request_json(
        &self,
        method: &str,
        endpoint: &str,
        context: EgressRequestContext,
        timeout: std::time::Duration,
        headers: HeaderMap,
        body: Option<&Value>,
    ) -> Result<WorkerProviderResponse, WorkerProviderError> {
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|error| WorkerProviderError::Denied(error.to_string()))?;
        let mut request = self
            .request(method, endpoint, context, timeout)
            .map_err(|error| WorkerProviderError::Denied(error.to_string()))?;
        for (name, value) in headers {
            if let Some(name) = name {
                request = request.header(name, value);
            }
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        response(request.send().await).await
    }
}

async fn response(
    result: Result<reqwest::Response, crate::egress::ProviderRequestError>,
) -> Result<WorkerProviderResponse, WorkerProviderError> {
    let response = result.map_err(|error| WorkerProviderError::Request {
        message: error.to_string(),
        is_connect: error.is_connect(),
    })?;
    Ok(WorkerProviderResponse {
        status: response.status(),
        headers: response.headers().clone(),
        body: response
            .bytes()
            .await
            .map_err(|error| WorkerProviderError::Request {
                message: error.to_string(),
                is_connect: error.is_connect(),
            })?,
    })
}

async fn reqwest_response(
    result: Result<reqwest::Response, reqwest::Error>,
) -> Result<WorkerProviderResponse, WorkerProviderError> {
    let response = result.map_err(|error| WorkerProviderError::Request {
        message: error.to_string(),
        is_connect: error.is_connect(),
    })?;
    Ok(WorkerProviderResponse {
        status: response.status(),
        headers: response.headers().clone(),
        body: response
            .bytes()
            .await
            .map_err(|error| WorkerProviderError::Request {
                message: error.to_string(),
                is_connect: error.is_connect(),
            })?,
    })
}
