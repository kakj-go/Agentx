use agentx_api_types::{ApiErrorResponse, FieldError};
use axum::{
    Json,
    http::{HeaderValue, StatusCode, header::HeaderName},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    field_errors: Vec<FieldError>,
    retry_after_seconds: Option<u64>,
}

pub fn invalid_credentials() -> ApiError {
    ApiError::unauthorized("INVALID_CREDENTIALS", "Username or password is incorrect")
}

impl ApiError {
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "FORBIDDEN", message)
    }

    pub fn not_found(resource: &'static str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("{resource} was not found"),
        )
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn unprocessable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }

    pub fn unavailable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }

    pub fn accepted(code: &'static str, message: impl Into<String>, retry_after: u64) -> Self {
        let mut error = Self::new(StatusCode::ACCEPTED, code, message);
        error.retry_after_seconds = Some(retry_after);
        error
    }

    pub fn with_field_error(
        mut self,
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.field_errors.push(FieldError {
            field: field.into(),
            code: code.into(),
            message: message.into(),
        });
        self
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "Control API request failed");
        #[cfg(test)]
        eprintln!("Control API request failed: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "The request could not be completed",
        )
    }

    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            field_errors: Vec::new(),
            retry_after_seconds: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let request_id = Uuid::now_v7();
        let mut response = (
            self.status,
            Json(ApiErrorResponse {
                code: self.code.to_owned(),
                message: self.message,
                request_id,
                field_errors: self.field_errors,
                details: None,
            }),
        )
            .into_response();
        response.headers_mut().insert(
            HeaderName::from_static("x-request-id"),
            HeaderValue::from_str(&request_id.to_string()).expect("UUID header is valid"),
        );
        if let Some(seconds) = self.retry_after_seconds {
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                HeaderValue::from_str(&seconds.to_string()).expect("retry seconds are valid"),
            );
        }
        response
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(value: sqlx::Error) -> Self {
        Self::internal(value)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
