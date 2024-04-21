use indexmap::IndexMap;
use serde::Serialize;

use crate::mock_endpoint::Method;

#[derive(Serialize, PartialEq, Debug)]
pub struct History {
    pub method: Method,
    pub headers: Vec<(String, String)>,
    pub path: String,
    pub queries: IndexMap<String, String>,
    pub body: String,
}
