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
    response::{error_response, success_response, WithError},
    state::SharedState,
};

pub fn route_session_to(app: Router<SharedState>) -> Router<SharedState> {
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
    State(state): State<SharedState>,
    Json(CreateReqBody { session }): Json<CreateReqBody>,
) -> impl IntoResponse {
    if !SESSION_NAME_REGEX.is_match(&session) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "session name should contains only alphanumeric, hyphen or underscore",
        );
    }

    let sessions = &mut state.write().unwrap().sessions;

    if sessions.contains_key(&session) {
        return error_response(
            StatusCode::CONFLICT,
            format!("session {} already exists", session),
        );
    }

    sessions.insert(session.clone(), vec![]);
    success_response(StatusCode::CREATED, CreateResBody { session })
}

#[derive(serde::Serialize)]
struct GetResBody {
    histories: Vec<crate::history::History>,
}

async fn get_session(
    State(state): State<SharedState>,
    Path(session): Path<String>,
) -> (StatusCode, Json<WithError<GetResBody>>) {
    let sessions = &state.read().unwrap().sessions;
    match sessions.get(&session) {
        Some(history) => success_response(
            StatusCode::OK,
            GetResBody {
                histories: history.clone(),
            },
        ),
        None => error_response(
            StatusCode::NOT_FOUND,
            format!("session {} is not found", session),
        ),
    }
}

#[derive(serde::Serialize)]
struct DeleteResBody {
    session: String,
}

async fn delete_session(
    State(state): State<SharedState>,
    Path(session): Path<String>,
) -> impl IntoResponse {
    let sessions = &mut state.write().unwrap().sessions;
    match sessions.shift_remove(&session) {
        Some(_) => success_response(StatusCode::OK, DeleteResBody { session }),
        None => error_response(
            StatusCode::NOT_FOUND,
            format!("session {} is not found", session),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{history::History, mock_endpoint::Method, state::testutil::new_state_with};

    use super::*;
    use axum_test::TestServer;
    use indexmap::{indexmap, IndexMap};
    use pretty_assertions::assert_eq;
    use rstest::*;
    use serde_json::{json, Value};

    fn initial_sessions() -> IndexMap<String, Vec<History>> {
        indexmap! {
            "exist_session".to_string() => vec![History {
                method: Method::Post,
                path: "/greet".to_string(),
                headers: indexmap! {
                    "token".to_string() => "abc".to_string()
                },
                query: indexmap! {
                    "answer".to_string() => "42".to_string(),
                },
                body: r#"{"message":"hello"}"#.to_string(),
            }],
        }
    }

    #[rstest]
    #[tokio::test]
    #[case(
        "success case",
        json!({ "session": "mysession" }),
        StatusCode::CREATED,
        json!({ "session": "mysession" }),
        {
            let mut sessions = initial_sessions();
            sessions.insert("mysession".to_string(), vec![]);
            sessions
        },
    )]
    #[tokio::test]
    #[case(
        "session already exists",
        json!({ "session": "exist_session" }),
        StatusCode::CONFLICT,
        json!({ "serverify_error": { "message": "session exist_session already exists" } }),
        initial_sessions()
    )]
    #[tokio::test]
    #[case(
        "invalid state name",
        json!({ "session": "invalid session" }),
        StatusCode::BAD_REQUEST,
        json!({ "serverify_error": { "message": "session name should contains only alphanumeric, hyphen or underscore" } }),
        initial_sessions()
    )]
    async fn create_session(
        #[case] title: &str,
        #[case] req_body: Value,
        #[case] expected_status_code: StatusCode,
        #[case] expected_res_body: Value,
        #[case] expected_sessions: IndexMap<String, Vec<History>>,
    ) {
        let state = new_state_with(initial_sessions());
        let app = route_session_to(Router::new()).with_state(Arc::clone(&state));
        let server = TestServer::new(app).unwrap();

        let response = server.post("/session").json(&req_body).await;

        assert_eq!(
            (expected_status_code, expected_res_body),
            (response.status_code(), response.json()),
            "{}: response",
            title
        );

        assert_eq!(
            expected_sessions,
            state.read().unwrap().sessions,
            "{}: state",
            title
        );
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
                    "body": r#"{"message":"hello"}"#
                }
            ]
        }),

    )]
    #[tokio::test]
    #[case(
        "session dose not exist",
        "undefined_session",
        StatusCode::NOT_FOUND,
        json!({ "serverify_error": { "message": "session undefined_session is not found" } }),
    )]
    async fn get_session(
        #[case] title: &str,
        #[case] session: &str,
        #[case] expected_status_code: StatusCode,
        #[case] expected_res_body: Value,
    ) {
        let state = new_state_with(initial_sessions());
        let app = route_session_to(Router::new()).with_state(Arc::clone(&state));
        let server = TestServer::new(app).unwrap();

        let response = server.get(&format!("/session/{}", session)).await;

        assert_eq!(
            (expected_status_code, expected_res_body),
            (response.status_code(), response.json()),
            "{}: response",
            title
        );

        assert_eq!(
            initial_sessions(),
            state.read().unwrap().sessions,
            "{}: state",
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
        {
            let mut sessions = initial_sessions();
            sessions.shift_remove(&"exist_session".to_string());
            sessions
        },
    )]
    #[tokio::test]
    #[case(
        "session is not found",
        "undefined_session",
        StatusCode::NOT_FOUND,
        json!({ "serverify_error": { "message": "session undefined_session is not found" } }),
        initial_sessions()
    )]
    async fn delete_session(
        #[case] title: &str,
        #[case] session: &str,
        #[case] expected_status_code: StatusCode,
        #[case] expected_res_body: Value,
        #[case] expected_sessions: IndexMap<String, Vec<History>>,
    ) {
        let state = new_state_with(initial_sessions());
        let app = route_session_to(Router::new()).with_state(Arc::clone(&state));
        let server = TestServer::new(app).unwrap();

        let response = server.delete(&format!("/session/{}", session)).await;

        assert_eq!(
            (expected_status_code, expected_res_body),
            (response.status_code(), response.json()),
            "{}: response",
            title
        );

        assert_eq!(
            expected_sessions,
            state.read().unwrap().sessions,
            "{}: state",
            title
        );
    }
}
