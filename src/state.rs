use std::sync::{Arc, RwLock};

use indexmap::IndexMap;

use crate::history::History;

#[derive(Default)]
pub struct AppStateOld {
    pub sessions: IndexMap<String, Vec<History>>,
}

pub type SharedState = Arc<RwLock<AppStateOld>>;

#[cfg(test)]
pub mod testutil {
    use super::*;

    pub fn new_state_with<K: ToString>(
        entries: impl IntoIterator<Item = (K, Vec<History>)>,
    ) -> SharedState {
        let state = SharedState::default();
        state
            .write()
            .unwrap()
            .sessions
            .extend(entries.into_iter().map(|(k, v)| (k.to_string(), v)));
        state
    }
}
