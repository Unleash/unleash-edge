use actix_middleware_etag::Etag;
use actix_web::middleware::Logger;
use actix_web::{web, App, HttpServer};
use clap::Parser;
use dashmap::DashMap;
use futures::future::join_all;
use std::sync::Arc;
use tokio::sync::RwLock;
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::ConnectVia;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use tracing::info;
use unleash_edge::builder::build_caches_and_refreshers;
use unleash_edge::cli::{CliArgs, EdgeMode};
use unleash_edge::feature_cache::FeatureCache;
use unleash_edge::http::background_send_metrics::send_metrics_one_shot;
use unleash_edge::http::broadcaster::Broadcaster;
use unleash_edge::http::refresher::feature_refresher::FeatureRefresher;
use unleash_edge::metrics::client_metrics::MetricsCache;
use unleash_edge::metrics::edge_metrics::EdgeInstanceData;
use unleash_edge::offline::offline_hotload;
use unleash_edge::persistence::{persist_data, EdgePersistence};
use unleash_edge::types::{EdgeToken, TokenValidationStatus};
use unleash_edge::{client_api, frontend_api, health_checker, openapi, ready_checker};
use unleash_edge::{edge_api, prom_metrics};
use unleash_edge::{internal_backstage, tls};
use unleash_edge::http::instance_data::InstanceDataSender;

