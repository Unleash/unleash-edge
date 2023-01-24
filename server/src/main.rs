use std::sync::Arc;

use crate::cli::EdgeMode;
use crate::offline_provider::OfflineProvider;
use actix_web::{middleware, web, App, HttpServer};
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
    let (metrics_handler, request_metrics) = metrics::instantiate(None);
    let client_provider = match args.mode {
        EdgeMode::Offline => OfflineProvider::instantiate_provider(
            args.clone().bootstrap_file,
            args.clone().client_keys,
        ),
    }
    .map_err(anyhow::Error::new)?;
    let server = HttpServer::new(move || {
        let client_provider_arc: Arc<dyn EdgeProvider> = Arc::new(client_provider.clone());
        let client_provider_data = web::Data::from(client_provider_arc);
        App::new()
            .app_data(client_provider_data)
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
    let server = if args.http.tls.tls_enable {
        let config = tls::config(args.clone().http.tls)
            .expect("Was expecting to succeed in configuring TLS");
        server
            .bind_rustls(args.https_server_tuple(), config)?
            .bind(args.http_server_tuple())
    } else {
        server.bind(args.http_server_tuple())
    };
    server?
        .shutdown_timeout(5)
        .run()
        .await
        .map_err(anyhow::Error::new)
}
