use crate::cli::EdgeMode;
use crate::offline_provider::OfflineProvider;
use actix_web::{middleware, web, App, HttpServer};
use actix_web_opentelemetry::RequestTracing;
use clap::Parser;
use cli::CliArgs;

mod cli;
mod client_api;
mod error;
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
        EdgeMode::Offline => OfflineProvider::instantiate_provider(args.clone().bootstrap_file),
    }
    .map_err(anyhow::Error::new)?;
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(client_provider.clone()))
            .wrap(RequestTracing::new())
            .wrap(request_metrics.clone())
            .wrap(middleware::Logger::default())
            .service(
                web::scope("/internal-backstage")
                    .configure(internal_backstage::configure_internal_backstage)
                    .service(
                        web::resource("/metrics").route(web::get().to(metrics_handler.clone())),
                    ),
            )
            .service(web::scope("/api").configure(client_api::configure_client_api))
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
