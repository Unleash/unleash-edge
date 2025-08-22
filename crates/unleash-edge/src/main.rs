use clap::Parser;
use futures::future::join_all;
use std::net::SocketAddr;
use std::pin::pin;
use tokio::signal;
use tokio::signal::unix::{SignalKind, signal};
use tower_http::normalize_path::NormalizePathLayer;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use unleash_edge::configure_server;
use unleash_edge_cli::{CliArgs, EdgeMode};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::{BackgroundTask, EdgeResult};

async fn shutdown_signal(address: String, shutdown_tasks: Vec<BackgroundTask>) {
    info!("Edge is listening to http traffic on {}", address);

    let mut sigint = pin!(signal::ctrl_c());
    let mut sigterm_stream = signal(SignalKind::terminate()).expect("Failed to bind SIGTERM");
    let mut sigterm = pin!(sigterm_stream.recv());

    tokio::select! {
        _ = &mut sigint => {
            info!("Received Ctrl+C (SIGINT), shutting down gracefully...");
        }
        _ = &mut sigterm => {
            info!("Received SIGTERM, shutting down gracefully...");
        }
    }
    join_all(shutdown_tasks).await;
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = unleash_edge_cli::CliArgs::parse();
    if args.markdown_help {
        clap_markdown::print_help_markdown::<CliArgs>();
        return Ok(());
    }

    tracing_subscriber::registry()
        .with(unleash_edge::tracing::formatting_layer(&args))
        .with(unleash_edge::tracing::log_filter())
        .init();

    match args.mode {
        EdgeMode::Health(health_args) => {
            unleash_edge::health_checker::check_health(health_args).await
        }
        EdgeMode::Ready(ready_args) => unleash_edge::ready_checker::check_ready(ready_args).await,
        _ => run_server(args).await,
    }
    .map_err(|e| e.into())
}

async fn run_server(args: CliArgs) -> EdgeResult<()> {
    let (router, shutdown_tasks) = configure_server(args.clone()).await?;
    let server = router
        .layer(NormalizePathLayer::trim_trailing_slash())
        .into_make_service_with_connect_info::<SocketAddr>();

    if args.http.tls.tls_enable {
        let config = unleash_edge::tls::axum_rustls_config(args.http.tls.clone()).await?;
        let addr = args.http.https_server_socket();
        axum_server::bind_rustls(addr, config)
            .serve(server.clone())
            .await
            .unwrap();
    }
    let http_listener = tokio::net::TcpListener::bind(&args.http.http_server_addr())
        .await
        .map_err(|_| EdgeError::NotReady)?;
    let _ = axum::serve(http_listener, server.clone())
        .with_graceful_shutdown(shutdown_signal(
            args.http.http_server_addr().clone(),
            shutdown_tasks,
        ))
        .await;
    Ok(())
}
