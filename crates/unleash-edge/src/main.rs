use axum::ServiceExt;
use clap::Parser;
use unleash_edge::configure_server;
use unleash_edge_cli::{CliArgs, EdgeMode};
use unleash_edge_types::EdgeResult;
use unleash_edge_types::errors::EdgeError;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
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
    if args.http.tls.tls_enable {
        let http_listener = tokio::net::TcpListener::bind(&args.http.http_server_addr()).await.map_err(|_| EdgeError::NotReady)?;
        axum::serve(
            http_listener,
            router.into_make_service_with_connect_info()
        )
    }
}
