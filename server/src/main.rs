use std::sync::Arc;

use actix_cors::Cors;

use actix_middleware_etag::Etag;
use actix_web::middleware::Logger;
use actix_web::{web, App, HttpServer};
use actix_web_opentelemetry::RequestTracing;
use clap::Parser;
use cli::CliArgs;
use unleash_edge::data_sources::builder::build_caches_and_refreshers;
use unleash_types::client_metrics::ConnectVia;

use unleash_edge::client_api;
use unleash_edge::edge_api;
use unleash_edge::frontend_api;
// use unleash_edge::http::background_refresh::refresh_features;
// use unleash_edge::http::background_send_metrics::send_metrics_task;
use unleash_edge::internal_backstage;
use unleash_edge::metrics::client_metrics::MetricsCache;
use unleash_edge::openapi;
// use unleash_edge::persistence;
use unleash_edge::prom_metrics;
use unleash_edge::{cli, middleware};
use utoipa_swagger_ui::SwaggerUi;
mod tls;
use utoipa::OpenApi;

#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv::dotenv().ok();
    let args = CliArgs::parse();
    let mode_arg = args.clone().mode;
    let http_args = args.clone().http;
    let (metrics_handler, request_metrics) = prom_metrics::instantiate(None);
    let connect_via = ConnectVia {
        app_name: args.clone().app_name,
        instance_id: args.clone().instance_id,
    };
    let ((token_cache, features_cache, engine_cache), maybe_validator) =
        build_caches_and_refreshers(args).await.unwrap();
    let rc_tc = Arc::new(token_cache);
    let rc_fc = Arc::new(features_cache);
    let rc_ec = Arc::new(engine_cache);
    let metrics_cache = Arc::new(MetricsCache::default());
    let metrics_cache_clone = metrics_cache.clone();

    let openapi = openapi::ApiDoc::openapi();

    let server = HttpServer::new(move || {
        let cors_middleware = Cors::default()
            .allow_any_origin()
            .send_wildcard()
            .allow_any_header()
            .allow_any_method();
        let mut app = App::new()
            .app_data(web::Data::new(mode_arg.clone()))
            .app_data(web::Data::new(connect_via.clone()))
            .app_data(web::Data::new(metrics_cache.clone()))
            .app_data(web::Data::from(rc_tc.clone()))
            .app_data(web::Data::from(rc_fc.clone()))
            .app_data(web::Data::from(rc_ec.clone()));
        // if maybe_validator.is_some() {
        //     app = app.app_data(web::Data::from(maybe_validator.unwrap().clone()))
        // }
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
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}").url("/api-doc/openapi.json", openapi.clone()),
            )
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
    let server = server?.workers(http_args.workers).shutdown_timeout(5);

    tokio::select! {
        _ = server.run() => {
            tracing::info!("Actix is shutting down. Persisting data");
            // persist_state()
            tracing::info!("Actix was shutdown properly");
        }

    };
    Ok(())
}
