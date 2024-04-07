use axum::routing::{on, MethodFilter};

pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

pub struct Endpoint<H: IntoIterator<Item = (String, String)> + Clone + Send + Sized + 'static> {
    pub method: Method,
    pub path: String,
    pub status: u16,
    pub headers: H,
    pub body: String,
}

impl<H: IntoIterator<Item = (String, String)> + Clone + Send + Sized + 'static> Endpoint<H> {
    pub fn route_to(self, app: axum::Router) -> axum::Router {
        let method = match self.method {
            Method::Get => MethodFilter::GET,
            Method::Post => MethodFilter::POST,
            Method::Put => MethodFilter::PUT,
            Method::Delete => MethodFilter::DELETE,
            Method::Patch => MethodFilter::PATCH,
        };

        let route = on(method, move || async move {
            self.headers
                .into_iter()
                .fold(axum::http::Response::builder(), |builder, (key, value)| {
                    builder.header(key, value)
                })
                .status(axum::http::StatusCode::from_u16(self.status).unwrap())
                .body(self.body)
                .unwrap()
        });

        app.route(&self.path, route)
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use super::*;

    use axum::http::{HeaderMap, HeaderName, HeaderValue};
    use axum_test::TestServer;

    use pretty_assertions::assert_eq;

    fn headers(kvs: Vec<(&'static str, &'static str)>) -> HeaderMap {
        HeaderMap::from_iter(
            kvs.into_iter()
                .map(|(k, v)| (HeaderName::from_static(k), HeaderValue::from_static(v))),
        )
    }

    #[tokio::test]
    async fn test_route_to() {
        let app = axum::Router::new();
        let endpoint = Endpoint {
            method: Method::Get,
            path: "/".to_string(),
            status: 200,
            headers: vec![("answer".to_string(), "42".to_string())],
            body: "Hello, world!".to_string(),
        };

        let app = endpoint.route_to(app);
        let server = TestServer::new(app).unwrap();
        let response = server.get("/").await;

        assert_eq!(200, response.status_code());
        assert_eq!("Hello, world!", response.text());
        assert_eq!(
            &headers(vec![("content-length", "13"), ("answer", "42")]),
            response.headers()
        );
    }
}
