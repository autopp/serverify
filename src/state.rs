use std::sync::{Arc, RwLock};

use indexmap::IndexMap;

use crate::history::History;

#[derive(Default)]
pub struct AppState {
    pub sessions: IndexMap<String, Vec<History>>,
}

pub type SharedState = Arc<RwLock<AppState>>;
