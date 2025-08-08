use actix_allow_deny_middleware::{AllowList, DenyList};
use actix_middleware_etag::Etag;
use actix_web::dev::Server;
use actix_web::{App, HttpServer, web};
use chrono::Duration;
use clap::Parser;
use dashmap::DashMap;
use futures::future::join_all;
use lazy_static::lazy_static;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;
use ulid::Ulid;
use unleash_edge::auth::token_validator::SHOULD_DEFER_VALIDATION;
use unleash_edge::error::EdgeError;
use unleash_edge::http::unleash_client::{HttpClientArgs, new_reqwest_client};
use unleash_edge::metrics::actix_web_prometheus_metrics::PrometheusMetrics;
use unleash_edge::middleware::fail_response_logger::LogStatus;
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::ConnectVia;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use tracing::info;
use unleash_edge::builder::{EdgeInfo, build_edge, build_offline};
use unleash_edge::cli::{AuthHeaders, CliArgs, EdgeMode};
use unleash_edge::feature_cache::FeatureCache;
use unleash_edge::http::background_send_metrics::send_metrics_one_shot;
use unleash_edge::http::broadcaster::Broadcaster;
use unleash_edge::http::instance_data::InstanceDataSending;
use unleash_edge::http::refresher::feature_refresher::FeatureRefresher;
use unleash_edge::metrics::client_metrics::MetricsCache;
use unleash_edge::metrics::edge_metrics::{EdgeInstanceData, Hosting};
use unleash_edge::offline::offline_hotload;
use unleash_edge::persistence::{EdgePersistence, persist_data};
use unleash_edge::types::{EdgeResult, EdgeToken, TokenValidationStatus};
use unleash_edge::{client_api, frontend_api, health_checker, openapi, ready_checker};
use unleash_edge::{edge_api, prom_metrics};
use unleash_edge::{http::unleash_client::ClientMetaInformation, metrics::metrics_pusher};
use unleash_edge::{internal_backstage, tls};

