use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Router,
};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::{
    request_logger::{LoggerError, RequestLog},
    response::{error_response, success_response, WithError},
    state::AppState,
};

pub fn route_session_to(app: Router<AppState>) -> Router<AppState> {
    app.route("/session", post(create_session))
        .route("/session/:session", get(get_session))
        .route("/session/:session", delete(delete_session))
}

#[derive(serde::Deserialize)]
struct CreateReqBody {
    session: String,
}

#[derive(serde::Serialize)]
struct CreateResBody {
    session: String,
}

static SESSION_NAME_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[-a-zA-Z0-9_]+$").unwrap());

async fn create_session(
    State(state): State<AppState>,
    Json(CreateReqBody { session }): Json<CreateReqBody>,
) -> impl IntoResponse {
    if !SESSION_NAME_REGEX.is_match(&session) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "session name should contains only alphanumeric, hyphen or underscore",
        );
    }

    match state.logger.create_session(&session).await {
        Ok(_) => success_response(StatusCode::CREATED, CreateResBody { session }),
        Err(LoggerError::InvalidSession(message)) => error_response(StatusCode::CONFLICT, message),
        Err(LoggerError::InternalError(message)) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    }
}

#[derive(serde::Serialize)]
struct GetResBody {
    histories: Vec<RequestLog>,
}

async fn get_session(
    State(state): State<AppState>,
    Path(session): Path<String>,
) -> (StatusCode, Json<WithError<GetResBody>>) {
    match state.logger.get_session_history(&session).await {
        Ok(histories) => success_response(StatusCode::OK, GetResBody { histories }),
        Err(LoggerError::InvalidSession(message)) => error_response(StatusCode::NOT_FOUND, message),
        Err(LoggerError::InternalError(message)) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    }
}

#[derive(serde::Serialize)]
struct DeleteResBody {
    session: String,
}

async fn delete_session(
    State(state): State<AppState>,
    Path(session): Path<String>,
) -> impl IntoResponse {
    match state.logger.delete_session(&session).await {
        Ok(_) => success_response(StatusCode::OK, DeleteResBody { session }),
        Err(LoggerError::InvalidSession(message)) => error_response(StatusCode::NOT_FOUND, message),
        Err(LoggerError::InternalError(message)) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::{method::Method, request_logger::testutil::new_logger};

    use super::*;
    use axum_test::TestServer;
    use chrono::{Local, NaiveDate, TimeZone};
    use indexmap::indexmap;
    use pretty_assertions::assert_eq;
    use rstest::*;
    use serde_json::{json, Value};

    const EXIST_SESSION: &str = "exist_session";

    async fn new_test_server_with_default_session() -> (TestServer, AppState) {
        let logger = new_logger().await;
        logger.create_session(EXIST_SESSION).await.unwrap();

        let requested_at = Local
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2024, 1, 2)
                    .unwrap()
                    .and_hms_opt(3, 4, 5)
                    .unwrap(),
            )
            .unwrap();
        logger
            .log_request(
                EXIST_SESSION,
                &RequestLog {
                    method: Method::Post,
                    path: "/greet".to_string(),
                    headers: indexmap! {
                        "token".to_string() => "abc".to_string()
                    },
                    query: indexmap! {
                        "answer".to_string() => "42".to_string(),
                    },
                    body: r#"{"message":"hello"}"#.to_string(),
                    requested_at,
                },
            )
            .await
            .unwrap();

        let state = AppState { logger };
        (
            TestServer::new(route_session_to(Router::new()).with_state(state.clone())).unwrap(),
            state,
        )
    }

    mod create_session {
        use super::*;
        use pretty_assertions::assert_eq;

        #[tokio::test]
        async fn success_case() {
            let (server, state) = new_test_server_with_default_session().await;

            let response = server
                .post("/session")
                .json(&json!({ "session": "mysession" }))
                .await;

            assert_eq!(
                (StatusCode::CREATED, json!({ "session": "mysession" })),
                (response.status_code(), response.json()),
            );

            assert_eq!(
                Ok(vec![]),
                state.logger.get_session_history("mysession").await
            );
        }

        #[tokio::test]
        async fn when_already_exists() {
            let (server, _) = new_test_server_with_default_session().await;

            let response = server
                .post("/session")
                .json(&json!({ "session": EXIST_SESSION }))
                .await;

            assert_eq!(
                (
                    StatusCode::CONFLICT,
                    json!({ "serverify_error": { "message": "session \"exist_session\" already exists" } })
                ),
                (response.status_code(), response.json()),
            );
        }

        #[tokio::test]
        async fn when_invalid_session_name() {
            let (server, state) = new_test_server_with_default_session().await;

            let response = server
                .post("/session")
                .json(&json!({ "session": "invalid session" }))
                .await;

            assert_eq!(
                (
                    StatusCode::BAD_REQUEST,
                    json!({ "serverify_error": { "message": "session name should contains only alphanumeric, hyphen or underscore" } })
                ),
                (response.status_code(), response.json()),
            );

            assert_eq!(
                Err(LoggerError::InvalidSession(
                    "session \"invalid session\" is not found".to_string()
                )),
                state.logger.get_session_history("invalid session").await
            );
        }
    }

    #[rstest]
    #[tokio::test]
    #[case(
        "success case",
        "exist_session",
        StatusCode::OK,
        json!({
            "histories": [
                {
                    "method": "post",
                    "path": "/greet",
                    "headers": {
                        "token": "abc"
                    },
                    "query": {"answer": "42" },
                    "body": r#"{"message":"hello"}"#,
                    "requested_at": "2024-01-02T03:04:05+09:00"
                }
            ]
        }),

    )]
    #[tokio::test]
    #[case(
        "session dose not exist",
        "undefined_session",
        StatusCode::NOT_FOUND,
        json!({ "serverify_error": { "message": "session \"undefined_session\" is not found" } }),
    )]
    async fn get_session(
        #[case] title: &str,
        #[case] session: &str,
        #[case] expected_status_code: StatusCode,
        #[case] expected_res_body: Value,
    ) {
        let (server, _) = new_test_server_with_default_session().await;

        let response = server.get(&format!("/session/{}", session)).await;

        assert_eq!(
            (expected_status_code, expected_res_body),
            (response.status_code(), response.json()),
            "{}: response",
            title
        );
    }

    #[rstest]
    #[tokio::test]
    #[case(
        "success case",
        "exist_session",
        StatusCode::OK,
        json!({ "session": "exist_session" }),
        false,
    )]
    #[tokio::test]
    #[case(
        "session is not found",
        "undefined_session",
        StatusCode::NOT_FOUND,
        json!({ "serverify_error": { "message": "session \"undefined_session\" is not found" } }),
        true
    )]
    async fn delete_session(
        #[case] title: &str,
        #[case] session: &str,
        #[case] expected_status_code: StatusCode,
        #[case] expected_res_body: Value,
        #[case] expected_exist_session_found: bool,
    ) {
        let (server, state) = new_test_server_with_default_session().await;

        let response = server.delete(&format!("/session/{}", session)).await;

        assert_eq!(
            (expected_status_code, expected_res_body),
            (response.status_code(), response.json()),
            "{}: response",
            title
        );

        assert_eq!(
            expected_exist_session_found,
            state
                .logger
                .get_session_history(EXIST_SESSION)
                .await
                .is_ok(),
        );
    }
}
