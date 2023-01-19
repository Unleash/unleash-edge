use actix_web::{middleware, web, App, HttpServer};
use actix_web_opentelemetry::RequestTracing;
use clap::Parser;
use cli::CliArgs;


mod cli;
mod error;
mod internal_backstage;
mod metrics;
mod tls;
mod types;

#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();
    let args = CliArgs::parse();
    let (metrics_handler, request_metrics) = metrics::instantiate(None);
    let server = HttpServer::new(move || {
        App::new()
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