lazy_static! {
    pub static ref SHOULD_FORCE_STRONG_ETAGS: bool = {
        env::var("EDGE_FORCE_STRONG_ETAGS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
    };
}

fn setup_server(
    args: CliArgs,
    edge_info: EdgeInfo,
    metrics_middleware: PrometheusMetrics,
    instance_data_sender_for_app_context: Arc<InstanceDataSending>,
    metrics_cache: Arc<MetricsCache>,
    our_instance_data_for_app_context: Arc<EdgeInstanceData>,
    instances_observed_for_app_context: Arc<RwLock<Vec<EdgeInstanceData>>>,
) -> EdgeResult<Server> {
    let http_args = args.clone().http;
    let request_timeout = args.edge_request_timeout;
    let keepalive_timeout = args.edge_keepalive_timeout;

    let (
        (token_cache, features_cache, delta_cache_manager, engine_cache),
        token_validator,
        feature_refresher,
        _,
    ) = edge_info;

    let server = HttpServer::new(move || {
        let qs_config =
            serde_qs::actix::QsQueryConfig::default().qs_config(serde_qs::Config::new(5, false));

        let connect_via = ConnectVia {
            app_name: args.app_name.clone(),
            instance_id: our_instance_data_for_app_context.identifier.clone(),
        };

        let cors_middleware = args.http.cors.middleware();
        let mut app = App::new()
            .wrap(LogStatus)
            .app_data(qs_config)
            .app_data(web::Data::new(args.token_header.clone()))
            .app_data(web::Data::new(args.trust_proxy.clone()))
            .app_data(web::Data::new(args.mode.clone()))
            .app_data(web::Data::new(connect_via))
            .app_data(web::Data::from(metrics_cache.clone()))
            .app_data(web::Data::from(token_cache.clone()))
            .app_data(web::Data::from(delta_cache_manager.clone()))
            .app_data(web::Data::from(features_cache.clone()))
            .app_data(web::Data::from(engine_cache.clone()))
            .app_data(web::Data::from(Broadcaster::new(
                delta_cache_manager.clone(),
            )))
            .app_data(web::Data::from(Arc::new(AuthHeaders::from(&args))))
            .app_data(web::Data::from(
                instance_data_sender_for_app_context.clone(),
            ))
            .app_data(web::Data::from(our_instance_data_for_app_context.clone()))
            .app_data(web::Data::from(instances_observed_for_app_context.clone()));

        if let Some(ref token_validator) = token_validator {
            app = app.app_data(web::Data::from(token_validator.clone()));
        }
        if let Some(ref refresher) = feature_refresher {
            app = app.app_data(web::Data::from(refresher.clone()));
        }

        app.service(
            web::scope(&args.http.base_path)
                .wrap(Etag {
                    force_strong_etag: *SHOULD_FORCE_STRONG_ETAGS,
                })
                .wrap(actix_web::middleware::Compress::default())
                .wrap(actix_web::middleware::NormalizePath::default())
                .wrap(cors_middleware)
                .wrap(metrics_middleware.clone())
                .service(web::scope("/internal-backstage").configure(|service_cfg| {
                    internal_backstage::configure_internal_backstage(
                        service_cfg,
                        args.internal_backstage.clone(),
                    )
                }))
                .service(
                    web::scope("/api")
                        .configure(client_api::configure_client_api)
                        .configure(|cfg| {
                            frontend_api::configure_frontend_api(cfg, args.disable_all_endpoint)
                        })
                        .wrap(DenyList::with_denied_ipnets(
                            &args.http.clone().deny_list.clone().unwrap_or_default(),
                        ))
                        .wrap(
                            args.http
                                .clone()
                                .allow_list
                                .clone()
                                .map(|list| AllowList::with_allowed_ipnets(&list))
                                .unwrap_or_default(),
                        ),
                )
                .service(
                    web::scope("/edge")
                        .configure(edge_api::configure_edge_api)
                        .wrap(DenyList::with_denied_ipnets(
                            &args.http.clone().deny_list.clone().unwrap_or_default(),
                        ))
                        .wrap(
                            args.http
                                .clone()
                                .allow_list
                                .clone()
                                .map(|list| AllowList::with_allowed_ipnets(&list))
                                .unwrap_or_default(),
                        ),
                )
                .service(
                    SwaggerUi::new("/swagger-ui/{_:.*}")
                        .url("/api-doc/openapi.json", openapi::ApiDoc::openapi()),
                ),
        )
    });
    let server = if http_args.tls.tls_enable {
        let config = tls::config(http_args.clone().tls)
            .expect("Was expecting to succeed in configuring TLS");
        server
            .bind_rustls_0_23(http_args.https_server_tuple(), config)
            .map_err(|e| EdgeError::TlsError(e.to_string()))?
            .bind(http_args.http_server_tuple())
    } else {
        server.bind(http_args.http_server_tuple())
    }
    .map_err(|e| EdgeError::ReadyCheckError(e.to_string()))?;
    let server = server
        .workers(http_args.workers)
        .shutdown_timeout(5)
        .keep_alive(std::time::Duration::from_secs(keepalive_timeout))
        .client_request_timeout(std::time::Duration::from_secs(request_timeout));
    Ok(server.run())
}

#[cfg(not(tarpaulin_include))]
#[actix_web::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = CliArgs::parse();
    if args.markdown_help {
        clap_markdown::print_help_markdown::<CliArgs>();
        return Ok(());
    }

    match args.mode {
        EdgeMode::Health(health_args) => health_checker::check_health(health_args).await,
        EdgeMode::Ready(ready_args) => ready_checker::check_ready(ready_args).await,
        _ => run_server(args).await,
    }
    .map_err(|e| e.into())
}

