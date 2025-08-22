use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Redirect};
use axum_extra::extract::Host;
use clap::Parser;
use std::net::SocketAddr;
use tower_http::normalize_path::NormalizePathLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
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

#[derive(Debug, Clone)]
pub struct HttpAppCfg {
    pub https_port: u16,
}

async fn run_server(args: CliArgs) -> EdgeResult<()> {
    let router = configure_server(args.clone()).await?;
    let server = router
        .layer(NormalizePathLayer::trim_trailing_slash())
        .into_make_service_with_connect_info::<SocketAddr>();

    if args.http.tls.tls_enable {
        let config = unleash_edge::tls::axum_rustls_config(args.http.tls.clone()).await?;
        let addr = args.http.https_server_socket();
        if args.http.tls.redirect_http_to_https {
            let http_redirect_app =
                Router::new()
                    .fallback(redirect_to_https)
                    .with_state(HttpAppCfg {
                        https_port: args.http.tls.tls_server_port,
                    });
            let http = axum_server::bind(args.http.http_server_socket())
                .serve(http_redirect_app.into_make_service());
            let https = axum_server::bind_rustls(addr, config).serve(server.clone());
            _ = try_join!(http, https);
        } else {
            _ = axum_server::bind_rustls(addr, config)
                .serve(server.clone())
                .await;
        }
    } else {
        let http_listener = tokio::net::TcpListener::bind(&args.http.http_server_addr())
            .await
            .map_err(|_| EdgeError::NotReady)?;
        _ = axum::serve(http_listener, server.clone()).await;
    };
    Ok(())
}

pub async fn redirect_to_https(
    State(cfg): State<HttpAppCfg>,
    Host(host): Host,
    uri: Uri,
) -> impl IntoResponse {
    let authority = rewrite_authority_port(&host, cfg.https_port)
        .unwrap_or_else(|| format!("{}:{}", host, cfg.https_port));

    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");

    let target = format!("https://{authority}{path_and_query}");
    Redirect::permanent(&target)
}

/// Replace/insert the port in `Host` (supports IPv4, names, and IPv6 literals).
fn rewrite_authority_port(host_header: &str, port: u16) -> Option<String> {
    let auth: Authority = host_header.parse().ok()?;
    let host = auth.host(); // host without brackets
    let is_ipv6 = host.contains(':'); // IPv6 literal contains ':'

    if is_ipv6 {
        Some(format!("[{host}]:{port}"))
    } else {
        Some(format!("{host}:{port}"))
    }
}
