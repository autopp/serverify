use axum::{http::StatusCode, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct ServerifyError {
    message: String,
}

#[derive(Serialize)]
pub struct ErrorResBody {
    pub serverify_error: ServerifyError,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum WithError<T: Serialize> {
    Success(T),
    Error(ErrorResBody),
}

pub fn success_response<T: Serialize>(
    status_code: StatusCode,
    body: T,
) -> (StatusCode, Json<WithError<T>>) {
    (status_code, Json(WithError::Success(body)))
}

pub fn error_response<T: Serialize>(
    status_code: StatusCode,
    message: impl ToString,
) -> (StatusCode, Json<WithError<T>>) {
    (
        status_code,
        Json(WithError::Error(ErrorResBody {
            serverify_error: ServerifyError {
                message: message.to_string(),
            },
        })),
    )
}
