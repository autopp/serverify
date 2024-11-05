use std::fs;

use clap::Parser;
use serverify::{config, serve};
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
    let handle = serve(endpoints, ("0.0.0.0", args.port)).await.unwrap();

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

    handle.shutdown().await.unwrap();
}
