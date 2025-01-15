use indexmap::IndexMap;
use serde::Serialize;

use crate::method::Method;

#[derive(Serialize, Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct History {
    pub method: Method,
    pub headers: IndexMap<String, String>,
    pub path: String,
    pub query: IndexMap<String, String>,
    pub body: String,
}
