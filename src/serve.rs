use std::net::SocketAddr;

use axum::{http::StatusCode, routing::get, Json, Router};
use tokio::{net::ToSocketAddrs, sync::oneshot::Sender, task::JoinHandle};

use crate::{
    mock_endpoint::MockEndpoint, request_logger::RequestLogger, session_endpoint::route_session_to,
    state::AppState,
};

pub struct ServerHandle {
    handle: JoinHandle<()>,
    close_tx: Sender<()>,
    addr: SocketAddr,
}

impl ServerHandle {
    pub async fn shutdown(self) -> Result<(), String> {
        self.close_tx.send(()).map_err(|_| "".to_string())?;
        self.handle.await.map_err(|e| e.to_string())
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

pub async fn serve<A: ToSocketAddrs>(
    endpoints: Vec<MockEndpoint>,
    addr: A,
) -> Result<ServerHandle, String> {
    let health = Router::new().route("/health", get(health));
    let mocks = endpoints
        .into_iter()
        .fold(health, |app, endpoint| endpoint.route_to(app));

    let pool = sqlx::sqlite::SqlitePool::connect("sqlite::memory:")
        .await
        .unwrap();
    let logger = RequestLogger::new(pool).unwrap();
    logger.init().await.unwrap();

    let app = route_session_to(mocks).with_state(AppState { logger });

    let (close_tx, close_rx) = tokio::sync::oneshot::channel();

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                println!("wait");
                _ = close_rx.await;
                println!("shut down")
            })
            .await
            .unwrap();
    });

    Ok(ServerHandle {
        handle,
        close_tx,
        addr,
    })
}

async fn health() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn serve_and_shutdown() {
        let server = serve(vec![], "0.0.0.0:0").await.unwrap();
        let addr = server.addr();

        let res = reqwest::get(format!("http://{}/health", addr))
            .await
            .unwrap();
        server.shutdown().await.unwrap();

        assert_eq!(200, res.status().as_u16());
    }
}
