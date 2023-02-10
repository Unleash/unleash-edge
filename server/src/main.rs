use std::sync::Arc;

use actix_cors::Cors;

use actix_middleware_etag::Etag;
use actix_web::middleware::Logger;
use actix_web::{web, App, HttpServer};
use actix_web_opentelemetry::RequestTracing;
use clap::Parser;
use cli::CliArgs;

use tokio::sync::RwLock;
use unleash_edge::client_api;
use unleash_edge::data_sources::builder::build_source_and_sink;
use unleash_edge::edge_api;
use unleash_edge::frontend_api;
use unleash_edge::http::background_refresh::refresh_features;
use unleash_edge::http::background_send_metrics::send_metrics_task;
use unleash_edge::internal_backstage;
use unleash_edge::metrics::client_metrics::MetricsCache;
use unleash_edge::prom_metrics;
use unleash_edge::{cli, middleware};

mod tls;

#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();
    let args = CliArgs::parse();
    let http_args = args.clone().http;
    let (metrics_handler, request_metrics) = prom_metrics::instantiate(None);
    let repo_info = build_source_and_sink(args).unwrap();
    let source = repo_info.source;
    let source_clone = source.clone();
    let sink_info = repo_info.sink_info;
    let validator = sink_info.as_ref().map(|sink| sink.token_validator.clone());

    let metrics_cache = Arc::new(RwLock::new(MetricsCache::default()));
    let metrics_cache_clone = metrics_cache.clone();

    let server = HttpServer::new(move || {
        let edge_source = web::Data::from(source.clone());
        let cors_middleware = Cors::default()
            .allow_any_origin()
            .send_wildcard()
            .allow_any_header()
            .allow_any_method();
        let mut app = App::new()
            .app_data(edge_source)
            .app_data(web::Data::from(metrics_cache.clone()));
        if validator.is_some() {
            app = app.app_data(web::Data::from(validator.clone().unwrap()))
        }
        app.wrap(Etag::default())
            .wrap(cors_middleware)
            .wrap(RequestTracing::new())
            .wrap(request_metrics.clone())
            .wrap(Logger::default())
            .service(web::scope("/internal-backstage").configure(|service_cfg| {
                internal_backstage::configure_internal_backstage(
                    service_cfg,
                    metrics_handler.clone(),
                )
            }))
            .service(
                web::scope("/api")
                    .wrap(middleware::as_async_middleware::as_async_middleware(
                        middleware::validate_token::validate_token,
                    ))
                    .configure(client_api::configure_client_api)
                    .configure(frontend_api::configure_frontend_api),
            )
            .service(web::scope("/edge").configure(edge_api::configure_edge_api))
    });
    let server = if http_args.tls.tls_enable {
        let config = tls::config(http_args.clone().tls)
            .expect("Was expecting to succeed in configuring TLS");
        server
            .bind_rustls(http_args.https_server_tuple(), config)?
            .bind(http_args.http_server_tuple())
    } else {
        server.bind(http_args.http_server_tuple())
    };
    let server = server?.shutdown_timeout(5);

    if let Some(sink_info) = sink_info {
        tokio::select! {
            _ = server.run() => {
                tracing::info!("Actix was shutdown properly");
            },
            _ = refresh_features(source_clone.clone(), sink_info.sink, sink_info.unleash_client.clone()) => {
                tracing::info!("Refresh task is shutting down");
            },
            _ = send_metrics_task(metrics_cache_clone, source_clone, sink_info.unleash_client, sink_info.metrics_interval_seconds) => {
                tracing::info!("Metrics task is shutting down");
            }
        }
    } else {
        server.run().await?;
    }

    Ok(())
}
