use std::io::ErrorKind;

use axum::{
    body::Body,
    extract::{FromRequestParts, Path, Query, Request, State},
    routing::{on, MethodFilter},
    Router,
};
use chrono::Local;
use futures::TryStreamExt;
use indexmap::IndexMap;
use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;

use crate::{
    method::Method,
    request_logger::{LoggerError, RequestLog},
    state::AppState,
};

#[derive(PartialEq, Debug)]
pub struct StatusCode(axum::http::StatusCode);

impl TryFrom<u16> for StatusCode {
    type Error = String;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        axum::http::StatusCode::from_u16(value)
            .map(StatusCode)
            .map_err(|err| err.to_string())
    }
}

#[derive(PartialEq, Debug)]
pub struct MockEndpoint {
    pub method: Method,
    pub path: String,
    pub status: StatusCode,
    pub headers: IndexMap<String, String>,
    pub body: String,
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

                        let Query(query) =
                            Query::<IndexMap<String, String>>::try_from_uri(&parts.uri)
                                .map_err(|err| (500, err.to_string()))?;

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
                            query,
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
                    self.headers
                        .into_iter()
                        .fold(axum::http::Response::builder(), |builder, (key, value)| {
                            builder.header(key, value)
                        })
                        .status(self.status.0)
                        .body(self.body)
                        .map_err(|e| (500, e.to_string()))
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
    use std::vec;

    use crate::request_logger::testutil::new_logger;

    use super::*;

    use axum::http::{HeaderMap, HeaderName, HeaderValue};
    use axum_test::TestServer;

    use indexmap::indexmap;
    use pretty_assertions::assert_eq;

    fn headers(kvs: Vec<(&'static str, &'static str)>) -> HeaderMap {
        HeaderMap::from_iter(
            kvs.into_iter()
                .map(|(k, v)| (HeaderName::from_static(k), HeaderValue::from_static(v))),
        )
    }

    #[tokio::test]
    async fn route_to() {
        let app = axum::Router::new();
        let endpoint = MockEndpoint {
            method: Method::Post,
            path: "/hello".to_string(),
            status: StatusCode::try_from(200).unwrap(),
            headers: indexmap! { "answer".to_string() => "42".to_string() },
            body: "Hello, world!".to_string(),
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
}
