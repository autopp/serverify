use std::io::ErrorKind;

use axum::{
    body::Body,
    extract::{FromRequestParts, Path, Query, Request, State},
    routing::{on, MethodFilter},
    Router,
};
use chrono::Local;
use futures::TryStreamExt;
use indexmap::{indexmap, IndexMap};
use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;

use crate::{
    json_template::JsonTemplate,
    method::Method,
    request_logger::{LoggerError, RequestLog},
    state::AppState,
};

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct StatusCode(axum::http::StatusCode);

impl TryFrom<u16> for StatusCode {
    type Error = String;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        axum::http::StatusCode::from_u16(value)
            .map(StatusCode)
            .map_err(|err| err.to_string())
    }
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum ResponseHandler {
    Static {
        status: StatusCode,
        headers: IndexMap<String, String>,
        body: String,
    },
    Paging {
        status: StatusCode,
        headers: IndexMap<String, String>,
        page_param: String,
        per_page_param: String,
        default_per_page: usize,
        template: JsonTemplate,
        items: Vec<serde_json::Value>,
    },
}

impl ResponseHandler {
    pub fn respond(
        &self,
        query: &IndexMap<String, String>,
    ) -> Result<axum::http::Response<String>, (u16, String)> {
        match self {
            ResponseHandler::Static {
                status,
                headers,
                body,
            } => headers
                .into_iter()
                .fold(axum::http::Response::builder(), |builder, (key, value)| {
                    builder.header(key, value)
                })
                .status(status.0)
                .body(body.clone())
                .map_err(|e| (500, e.to_string())),
            ResponseHandler::Paging {
                status,
                headers,
                page_param,
                per_page_param,
                default_per_page,
                template,
                items,
            } => {
                let page = query
                    .get(page_param)
                    .and_then(|page| page.parse::<usize>().ok())
                    .unwrap_or(1);
                let per_page = query
                    .get(per_page_param)
                    .and_then(|page| page.parse::<usize>().ok())
                    .unwrap_or(*default_per_page);

                let contents = serde_json::Value::Array(
                    items
                        .iter()
                        .skip((page - 1) * per_page)
                        .take(per_page)
                        .map(Clone::clone)
                        .collect::<Vec<_>>(),
                );

                let body = template
                    .expand(indexmap! { "$_contents".to_string() => contents })
                    .to_string();

                headers
                    .into_iter()
                    .fold(
                        axum::http::Response::builder().header("content-type", "application/json"),
                        |builder, (key, value)| builder.header(key, value),
                    )
                    .status(status.0)
                    .body(body.clone())
                    .map_err(|e| (500, e.to_string()))
            }
        }
    }
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct MockEndpoint {
    pub method: Method,
    pub path: String,
    pub response: ResponseHandler,
}

#[derive(Deserialize)]
struct PathParams {
    serverify_session: String,
}

impl MockEndpoint {
    pub fn route_to(self, app: axum::Router<AppState>) -> axum::Router<AppState> {
        let method = match self.method {
            Method::Get => MethodFilter::GET,
            Method::Post => MethodFilter::POST,
            Method::Put => MethodFilter::PUT,
            Method::Delete => MethodFilter::DELETE,
            Method::Patch => MethodFilter::PATCH,
        };

        let route = on(
            method,
            move |State(state): State<AppState>, req: Request<Body>| async move {
                async {
                    // save history
                    let (mut parts, body) = req.into_parts();
                    let Path(PathParams { serverify_session }) =
                        Path::from_request_parts(&mut parts, &state)
                            .await
                            .map_err(|err| (500, err.to_string()))?;
                    let Query(query) = Query::<IndexMap<String, String>>::try_from_uri(&parts.uri)
                        .map_err(|err| (500, err.to_string()))?;

                    if serverify_session != "default" {
                        let method = match parts.method {
                            axum::http::Method::GET => Method::Get,
                            axum::http::Method::POST => Method::Post,
                            axum::http::Method::PUT => Method::Put,
                            axum::http::Method::DELETE => Method::Delete,
                            axum::http::Method::PATCH => Method::Patch,
                            _ => unreachable!(),
                        };
                        let headers = parts
                            .headers
                            .iter()
                            .map(|(name, value)| {
                                let value_str =
                                    value.to_str().map_err(|err| (500, err.to_string()))?;
                                Ok((name.to_string(), value_str.to_string()))
                            })
                            .collect::<Result<IndexMap<String, String>, (u16, String)>>()?;
                        let path = parts.uri.path().to_string();

                        let mut stream = StreamReader::new(
                            body.into_data_stream()
                                .map_err(|err| std::io::Error::new(ErrorKind::Other, err)),
                        );

                        let mut buf: Vec<u8> = vec![];
                        stream
                            .read_buf(&mut buf)
                            .await
                            .map_err(|err| (500, err.to_string()))?;

                        let log = RequestLog {
                            method,
                            headers,
                            path,
                            query: query.clone(),
                            body: String::from_utf8_lossy(&buf).to_string(),
                            requested_at: Local::now(),
                        };

                        state
                            .logger
                            .log_request(&serverify_session, &log)
                            .await
                            .map_err(|err| match err {
                                LoggerError::InternalError(msg) => (500, msg),
                                LoggerError::InvalidSession(msg) => (404, msg),
                            })?;
                    }

                    // respond
                    self.response.respond(&query)
                }
                .await
                .unwrap_or_else(|(status, message)| {
                    axum::http::Response::builder()
                        .status(status)
                        .body(message)
                        .unwrap()
                })
            },
        );

        app.nest(
            "/mock/:serverify_session",
            Router::new().route(&self.path, route),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, vec};

    use crate::request_logger::testutil::new_logger;

    use super::*;

    use axum::http::{HeaderMap, HeaderName, HeaderValue};
    use axum_test::TestServer;

    use indexmap::indexmap;
    use serde_json::json;

    fn headers(kvs: Vec<(&str, &str)>) -> HeaderMap {
        HeaderMap::from_iter(kvs.into_iter().map(|(k, v)| {
            (
                HeaderName::from_str(k).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            )
        }))
    }

    mod route_to {
        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        #[rstest]
        #[tokio::test]
        async fn route_to_static() {
            let app = axum::Router::new();
            let endpoint = MockEndpoint {
                method: Method::Post,
                path: "/hello".to_string(),
                response: ResponseHandler::Static {
                    status: StatusCode::try_from(200).unwrap(),
                    headers: indexmap! { "answer".to_string() => "42".to_string() },
                    body: "Hello, world!".to_string(),
                },
            };

            let logger = new_logger().await;
            logger.create_session("123").await.unwrap();
            let state = AppState { logger };

            let app = endpoint.route_to(app).with_state(state.clone());
            let server = TestServer::new(app).unwrap();
            let response = server
                .post("/mock/123/hello")
                .add_query_param("foo", "x")
                .add_query_param("bar", "y")
                .add_header(
                    HeaderName::from_static("token"),
                    HeaderValue::from_static("abc"),
                )
                .text("hello world")
                .await;

            assert_eq!(200, response.status_code());
            assert_eq!("Hello, world!", response.text());
            assert_eq!(
                &headers(vec![("content-length", "13"), ("answer", "42")]),
                response.headers()
            );

            let logs = state.logger.get_session_history("123").await.unwrap();
            assert_eq!(1, logs.len());

            let log = logs.first().unwrap();
            assert_eq!(Method::Post, log.method);
            assert_eq!(
                indexmap! { "content-type".to_string() => "text/plain".to_string(), "token".to_string() => "abc".to_string(), },
                log.headers
            );
            assert_eq!("/hello".to_string(), log.path);
            assert_eq!(
                indexmap! { "foo".to_string() => "x".to_string(), "bar".to_string() => "y".to_string() },
                log.query
            );
            assert_eq!("hello world".to_string(), log.body);
        }

        #[rstest]
        #[tokio::test]
        #[case(
            ResponseHandler::Paging {
                status: StatusCode::try_from(200).unwrap(),
                headers: indexmap! { "answer".to_string() => "42".to_string() },
                template: JsonTemplate::parse(
                    json!({
                        "total": 10,
                        "members": "$_contents",
                    }),
                    vec!["$_contents".to_string()],
                )
                .unwrap(),
                items: vec![
                    json!({"name": "member0"}),
                    json!({"name": "member1"}),
                    json!({"name": "member2"}),
                    json!({"name": "member3"}),
                    json!({"name": "member4"}),
                    json!({"name": "member5"}),
                    json!({"name": "member6"}),
                    json!({"name": "member7"}),
                    json!({"name": "member8"}),
                    json!({"name": "member9"}),
                ],
                page_param: "page".to_string(),
                per_page_param: "per_page".to_string(),
                default_per_page: 2,
            },
            vec![
                ("page", "2"),
                ("per_page", "3"),
            ],
            200,
            vec![
                ("answer", "42"),
            ],
            json!({
                "total": 10,
                "members": [
                    {"name": "member3"},
                    {"name": "member4"},
                    {"name": "member5"},
                ],
            }),
        )]
        #[tokio::test]
        #[case(
            ResponseHandler::Paging {
                status: StatusCode::try_from(200).unwrap(),
                headers: indexmap! { "answer".to_string() => "42".to_string() },
                template: JsonTemplate::parse(
                    json!({
                        "total": 10,
                        "members": "$_contents",
                    }),
                    vec!["$_contents".to_string()],
                )
                .unwrap(),
                items: vec![
                    json!({"name": "member0"}),
                    json!({"name": "member1"}),
                    json!({"name": "member2"}),
                    json!({"name": "member3"}),
                    json!({"name": "member4"}),
                    json!({"name": "member5"}),
                    json!({"name": "member6"}),
                    json!({"name": "member7"}),
                    json!({"name": "member8"}),
                    json!({"name": "member9"}),
                ],
                page_param: "page".to_string(),
                per_page_param: "per_page".to_string(),
                default_per_page: 2,
            },
            vec![
                ("page", "2"),
            ],
            200,
            vec![
                ("answer", "42"),
            ],
            json!({
                "total": 10,
                "members": [
                    {"name": "member2"},
                    {"name": "member3"},
                ],
            }),
        )]
        async fn paging_response(
            #[case] response_handler: ResponseHandler,
            #[case] query: Vec<(&'static str, &'static str)>,
            #[case] expected_status_code: u16,
            #[case] expected_headers: Vec<(&'static str, &'static str)>,
            #[case] expected_body: serde_json::Value,
        ) {
            let app = axum::Router::new();
            let endpoint = MockEndpoint {
                method: Method::Post,
                path: "/hello".to_string(),
                response: response_handler,
            };

            let logger = new_logger().await;
            logger.create_session("123").await.unwrap();
            let state = AppState { logger };

            let app = endpoint.route_to(app).with_state(state.clone());
            let server = TestServer::new(app).unwrap();

            let response = query
                .iter()
                .fold(server.post("/mock/123/hello"), |req, (k, v)| {
                    req.add_query_param(k, v)
                })
                .add_header(
                    HeaderName::from_static("token"),
                    HeaderValue::from_static("abc"),
                )
                .text("hello world")
                .await;

            assert_eq!(expected_status_code, response.status_code());
            response.assert_json(&expected_body);

            let content_length = expected_body.to_string().len().to_string();
            let mut header_pairs = vec![
                ("content-length", content_length.as_str()),
                ("content-type", "application/json"),
            ];

            header_pairs.extend(expected_headers.iter().map(|(k, v)| (*k, *v)));
            assert_eq!(&headers(header_pairs), response.headers());

            let logs = state.logger.get_session_history("123").await.unwrap();
            assert_eq!(1, logs.len(), "log size should be 1");

            let log = logs.first().unwrap();
            assert_eq!(Method::Post, log.method);
            assert_eq!(
                indexmap! { "content-type".to_string() => "text/plain".to_string(), "token".to_string() => "abc".to_string(), },
                log.headers
            );
            assert_eq!("/hello".to_string(), log.path);
            assert_eq!(
                query
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect::<IndexMap<String, String>>(),
                log.query
            );
            assert_eq!("hello world".to_string(), log.body);
        }
    }
}
