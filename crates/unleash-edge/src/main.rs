use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Redirect};
use axum_extra::extract::Host;
use axum_server::Handle;
use clap::Parser;
use futures::future::join_all;
use http::Uri;
use http::uri::Authority;
use hyper_util::rt::TokioTimer;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{SocketAddr, TcpListener};
use std::pin::pin;
use std::time::Duration;
use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};

use tokio::try_join;
use tower_http::normalize_path::NormalizePathLayer;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use unleash_edge::configure_server;
use unleash_edge_cli::{CliArgs, EdgeMode};
use unleash_edge_types::{BackgroundTask, EdgeResult};

async fn shutdown_signal(
    protocol: &str,
    address: String,
    path: String,
    shutdown_tasks: Vec<BackgroundTask>,
) {
    info!(
        "Edge is listening to {protocol} traffic on {} at {path}",
        address
    );

    let mut sigint = pin!(signal::ctrl_c());
    #[cfg(unix)]
    {
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
    }
    #[cfg(not(unix))]
    {
        tokio::select! {
            _ = &mut sigint => {
                info!("Received Ctrl+C (SIGINT), shutting down gracefully...");
            }
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

#[derive(Debug, Clone)]
pub struct HttpAppCfg {
    pub https_port: u16,
}

const H1_HEADER_TIMEOUT: Duration = Duration::from_secs(15); // protects against slowloris
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(20);
const KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(20);

fn make_listener(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;

    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    Ok(socket.into())
}

async fn run_server(args: CliArgs) -> EdgeResult<()> {
    if args.http.tls.tls_enable {
        let (router, shutdown_tasks) = configure_server(args.clone()).await?;
        let server = router
            .layer(NormalizePathLayer::trim_trailing_slash())
            .into_make_service_with_connect_info::<SocketAddr>();

        let https_handle = Handle::new();
        let https_handle_clone = https_handle.clone();
        let shutdown_fut = shutdown_signal(
            "TLS",
            args.http.https_server_addr().clone(),
            args.http.base_path.clone(),
            shutdown_tasks,
        );
        let http_handle = Handle::new();
        let http_handle_clone = http_handle.clone();

        tokio::spawn(async move {
            let _ = shutdown_fut.await;
            https_handle_clone.graceful_shutdown(Some(Duration::from_secs(10)));
            http_handle_clone.graceful_shutdown(Some(Duration::from_secs(10)));
        });
        let config = unleash_edge::tls::axum_rustls_config(args.http.tls.clone()).await?;
        if args.http.tls.redirect_http_to_https {
            let http_redirect_app =
                Router::new()
                    .fallback(redirect_to_https)
                    .with_state(HttpAppCfg {
                        https_port: args.http.tls.tls_server_port,
                    });
            let http_listener =
                make_listener(args.http.http_server_socket()).expect("Failed to bind HTTP socket");
            let http = axum_server::from_tcp(http_listener)
                .handle(http_handle)
                .serve(http_redirect_app.into_make_service());

            let https_listener =
                make_listener(args.http.https_server_socket()).expect("Failed to bind tls socket");
            let mut builder =
                axum_server::from_tcp_rustls(https_listener, config).handle(https_handle.clone());
            let https_builder = builder.http_builder();
            https_builder
                .http1()
                .timer(TokioTimer::new())
                .keep_alive(true)
                .header_read_timeout(H1_HEADER_TIMEOUT);
            https_builder
                .http2()
                .timer(TokioTimer::new())
                .keep_alive_interval(Some(KEEP_ALIVE_INTERVAL))
                .keep_alive_timeout(KEEP_ALIVE_TIMEOUT);
            let https = builder.serve(server.clone());
            _ = try_join!(http, https);
        } else {
            let https_listener =
                make_listener(args.http.https_server_socket()).expect("Failed to bind tls socket");
            let mut builder =
                axum_server::from_tcp_rustls(https_listener, config).handle(https_handle.clone());
            let https_builder = builder.http_builder();
            https_builder
                .http1()
                .timer(TokioTimer::new())
                .keep_alive(true)
                .header_read_timeout(H1_HEADER_TIMEOUT);
            https_builder
                .http2()
                .timer(TokioTimer::new())
                .keep_alive_interval(Some(KEEP_ALIVE_INTERVAL))
                .keep_alive_timeout(KEEP_ALIVE_TIMEOUT);
            _ = builder.serve(server.clone()).await;
        }
    } else {
        let (router, shutdown_tasks) = configure_server(args.clone()).await?;
        let server = router.into_make_service_with_connect_info::<SocketAddr>();
        let handle = Handle::new();
        let http_handle_clone = handle.clone();
        let shutdown_fut = shutdown_signal(
            "HTTP",
            args.http.http_server_addr().clone(),
            args.http.base_path.clone(),
            shutdown_tasks,
        );
        tokio::spawn(async move {
            let _ = shutdown_fut.await;
            http_handle_clone.graceful_shutdown(Some(Duration::from_secs(10)));
        });
        let http_listener =
            make_listener(args.http.http_server_socket()).expect("Failed to bind HTTP socket");
        let mut builder = axum_server::from_tcp(http_listener).handle(handle);
        let http_builder = builder.http_builder();
        http_builder
            .http1()
            .timer(TokioTimer::new())
            .keep_alive(true)
            .header_read_timeout(H1_HEADER_TIMEOUT);
        http_builder
            .http2()
            .timer(TokioTimer::new())
            .keep_alive_interval(Some(KEEP_ALIVE_INTERVAL))
            .keep_alive_timeout(KEEP_ALIVE_TIMEOUT);
        _ = builder.serve(server).await;
    }
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
