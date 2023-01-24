use std::sync::Arc;

use crate::cli::EdgeMode;
use crate::offline_provider::OfflineProvider;
use actix_cors::Cors;
use actix_web::{http, middleware, web, App, HttpServer};
use actix_web_opentelemetry::RequestTracing;
use clap::Parser;
use cli::CliArgs;
use types::EdgeProvider;

mod cli;
mod client_api;
mod edge_api;
mod error;
mod frontend_api;
mod internal_backstage;
mod metrics;
mod offline_provider;
mod tls;
mod types;

#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();
    let args = CliArgs::parse();
    let http_args = args.clone().http;
    let (metrics_handler, request_metrics) = metrics::instantiate(None);
    let client_provider = match args.mode {
        EdgeMode::Offline(offline_args) => OfflineProvider::instantiate_provider(
            offline_args.bootstrap_file,
            offline_args.client_keys,
        ),
    }
    .map_err(anyhow::Error::new)?;
    let server = HttpServer::new(move || {
        let client_provider_arc: Arc<dyn EdgeProvider> = Arc::new(client_provider.clone());
        let client_provider_data = web::Data::from(client_provider_arc);

        let cors_middleware = Cors::default()
            .allowed_origin("*")
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![http::header::AUTHORIZATION, http::header::ACCEPT])
            .allowed_header(http::header::CONTENT_TYPE);
        App::new()
            .app_data(client_provider_data)
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
    server?
        .shutdown_timeout(5)
        .run()
        .await
        .map_err(anyhow::Error::new)
}
