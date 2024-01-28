use axum::{
    self,
    routing::{on, MethodFilter},
    Router,
};
use tokio::signal;

#[tokio::main]
async fn main() {
    println!("Hello, world!");
    let app = Router::new().route("/", on(MethodFilter::GET, || async { "Hello, World!" }));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4000").await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
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
