use actix_cors::Cors;

use actix_middleware_etag::Etag;
use actix_web::middleware::Logger;
use actix_web::{http, web, App, HttpServer};
use actix_web_opentelemetry::RequestTracing;
use clap::Parser;
use cli::CliArgs;
use tokio::sync::mpsc;
use unleash_edge::client_api;
use unleash_edge::data_sources::builder::build_source_and_sink;
use unleash_edge::edge_api;
use unleash_edge::frontend_api;
use unleash_edge::http::token_refresh::poll_for_token_status;
use unleash_edge::internal_backstage;
use unleash_edge::metrics;
use unleash_edge::types::EdgeToken;
use unleash_edge::{cli, middleware};

mod tls;

#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();
    let args = CliArgs::parse();
    let http_args = args.clone().http;
    let (metrics_handler, request_metrics) = metrics::instantiate(None);
    let (source, sink) = build_source_and_sink(args).map_err(anyhow::Error::new)?;
    let refresh_sink = sink.clone();

    let (sender, receiver) = mpsc::channel::<EdgeToken>(32);
    let server = HttpServer::new(move || {
        let edge_source = web::Data::from(source.clone());
        let edge_sink = web::Data::from(sink.clone());
        let cors_middleware = Cors::default()
            .allow_any_origin()
            .send_wildcard()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![http::header::AUTHORIZATION, http::header::ACCEPT])
            .allowed_header(http::header::CONTENT_TYPE);
        App::new()
            .app_data(edge_source)
            .app_data(edge_sink)
            .app_data(web::Data::new(sender.clone()))
            .wrap(Etag::default())
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

    tokio::select! {
        _ = server.run() => {
            tracing::info!("Actix was shutdown properly");
        },
        _ = poll_for_token_status(receiver, refresh_sink) => {
            tracing::info!("Token validator task is shutting down")
        }
    }

    Ok(())
}
