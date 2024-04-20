use std::fs;

use axum::{self, http::StatusCode, routing::get, Json, Router};
use clap::Parser;
use serverify::config;
use tokio::signal;

#[derive(Parser)]
struct Args {
    #[clap(long = "port", default_value = "8080")]
    port: u16,
    config_path: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let src = fs::read_to_string(args.config_path).unwrap();
    let endpoints = config::parse_config(&src).unwrap();

    let health = Router::new().route("/health", get(health));
    let mocks = endpoints
        .into_iter()
        .fold(Router::new(), |app, endpoint| endpoint.route_to(app));
    let app = health.nest("/mock/:serverify_session", mocks);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", args.port))
        .await
        .unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn health() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

async fn shutdown_signal() {
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
    }
}
