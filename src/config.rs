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
    #[serde(rename = "paging")]
    Paging {
        status: u16,
        headers: Option<IndexMap<String, String>>,
        page_param: String,
        per_page_param: String,
        default_per_page: usize,
        page_origin: Option<usize>,
        template: serde_json::Value,
        items: Vec<serde_json::Value>,
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
                        match endpoint.response {
                            ResponseConfig::Static {
                                status,
                                headers,
                                body,
                            } => Ok(ResponseHandler::new_static(
                                StatusCode::try_from(status)?,
                                headers.unwrap_or_default(),
                                body,
                            )),
                            ResponseConfig::Paging {
                                status,
                                headers,
                                page_param,
                                per_page_param,
                                default_per_page,
                                page_origin,
                                template,
                                items,
                            } => ResponseHandler::new_paging(
                                status.try_into()?,
                                headers.unwrap_or_default(),
                                page_param,
                                per_page_param,
                                default_per_page,
                                page_origin,
                                template,
                                items,
                            ),
                        }
                        .map(|response_handler| MockEndpoint {
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
    use serde_json::json;

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
    /friend:
        get:
            response:
                type: paging
                status: 200
                headers:
                    Content-Type: application/json
                page_param: p
                per_page_param: items
                default_per_page: 10
                page_origin: 0
                template:
                    friends: $_contents
                items:
                    - name: Alice
                      age: 10
                    - name: Bob
                      age: 20
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
        MockEndpoint {
            method: Method::Get,
            path: "/friend".to_string(),
            response: ResponseHandler::new_paging(
                StatusCode::try_from(200).unwrap(),
                indexmap!{ "Content-Type".to_string() => "application/json".to_string() },
                "p".to_string(),
                "items".to_string(),
                10,
                Some(0),
                json!({
                    "friends": "$_contents",
                }),
                vec![
                    json!({
                        "name": "Alice",
                        "age": 10,
                    }),
                    json!({
                        "name": "Bob",
                        "age": 20,
                    }),
                ]
            ).unwrap()
        }
    ]))]
    fn test_parse_config(#[case] src: &str, #[case] expected: Result<Vec<MockEndpoint>, String>) {
        assert_eq!(expected, parse_config(src));
    }
}
