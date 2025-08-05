use std::net::SocketAddr;
use axum::ServiceExt;
use clap::Parser;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use unleash_edge::configure_server;
use unleash_edge_cli::{CliArgs, EdgeMode};
use unleash_edge_types::EdgeResult;
use unleash_edge_types::errors::EdgeError;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
    let args = unleash_edge_cli::CliArgs::parse();
    if args.markdown_help {
        clap_markdown::print_help_markdown::<CliArgs>();
        return Ok(());
    }

    match args.mode {
        EdgeMode::Health(health_args) => unleash_edge::health_checker::check_health(health_args).await,
        EdgeMode::Ready(ready_args) => unleash_edge::ready_checker::check_ready(ready_args).await,
        _ => run_server(args).await,
    }
    .map_err(|e| e.into())
}

async fn run_server(args: CliArgs) -> EdgeResult<()> {

    let router = configure_server(args.clone()).await?;
    let server = router.into_make_service_with_connect_info::<SocketAddr>();
    if args.http.tls.tls_enable {
        let config = unleash_edge::tls::axum_rustls_config(args.http.tls.clone()).await?;
        let addr = args.http.https_server_socket();
        let https_server = axum_server::bind_rustls(addr, config)
            .serve(server.clone())
            .await
            .unwrap();
    }
    let http_listener = tokio::net::TcpListener::bind(&args.http.http_server_addr()).await.map_err(|_| EdgeError::NotReady)?;
    let _ = axum::serve(http_listener, server.clone()).await;
    info!("Edge is listening to http traffic on {}", &args.http.http_server_addr());
    Ok(())
}
