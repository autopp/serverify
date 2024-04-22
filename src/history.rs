use indexmap::IndexMap;
use serde::Serialize;

use crate::mock_endpoint::Method;

#[derive(Serialize, PartialEq, Debug, Clone)]
pub struct History {
    pub method: Method,
    pub headers: IndexMap<String, String>,
    pub path: String,
    pub query: IndexMap<String, String>,
    pub body: String,
}
