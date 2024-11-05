use std::fs;

use axum::{self, http::StatusCode, routing::get, Json, Router};
use clap::Parser;
use serverify::{
    config, request_logger::RequestLogger, session_endpoint::route_session_to, state::AppState,
};
use tokio::signal;

#[derive(Parser)]
struct Args {
    #[clap(long = "port", default_value = "8080")]
    port: u16,
    config_path: String,
}

trait ResultExt<T, E> {
    fn exit_on_err(self, code: i32) -> T;
}

impl<T, E: ToString> ResultExt<T, E> for Result<T, E> {
    fn exit_on_err(self, code: i32) -> T {
        match self {
            Ok(value) => value,
            Err(err) => {
                eprintln!("Error: {}", err.to_string());
                std::process::exit(code);
            }
        }
    }
}

const EXIT_STATUS_INVALID_INPUT: i32 = 2;

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let endpoints = fs::read_to_string(&args.config_path)
        .map_err(|err| err.to_string())
        .and_then(|src| config::parse_config(&src))
        .map_err(|err| format!("cannot read config from {}: {}", args.config_path, err))
        .exit_on_err(EXIT_STATUS_INVALID_INPUT);

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

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", args.port))
        .await
        .unwrap();

    let (close_tx, close_rx) = tokio::sync::oneshot::channel();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                println!("wait");
                _ = close_rx.await;
                println!("shut down")
            })
            .await
            .unwrap();
    });

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install CTRL+C signal handler");
    };

    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM signal handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    };

    _ = close_tx.send(());

    _ = server_handle.await;
}

async fn health() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}