async fn run_server(args: CliArgs) -> EdgeResult<()> {
    let app_name = args.app_name.clone();
    let app_id = Ulid::new();
    let hosting_strategy = std::env::var("EDGE_HOSTING")
        .map(Into::into)
        .unwrap_or(Hosting::SelfHosted);
    let edge_instance_data = Arc::new(EdgeInstanceData::new(
        &args.app_name,
        &app_id,
        hosting_strategy,
    ));
    let client_meta_information = ClientMetaInformation {
        app_name: args.app_name.clone(),
        instance_id: app_id.to_string(),
        connection_id: app_id.to_string(),
    };

    let metrics_middleware = prom_metrics::instantiate(
        None,
        args.internal_backstage.disable_metrics_endpoint,
        &args.log_format,
        &edge_instance_data.clone(),
    );

    let (edge_info, instance_data_sender, token_validation_queue) = match &args.mode {
        EdgeMode::Edge(edge_args) => {
            let client = new_reqwest_client(HttpClientArgs {
                skip_ssl_verification: edge_args.skip_ssl_verification,
                client_identity: edge_args.client_identity.clone(),
                upstream_certificate_file: edge_args.upstream_certificate_file.clone(),
                connect_timeout: Duration::seconds(edge_args.upstream_request_timeout),
                socket_timeout: Duration::seconds(edge_args.upstream_socket_timeout),
                keep_alive_timeout: Duration::seconds(edge_args.client_keepalive_timeout),
                client_meta_information: client_meta_information.clone(),
            })?;

            let (deferred_validation_tx, deferred_validation_rx) = if *SHOULD_DEFER_VALIDATION {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                (Some(tx), Some(rx))
            } else {
                (None, None)
            };

            let auth_headers = AuthHeaders::from(&args);
            let caches = build_edge(
                edge_args,
                client_meta_information.clone(),
                auth_headers,
                client.clone(),
                deferred_validation_tx,
            )
            .await?;

            let instance_data_sender: Arc<InstanceDataSending> =
                Arc::new(InstanceDataSending::from_args(
                    args.clone(),
                    &client_meta_information,
                    client,
                    metrics_middleware.registry.clone(),
                )?);

            (caches, instance_data_sender, deferred_validation_rx)
        }
        EdgeMode::Offline(offline_args) => {
            let caches =
                build_offline(offline_args.clone()).map(|cache| (cache, None, None, None))?;
            (caches, Arc::new(InstanceDataSending::SendNothing), None)
        }
        _ => unreachable!(),
    };

    let (
        (token_cache, features_cache, _, engine_cache),
        token_validator,
        feature_refresher,
        persistence,
    ) = edge_info.clone();

    let instance_data_sender_for_app_context = instance_data_sender.clone();
    let lazy_feature_cache = features_cache.clone();
    let lazy_token_cache = token_cache.clone();
    let lazy_engine_cache = engine_cache.clone();
    let lazy_feature_refresher = feature_refresher.clone();
    let metrics_cache = Arc::new(MetricsCache::default());
    let metrics_cache_clone = metrics_cache.clone();

    let instances_observed_for_app_context: Arc<RwLock<Vec<EdgeInstanceData>>> =
        Arc::new(RwLock::new(Vec::new()));

    let server = setup_server(
        args.clone(),
        edge_info,
        metrics_middleware.clone(),
        instance_data_sender_for_app_context,
        metrics_cache.clone(),
        edge_instance_data.clone(),
        instances_observed_for_app_context.clone(),
    )?;

    if let (Some(validator), Some(rx)) = (token_validator.clone(), token_validation_queue) {
        tokio::spawn(async move {
            validator.schedule_deferred_validation(rx).await;
        });
    }

    match &args.mode {
        EdgeMode::Edge(edge) => {
            let refresher_for_background = feature_refresher.clone().unwrap();
            if edge.streaming {
                let custom_headers = edge.custom_client_headers.clone();
                if edge.delta {
                    tokio::spawn(async move {
                        let _ = refresher_for_background
                            .start_streaming_delta_background_task(
                                client_meta_information,
                                custom_headers,
                            )
                            .await;
                    });
                } else {
                    tokio::spawn(async move {
                        let _ = refresher_for_background
                            .start_streaming_features_background_task(
                                client_meta_information,
                                custom_headers,
                            )
                            .await;
                    });
                }
            }

            let refresher = feature_refresher.clone().unwrap();
            let validator = token_validator.clone().unwrap();

            tokio::select! {
                _ = server => {
                    info!("Actix is shutting down. Persisting data");
                    clean_shutdown(persistence, lazy_feature_cache.clone(), lazy_token_cache.clone(), metrics_cache_clone.clone(), feature_refresher.clone(), InstanceDataShutdownArgs { instance_data_sending: instance_data_sender.clone(), our_instance_data: edge_instance_data.clone(), downstream_instance_data: instances_observed_for_app_context.clone() }).await;
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
                _ = validator.schedule_revalidation_of_startup_tokens(edge.tokens.clone(), lazy_feature_refresher.clone()) => {
                    info!("Token validator validation of startup tokens was unexpectedly shut down");
                }
                _ = metrics_pusher::prometheus_remote_write(metrics_middleware.registry.clone(), edge.prometheus_remote_write_url.clone(), edge.prometheus_push_interval, edge.prometheus_username.clone(), edge.prometheus_password.clone(), app_name) => {
                    info!("Prometheus push unexpectedly shut down");
                }
                _ = unleash_edge::http::instance_data::loop_send_instance_data(instance_data_sender.clone(), edge_instance_data.clone(), instances_observed_for_app_context.clone()) => {
                    info!("Instance data pusher unexpectedly quit");
                }
            }
        }
        EdgeMode::Offline(offline_args) if offline_args.reload_interval > 0 => {
            tokio::select! {
                _ = offline_hotload::start_hotload_loop(lazy_feature_cache, lazy_engine_cache, offline_args.clone()) => {
                    info!("Hotloader unexpectedly shut down.");
                },
                _ = server => {
                    info!("Actix is shutting down. No pending tasks.");
                },
            }
        }
        _ => tokio::select! {
            _ = server => {
                info!("Actix is shutting down. Persisting data");
                clean_shutdown(persistence, lazy_feature_cache.clone(), lazy_token_cache.clone(), metrics_cache_clone.clone(), feature_refresher.clone(), InstanceDataShutdownArgs { instance_data_sending: instance_data_sender.clone(), our_instance_data: edge_instance_data.clone(), downstream_instance_data: instances_observed_for_app_context.clone() }).await;
                info!("Actix was shutdown properly");

            }
        },
    };

    Ok(())
}

struct InstanceDataShutdownArgs {
    instance_data_sending: Arc<InstanceDataSending>,
    our_instance_data: Arc<EdgeInstanceData>,
    downstream_instance_data: Arc<RwLock<Vec<EdgeInstanceData>>>,
}

#[cfg(not(tarpaulin_include))]
async fn clean_shutdown(
    persistence: Option<Arc<dyn EdgePersistence>>,
    feature_cache: Arc<FeatureCache>,
    token_cache: Arc<DashMap<String, EdgeToken>>,
    metrics_cache: Arc<MetricsCache>,
    feature_refresher: Option<Arc<FeatureRefresher>>,
    instance_data_shutdown: InstanceDataShutdownArgs,
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
    match instance_data_shutdown.instance_data_sending.as_ref() {
        InstanceDataSending::SendInstanceData(instance_data_sender) => {
            info!("Connected to an upstream, flushing last set of instance data");
            let _ = unleash_edge::http::instance_data::send_instance_data(
                instance_data_sender,
                instance_data_shutdown.our_instance_data,
                instance_data_shutdown.downstream_instance_data,
            )
            .await;
        }
        InstanceDataSending::SendNothing => {
            info!("No instance data sender configured, skipping flushing instance data");
        }
    }
}
