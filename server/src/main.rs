use std::sync::Arc;

use crate::cli::EdgeMode;
use actix_cors::Cors;
use actix_middleware_etag::Etag;
use actix_web::{http, middleware, web, App, HttpServer};
use actix_web_opentelemetry::RequestTracing;
use clap::Parser;
use cli::CliArgs;
use types::EdgeProvider;

use unleash_edge::cli;
use unleash_edge::cli::EdgeArg;
use unleash_edge::cli::OfflineArgs;
use unleash_edge::client_api;
use unleash_edge::data_sources::memory_provider::MemoryProvider;
use unleash_edge::data_sources::offline_provider::OfflineProvider;
use unleash_edge::data_sources::redis_provider::RedisProvider;
use unleash_edge::edge_api;
use unleash_edge::frontend_api;
use unleash_edge::internal_backstage;
use unleash_edge::metrics;
use unleash_edge::types;
use unleash_edge::types::EdgeResult;

mod tls;

fn build_offline(offline_args: OfflineArgs) -> EdgeResult<Arc<dyn EdgeProvider>> {
    Ok(
        OfflineProvider::instantiate_provider(
            offline_args.bootstrap_file,
            offline_args.client_keys,
        )
        .map(Arc::new)?,
    )
}

fn build_memory() -> EdgeResult<Arc<dyn EdgeProvider>> {
    Ok(Arc::new(MemoryProvider::default()))
}

fn build_redis(redis_url: String) -> EdgeResult<Arc<dyn EdgeProvider>> {
    Ok(RedisProvider::new(&redis_url).map(Arc::new)?)
}

fn build_data_source(args: CliArgs) -> EdgeResult<Arc<dyn EdgeProvider>> {
    match args.mode {
        EdgeMode::Offline(offline_args) => build_offline(offline_args),
        EdgeMode::Edge(edge_args) => {
            let arg: EdgeArg = edge_args.into();
            match arg {
                EdgeArg::Redis(redis_url) => build_redis(redis_url),
                EdgeArg::InMemory => build_memory(),
            }
        }
    }
}

#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();
    let args = CliArgs::parse();
    let http_args = args.clone().http;
    let (metrics_handler, request_metrics) = metrics::instantiate(None);
    let client_provider: Arc<dyn EdgeProvider> =
        build_data_source(args).map_err(anyhow::Error::new)?;
    let server = HttpServer::new(move || {
        let client_provider_data = web::Data::from(client_provider.clone());

        let cors_middleware = Cors::default()
            .allow_any_origin()
            .send_wildcard()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![http::header::AUTHORIZATION, http::header::ACCEPT])
            .allowed_header(http::header::CONTENT_TYPE);
        App::new()
            .app_data(client_provider_data)
            .wrap(Etag::default())
            .wrap(cors_middleware)
            .wrap(RequestTracing::new())
            .wrap(request_metrics.clone())
            .wrap(middleware::Logger::default())
            .service(web::scope("/internal-backstage").configure(|service_cfg| {
                internal_backstage::configure_internal_backstage(
                    service_cfg,
                    metrics_handler.clone(),
                )
            }))
            .service(
                web::scope("/api")
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

    tokio::select! {
        _ = server.run() => {
        }
    }

    Ok(())
}
