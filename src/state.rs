use crate::request_logger::RequestLogger;

#[derive(Clone)]
pub struct AppState {
    pub logger: RequestLogger,
}
