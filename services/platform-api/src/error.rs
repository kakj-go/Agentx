use agentx_api_types::{ApiErrorResponse, FieldError};
use axum::{
    Json,
    http::{HeaderValue, StatusCode, header::HeaderName},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub fields: Vec<FieldError>,
    pub details: Option<serde_json::Value>,
    database_error: Option<Box<DatabaseErrorContext>>,
}

#[derive(Debug)]
struct DatabaseErrorContext {
    index: Option<String>,
    database_code: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct UniqueConstraint {
    pub index: &'static str,
    pub code: &'static str,
    pub field: &'static str,
    pub message: &'static str,
}

impl AppError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            fields: Vec::new(),
            details: None,
            database_error: None,
        }
    }

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

    pub fn unique(constraint: UniqueConstraint) -> Self {
        Self::conflict(constraint.code, constraint.message).with_field(
            constraint.field,
            constraint.code,
            constraint.message,
        )
    }

    pub fn too_many_requests(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, code, message)
    }

    pub fn unprocessable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }

    pub fn service_unavailable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }

    pub fn with_field(
        mut self,
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.fields.push(FieldError {
            field: field.into(),
            code: code.into(),
            message: message.into(),
        });
        self
    }

    pub fn with_details(mut self, details: impl serde::Serialize) -> Self {
        self.details = Some(serde_json::to_value(details).unwrap_or_else(|error| {
            tracing::error!(%error, "failed to serialize API error details");
            serde_json::Value::Null
        }));
        self
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "request failed");
        #[cfg(test)]
        eprintln!("request failed: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "The request could not be completed",
        )
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let request_id = Uuid::now_v7();
        if let Some(database) = &self.database_error {
            tracing::error!(
                %request_id,
                unique_index = database.index.as_deref(),
                database_code = database.database_code.as_deref(),
                "unmapped database unique constraint"
            );
        }
        let mut response = (
            self.status,
            Json(ApiErrorResponse {
                code: self.code.to_owned(),
                message: self.message,
                request_id,
                field_errors: self.fields,
                details: self.details,
            }),
        )
            .into_response();
        response.headers_mut().insert(
            HeaderName::from_static("x-request-id"),
            HeaderValue::from_str(&request_id.to_string()).expect("UUID is a valid header value"),
        );
        response
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        if let sqlx::Error::Database(database) = &value {
            if database.is_unique_violation() {
                return Self {
                    database_error: Some(Box::new(DatabaseErrorContext {
                        index: mysql_unique_index(database.message()),
                        database_code: mysql_database_code(database.as_ref()),
                    })),
                    ..Self::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "INTERNAL_ERROR",
                        "The request could not be completed",
                    )
                };
            }
        }
        Self::internal(value)
    }
}

pub fn map_unique(error: sqlx::Error, constraints: &[UniqueConstraint]) -> AppError {
    if let sqlx::Error::Database(database) = &error {
        if database.is_unique_violation() {
            let index = mysql_unique_index(database.message());
            if let Some(constraint) = index.as_deref().and_then(|index| {
                constraints
                    .iter()
                    .find(|constraint| constraint.index == index)
            }) {
                return AppError::unique(*constraint);
            }
        }
    }
    AppError::from(error)
}

fn mysql_unique_index(message: &str) -> Option<String> {
    let marker = "for key ";
    let tail = message.rsplit_once(marker)?.1.trim();
    let quoted = tail
        .strip_prefix('\'')
        .and_then(|value| value.split_once('\'').map(|(value, _)| value))
        .or_else(|| {
            tail.strip_prefix('`')
                .and_then(|value| value.split_once('`').map(|(value, _)| value))
        })
        .unwrap_or_else(|| tail.split_whitespace().next().unwrap_or(tail));
    Some(
        quoted
            .trim_matches(|character| character == '\'' || character == '`')
            .rsplit('.')
            .next()
            .unwrap_or(quoted)
            .to_owned(),
    )
}

fn mysql_database_code(database: &(dyn sqlx::error::DatabaseError + 'static)) -> Option<String> {
    database
        .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
        .map(|error| error.number().to_string())
        .or_else(|| database.code().map(|code| code.into_owned()))
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::mysql_unique_index;

    #[test]
    fn extracts_mysql_unique_index_names_without_values() {
        assert_eq!(
            mysql_unique_index("Duplicate entry 'secret' for key 'model_aliases.uq_model_alias'"),
            Some("uq_model_alias".to_owned())
        );
        assert_eq!(
            mysql_unique_index("Duplicate entry 'secret' for key `uq_skill_alias`"),
            Some("uq_skill_alias".to_owned())
        );
    }
}
