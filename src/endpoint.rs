use axum::routing::{on, MethodFilter};
use indexmap::IndexMap;

pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

pub struct Endpoint {
    pub method: Method,
    pub path: String,
    pub status: u16,
    pub headers: IndexMap<String, String>,
    pub body: String,
}

impl Endpoint {
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
    use super::*;

    use axum_test::TestServer;

    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn test_route_to() {
        let app = axum::Router::new();
        let endpoint = Endpoint {
            method: Method::Get,
            path: "/".to_string(),
            status: 200,
            headers: IndexMap::new(),
            body: "Hello, world!".to_string(),
        };

        let app = endpoint.route_to(app);

        let server = TestServer::new(app).unwrap();
        let response = server.get("/").await;

        assert_eq!(response.status_code(), 200);
        assert_eq!(response.text(), "Hello, world!");
    }
}
