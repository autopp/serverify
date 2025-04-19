use indexmap::IndexMap;
use serde::Deserialize;

use crate::{
    method::Method,
    mock_endpoint::{MockEndpoint, ResponseHandler, StatusCode},
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
#[serde(tag = "type")]
enum ResponseConfig {
    #[serde(rename = "static")]
    Static {
        status: u16,
        headers: Option<IndexMap<String, String>>,
        body: String,
    },
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
                        let response_handler = match endpoint.response {
                            ResponseConfig::Static {
                                status,
                                headers,
                                body,
                            } => ResponseHandler::new_static(
                                StatusCode::try_from(status)?,
                                headers.unwrap_or_default(),
                                body,
                            ),
                        };
                        Ok(MockEndpoint {
                            method,
                            path: path.clone(),
                            response: response_handler,
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
                type: static
                status: 200
                headers:
                    Content-Type: text/plain
                body: "Hello, world!"
        post:
            response:
                type: static
                status: 204
                body: ""
    /goodbye:
        get:
            response:
                type: static
                status: 200
                headers:
                    Content-Type: text/plain
                body: "Goodbye, world!"
    "#, Ok(vec![
        MockEndpoint {
            method: Method::Get,
            path: "/hello".to_string(),
            response: ResponseHandler::new_static(
                StatusCode::try_from(200).unwrap(),
                indexmap! { "Content-Type".to_string() => "text/plain".to_string() },
                "Hello, world!".to_string(),
            ),
        },
        MockEndpoint {
            method: Method::Post,
            path: "/hello".to_string(),
            response: ResponseHandler::new_static(
                StatusCode::try_from(204).unwrap(),
                indexmap! {},
                "".to_string(),
            ),
        },
        MockEndpoint {
            method: Method::Get,
            path: "/goodbye".to_string(),
            response: ResponseHandler::new_static(
                StatusCode::try_from(200).unwrap(),
                indexmap! { "Content-Type".to_string() => "text/plain".to_string() },
                "Goodbye, world!".to_string(),
            ),
        },
    ]))]
    fn test_parse_config(#[case] src: &str, #[case] expected: Result<Vec<MockEndpoint>, String>) {
        assert_eq!(expected, parse_config(src));
    }
}