#[cfg(not(tarpaulin_include))]
#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
    use unleash_edge::{http::unleash_client::ClientMetaInformation, metrics::metrics_pusher};

    let args = CliArgs::parse();
    let disable_all_endpoint = args.disable_all_endpoint;
    if args.markdown_help {
        clap_markdown::print_help_markdown::<CliArgs>();
        return Ok(());
    }
    if let EdgeMode::Health(args) = args.mode {
        return health_checker::check_health(args)
            .await
            .map_err(|e| e.into());
    };
    if let EdgeMode::Ready(args) = args.mode {
        return ready_checker::check_ready(args).await.map_err(|e| e.into());
    }
    let schedule_args = args.clone();
    let mode_arg = args.clone().mode;
    let http_args = args.clone().http;
    let cors_arg = http_args.cors.clone();
    let token_header = args.clone().token_header;
    let request_timeout = args.edge_request_timeout;
    let keepalive_timeout = args.edge_keepalive_timeout;
    let trust_proxy = args.clone().trust_proxy;
    let base_path = http_args.base_path.clone();
    let (metrics_handler, request_metrics) = prom_metrics::instantiate(None, &args.log_format);
    let connect_via = ConnectVia {
        app_name: args.clone().app_name,
        instance_id: args.clone().instance_id,
    };
    let app_name = args.app_name.clone();
    let our_instance_data_for_app_context = Arc::new(EdgeInstanceData::new(&app_name));
    let our_instance_data = our_instance_data_for_app_context.clone();
    let instance_id = args.instance_id.clone();
    let custom_headers = match args.mode {
        EdgeMode::Edge(ref edge) => edge.custom_client_headers.clone(),
        _ => vec![],
    };

    let internal_backstage_args = args.internal_backstage.clone();

    let (
        (token_cache, features_cache, delta_cache, engine_cache),
        token_validator,
        feature_refresher,
        persistence,
    ) = build_caches_and_refreshers(args.clone()).await.unwrap();

    let instance_data_sender = InstanceDataSender::from_args(args.clone(), our_instance_data.clone());
    let token_validator_schedule = token_validator.clone();
    let lazy_feature_cache = features_cache.clone();
    let lazy_token_cache = token_cache.clone();
    let lazy_engine_cache = engine_cache.clone();
    let lazy_feature_refresher = feature_refresher.clone();

    let metrics_cache = Arc::new(MetricsCache::default());
    let metrics_cache_clone = metrics_cache.clone();

    let openapi = openapi::ApiDoc::openapi();
    let refresher_for_app_data = feature_refresher.clone();
    let prom_registry_for_write = metrics_handler.registry.clone();
    let instances_observed_for_app_context: Arc<RwLock<Vec<EdgeInstanceData>>> =
        Arc::new(RwLock::new(Vec::new()));
    let downstream_instance_data = instances_observed_for_app_context.clone();

    let broadcaster = Broadcaster::new(features_cache.clone());

    let server = HttpServer::new(move || {
        let qs_config =
            serde_qs::actix::QsQueryConfig::default().qs_config(serde_qs::Config::new(5, false));

        let cors_middleware = cors_arg.middleware();
        let mut app = App::new()
            .app_data(qs_config)
            .app_data(web::Data::new(token_header.clone()))
            .app_data(web::Data::new(trust_proxy.clone()))
            .app_data(web::Data::new(mode_arg.clone()))
            .app_data(web::Data::new(connect_via.clone()))
            .app_data(web::Data::from(metrics_cache.clone()))
            .app_data(web::Data::from(token_cache.clone()))
            .app_data(web::Data::from(delta_cache.clone()))
            .app_data(web::Data::from(features_cache.clone()))
            .app_data(web::Data::from(engine_cache.clone()))
            .app_data(web::Data::from(broadcaster.clone()))
            .app_data(web::Data::from(our_instance_data_for_app_context.clone()))
            .app_data(web::Data::from(instances_observed_for_app_context.clone()));

        app = match token_validator.clone() {
            Some(v) => app.app_data(web::Data::from(v)),
            None => app,
        };
        app = match refresher_for_app_data.clone() {
            Some(refresher) => app.app_data(web::Data::from(refresher)),
            None => app,
        };
        app.service(
            web::scope(&base_path)
                .wrap(Etag)
                .wrap(actix_web::middleware::Compress::default())
                .wrap(actix_web::middleware::NormalizePath::default())
                .wrap(cors_middleware)
                .wrap(request_metrics.clone())
                .wrap(Logger::default())
                .service(web::scope("/internal-backstage").configure(|service_cfg| {
                    internal_backstage::configure_internal_backstage(
                        service_cfg,
                        metrics_handler.clone(),
                        internal_backstage_args.clone(),
                    )
                }))
                .service(
                    web::scope("/api")
                        .configure(client_api::configure_client_api)
                        .configure(|cfg| {
                            frontend_api::configure_frontend_api(cfg, disable_all_endpoint)
                        }),
                )
                .service(web::scope("/edge").configure(edge_api::configure_edge_api))
                .service(
                    SwaggerUi::new("/swagger-ui/{_:.*}")
                        .url("/api-doc/openapi.json", openapi.clone()),
                ),
        )
    });
    let server = if http_args.tls.tls_enable {
        let config = tls::config(http_args.clone().tls)
            .expect("Was expecting to succeed in configuring TLS");
        server
            .bind_rustls_0_23(http_args.https_server_tuple(), config)?
            .bind(http_args.http_server_tuple())
    } else {
        server.bind(http_args.http_server_tuple())
    };
    let server = server?
        .workers(http_args.workers)
        .shutdown_timeout(5)
        .keep_alive(std::time::Duration::from_secs(keepalive_timeout))
        .client_request_timeout(std::time::Duration::from_secs(request_timeout));

    match schedule_args.mode {
        EdgeMode::Edge(edge) => {
            let refresher_for_background = feature_refresher.clone().unwrap();
            if edge.streaming {
                let app_name = app_name.clone();
                let custom_headers = custom_headers.clone();
                if edge.delta {
                    tokio::spawn(async move {
                        let _ = refresher_for_background
                            .start_streaming_delta_background_task(
                                ClientMetaInformation {
                                    app_name,
                                    instance_id,
                                },
                                custom_headers,
                            )
                            .await;
                    });
                } else {
                    tokio::spawn(async move {
                        let _ = refresher_for_background
                            .start_streaming_features_background_task(
                                ClientMetaInformation {
                                    app_name,
                                    instance_id,
                                },
                                custom_headers,
                            )
                            .await;
                    });
                }
            }

            let refresher = feature_refresher.clone().unwrap();

            let validator = token_validator_schedule.clone().unwrap();

            tokio::select! {
                _ = server.run() => {
                    info!("Actix is shutting down. Persisting data");
                    clean_shutdown(persistence.clone(), lazy_feature_cache.clone(), lazy_token_cache.clone(), metrics_cache_clone.clone(), feature_refresher.clone()).await;
                    info!("Actix was shutdown properly");
                },
                _ = refresher.start_refresh_features_background_task() => {
                    info!("Feature refresher unexpectedly shut down");
                }
                _ = unleash_edge::http::background_send_metrics::send_metrics_task(metrics_cache_clone.clone(), refresher.clone(), edge.metrics_interval_seconds.try_into().unwrap()) => {
                    info!("Metrics poster unexpectedly shut down");
                }
                _ = persist_data(persistence.clone(), lazy_token_cache.clone(), lazy_feature_cache.clone()) => {
                    info!("Persister was unexpectedly shut down");
                }
                _ = validator.schedule_validation_of_known_tokens(edge.token_revalidation_interval_seconds) => {
                    info!("Token validator validation of known tokens was unexpectedly shut down");
                }
                _ = validator.schedule_revalidation_of_startup_tokens(edge.tokens, lazy_feature_refresher.clone()) => {
                    info!("Token validator validation of startup tokens was unexpectedly shut down");
                }
                _ = metrics_pusher::prometheus_remote_write(prom_registry_for_write.clone(), edge.prometheus_remote_write_url, edge.prometheus_push_interval, edge.prometheus_username, edge.prometheus_password, app_name) => {
                    info!("Prometheus push unexpectedly shut down");
                }
                _ = unleash_edge::http::instance_data::send_instance_data(instance_data_sender?, prom_registry_for_write.clone(), our_instance_data.clone(), downstream_instance_data.clone()) => {
                    info!("Instance data pusher unexpectedly quit");
                }
            }
        }
        EdgeMode::Offline(offline_args) if offline_args.reload_interval > 0 => {
            tokio::select! {
                _ = offline_hotload::start_hotload_loop(lazy_feature_cache, lazy_engine_cache, offline_args) => {
                    info!("Hotloader unexpectedly shut down.");
                },
                _ = server.run() => {
                    info!("Actix is shutting down. No pending tasks.");
                },
            }
        }
        _ => tokio::select! {
            _ = server.run() => {
                info!("Actix is shutting down. Persisting data");
                clean_shutdown(persistence, lazy_feature_cache.clone(), lazy_token_cache.clone(), metrics_cache_clone.clone(), feature_refresher.clone()).await;
                info!("Actix was shutdown properly");

            }
        },
    };

    Ok(())
}

