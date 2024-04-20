use std::{
    io::ErrorKind,
    sync::{Arc, RwLock},
};

use axum::{
    body::Body,
    extract::{FromRequestParts, Path, Query, Request, State},
    routing::{on, MethodFilter},
    Router,
};
use futures::TryStreamExt;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;

use crate::history::History;

#[derive(Default)]
pub struct AppState {
    pub sessions: IndexMap<String, Vec<History>>,
}

pub type SharedState = Arc<RwLock<AppState>>;

#[derive(Serialize, Deserialize, Hash, PartialEq, Eq, Debug)]
pub enum Method {
    #[serde(rename = "get")]
    Get,
    #[serde(rename = "post")]
    Post,
    #[serde(rename = "put")]
    Put,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "patch")]
    Patch,
}

#[derive(PartialEq, Debug)]
pub struct Endpoint {
    pub method: Method,
    pub path: String,
    pub status: u16,
    pub headers: IndexMap<String, String>,
    pub body: String,
}

pub struct PathPrefix<'a>(&'a str);

impl<'a> PathPrefix<'a> {
    pub fn new(prefix: &'a str) -> Result<Self, String> {
        if prefix.ends_with("/:serverify_session") || prefix.contains("/:serverify_session/") {
            Ok(Self(prefix))
        } else {
            Err("Prefix must start with a slash".to_string())
        }
    }
}

#[derive(Deserialize)]
struct PathParams {
    serverify_session: String,
}

impl Endpoint {
    pub fn route_to(
        self,
        app: axum::Router<SharedState>,
        PathPrefix(prefix): &PathPrefix,
    ) -> axum::Router<SharedState> {
        let method = match self.method {
            Method::Get => MethodFilter::GET,
            Method::Post => MethodFilter::POST,
            Method::Put => MethodFilter::PUT,
            Method::Delete => MethodFilter::DELETE,
            Method::Patch => MethodFilter::PATCH,
        };

        let route = on(
            method,
            move |State(state): State<SharedState>, req: Request<Body>| async move {
                // save history
                let (mut parts, body) = req.into_parts();
                let Path(PathParams { serverify_session }) =
                    Path::from_request_parts(&mut parts, &state).await.unwrap(); // TODO: handle error
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
                            (name.to_string(), value.to_str().unwrap().to_string())
                        })
                        .collect();
                    let path = parts.uri.path().to_string();

                    let Query(queries) =
                        Query::<IndexMap<String, String>>::try_from_uri(&parts.uri).unwrap(); // TODO: handle error

                    let mut stream = StreamReader::new(
                        body.into_data_stream()
                            .map_err(|err| std::io::Error::new(ErrorKind::Other, err)),
                    );

                    let mut buf: Vec<u8> = vec![];
                    stream.read_buf(&mut buf).await.unwrap(); // TODO handle error
                    let history = History {
                        method,
                        headers,
                        path,
                        queries,
                        body: String::from_utf8_lossy(&buf).to_string(),
                    };

                    let sessions = &mut state.write().unwrap().sessions;
                    let session = sessions.get_mut(&"123".to_string()).unwrap();
                    session.push(history);
                }

                // respond
                self.headers
                    .into_iter()
                    .fold(axum::http::Response::builder(), |builder, (key, value)| {
                        builder.header(key, value)
                    })
                    .status(axum::http::StatusCode::from_u16(self.status).unwrap())
                    .body(self.body)
                    .unwrap()
            },
        );

        app.nest(prefix, Router::new().route(&self.path, route))
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

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
        let endpoint = Endpoint {
            method: Method::Post,
            path: "/hello".to_string(),
            status: 200,
            headers: indexmap! { "answer".to_string() => "42".to_string() },
            body: "Hello, world!".to_string(),
        };

        let shared_state = SharedState::default();
        {
            let sessions = &mut shared_state.write().unwrap().sessions;
            sessions.insert("123".to_string(), vec![]);
        };
        let path_prefix = PathPrefix::new("/mock/:serverify_session").unwrap();

        let app = endpoint
            .route_to(app, &path_prefix)
            .with_state(Arc::clone(&shared_state));
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

        let sessions = &shared_state.read().unwrap().sessions;
        assert_eq!(
            &indexmap! {
                "123".to_string() => vec![History {
                    method: Method::Post,
                    headers: vec![
                        ("content-type".to_string(), "text/plain".to_string()),
                        ("token".to_string(), "abc".to_string()),
                    ],
                    path: "/hello".to_string(),
                    queries: indexmap! {
                        "foo".to_string() => "x".to_string(),
                        "bar".to_string() => "y".to_string()
                    },
                    body: "hello world".to_string(),
                }]
            },
            sessions
        );
    }
}
