use indexmap::IndexMap;
use serde::Deserialize;

use crate::{
    method::Method,
    mock_endpoint::{MockEndpoint, StatusCode},
};

#[derive(Deserialize)]
struct Config {
    pub paths: IndexMap<String, IndexMap<Method, EndpointConfig>>,
}

#[derive(Deserialize)]
struct EndpointConfig {
    pub response: ResponseConfig,
}

#[derive(Deserialize)]
struct ResponseConfig {
    pub status: u16,
    pub headers: Option<IndexMap<String, String>>,
    pub body: String,
}

pub fn parse_config(src: &str) -> Result<Vec<MockEndpoint>, String> {
    serde_yaml::from_str::<Config>(src)
        .map_err(|e| e.to_string())
        .and_then(|config| {
            config
                .paths
                .into_iter()
                .flat_map(|(path, methods)| {
                    methods.into_iter().map(move |(method, endpoint)| {
                        let status = StatusCode::try_from(endpoint.response.status)?;
                        Ok(MockEndpoint {
                            method,
                            path: path.clone(),
                            status,
                            headers: endpoint.response.headers.unwrap_or_default(),
                            body: endpoint.response.body,
                        })
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::indexmap;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case(r#"
paths:
    /hello:
        get:
            response:
                status: 200
                headers:
                    Content-Type: text/plain
                body: "Hello, world!"
        post:
            response:
                status: 204
                body: ""
    /goodbye:
        get:
            response:
                status: 200
                headers:
                    Content-Type: text/plain
                body: "Goodbye, world!"
    "#, Ok(vec![
        MockEndpoint {
            method: Method::Get,
            path: "/hello".to_string(),
            status: StatusCode::try_from(200).unwrap(),
            headers: indexmap! { "Content-Type".to_string() => "text/plain".to_string() },
            body: "Hello, world!".to_string(),
        },
        MockEndpoint {
            method: Method::Post,
            path: "/hello".to_string(),
            status: StatusCode::try_from(204).unwrap(),
            headers: indexmap! {},
            body: "".to_string(),
        },
        MockEndpoint {
            method: Method::Get,
            path: "/goodbye".to_string(),
            status: StatusCode::try_from(200).unwrap(),
            headers: indexmap! { "Content-Type".to_string() => "text/plain".to_string() },
            body: "Goodbye, world!".to_string(),
        },
    ]))]
    fn test_parse_config(#[case] src: &str, #[case] expected: Result<Vec<MockEndpoint>, String>) {
        assert_eq!(expected, parse_config(src));
    }
}