#[cfg(not(tarpaulin_include))]
async fn clean_shutdown(
    persistence: Option<Arc<dyn EdgePersistence>>,
    feature_cache: Arc<FeatureCache>,
    token_cache: Arc<DashMap<String, EdgeToken>>,
    metrics_cache: Arc<MetricsCache>,
    feature_refresher: Option<Arc<FeatureRefresher>>,
) {
    let tokens: Vec<EdgeToken> = token_cache
        .iter()
        .filter(|e| e.value().status == TokenValidationStatus::Validated)
        .map(|entry| entry.value().clone())
        .collect();

    let features: Vec<(String, ClientFeatures)> = feature_cache
        .iter()
        .map(|entry| (entry.key().clone(), entry.value().clone()))
        .collect();

    if let Some(persistence) = persistence {
        let res = join_all(vec![
            persistence.save_tokens(tokens),
            persistence.save_features(features),
        ])
        .await;
        if res.iter().all(|save| save.is_ok()) {
            info!("Successfully persisted data to storage backend");
        } else {
            res.iter()
                .filter(|save| save.is_err())
                .for_each(|failed_save| tracing::error!("Failed backing up: {failed_save:?}"));
        }
    }
    if let Some(feature_refresher) = feature_refresher {
        info!("Connected to an upstream, flushing last set of metrics");
        send_metrics_one_shot(metrics_cache, feature_refresher).await;
    }
}
