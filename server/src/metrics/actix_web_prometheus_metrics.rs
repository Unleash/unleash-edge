/// A big thank you to https://github.com/atomix-team/actix-web-prometheus for the initial idea
/// but https://github.com/nlopes/actix-web-prom had fixed comments and tests which we use with pride.
///
/// Uses different metric names to more closely align with what we had in our old metrics middleware.
/// In addition, we needed the possibility to deny access to the metrics endpoint if the user had disabled it at startup
use futures_core::ready;
use std::{
    collections::{HashMap, HashSet},
    future::{Ready, ready},
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};

use actix_http::{
    Method, StatusCode,
    header::{CONTENT_TYPE, HeaderValue},
};
use actix_service::{Service, Transform, forward_ready};
use actix_web::{
    HttpMessage,
    body::{BodySize, EitherBody, MessageBody},
    dev::{ServiceRequest, ServiceResponse},
    web::{Bytes, Data},
};
use pin_project_lite::pin_project;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};
use tracing::warn;

use super::edge_metrics::EdgeInstanceData;

/// MetricsConfig define middleware and config struct to change the behaviour of the metrics
/// struct to define some particularities
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// list of params where the cardinality matters
    pub cardinality_keep_params: Vec<String>,
}

#[derive(Debug)]
/// Builder to create new PrometheusMetrics struct.HistogramVec
///
/// It allows setting optional parameters like registry, buckets, etc.
pub struct PrometheusMetricsBuilder {
    namespace: String,
    endpoint: Option<String>,
    const_labels: HashMap<String, String>,
    registry: Registry,
    buckets: Vec<f64>,
    size_buckets: Vec<f64>,
    exclude: HashSet<String>,
    exclude_status: HashSet<StatusCode>,
    unmatched_patterns_mask: Option<String>,
    disable_metrics_endpoint: bool,
    metrics_configuration: ActixMetricsConfiguration,
}
impl PrometheusMetricsBuilder {
    /// Create new `PrometheusMetricsBuilder`
    ///
    /// namespace example: "actix"
    pub fn new(namespace: &str) -> Self {
        Self {
            namespace: namespace.into(),
            endpoint: None,
            const_labels: HashMap::new(),
            registry: Registry::new(),
            buckets: prometheus::DEFAULT_BUCKETS.to_vec(),
            size_buckets: vec![
                5.0, 10.0, 100.0, 250.0, 500.0, 750.0, 1000.0, 2500.0, 5000.0, 10000.0, 25000.0,
                50000.0, 100000.0,
            ],
            exclude: HashSet::new(),
            exclude_status: HashSet::new(),
            unmatched_patterns_mask: None,
            disable_metrics_endpoint: false,
            metrics_configuration: ActixMetricsConfiguration::default(),
        }
    }

    /// Set actix web endpoint
    ///
    /// Example: "/metrics"
    pub fn endpoint(mut self, value: &str) -> Self {
        self.endpoint = Some(value.into());
        self
    }

    /// Set histogram buckets
    pub fn buckets(mut self, value: &[f64]) -> Self {
        self.buckets = value.to_vec();
        self
    }

    /// Set labels to add on every metrics
    pub fn const_labels(mut self, value: HashMap<String, String>) -> Self {
        self.const_labels = value;
        self
    }

    /// Set registry
    ///
    /// By default one is set and is internal to `PrometheusMetrics`
    pub fn registry(mut self, value: Registry) -> Self {
        self.registry = value;
        self
    }

    /// Ignore and do not record metrics for specified path.
    pub fn exclude<T: Into<String>>(mut self, path: T) -> Self {
        self.exclude.insert(path.into());
        self
    }

    /// Disable the metrics endpoint
    pub fn disable_metrics_endpoint(mut self, value: bool) -> Self {
        self.disable_metrics_endpoint = value;
        self
    }

    /// Ignore and do not record metrics for paths returning the status code.
    pub fn exclude_status<T: Into<StatusCode>>(mut self, status: T) -> Self {
        self.exclude_status.insert(status.into());
        self
    }

    /// Replaces the request path with the supplied mask if no actix-web handler is matched
    pub fn mask_unmatched_patterns<T: Into<String>>(mut self, mask: T) -> Self {
        self.unmatched_patterns_mask = Some(mask.into());
        self
    }

    /// Set metrics configuration
    pub fn metrics_configuration(mut self, value: ActixMetricsConfiguration) -> Self {
        self.metrics_configuration = value;
        self
    }

    /// Instantiate `PrometheusMetrics` struct
    pub fn build(self) -> Result<PrometheusMetrics, Box<dyn std::error::Error + Send + Sync>> {
        let labels_vec = self.metrics_configuration.labels.clone().to_vec();
        let labels = &labels_vec.iter().map(|s| s.as_str()).collect::<Vec<&str>>();

        let http_requests_total_opts = Opts::new(
            self.metrics_configuration
                .http_requests_total_name
                .to_owned(),
            "Total number of HTTP requests",
        )
        .namespace(&self.namespace)
        .const_labels(self.const_labels.clone());

        let http_requests_total = IntCounterVec::new(http_requests_total_opts, labels)?;

        let http_requests_duration_seconds_opts = HistogramOpts::new(
            self.metrics_configuration
                .http_requests_duration_seconds
                .to_owned(),
            "HTTP request duration in seconds for all requests",
        )
        .namespace(&self.namespace)
        .buckets(self.buckets.to_vec())
        .const_labels(self.const_labels.clone());

        let http_requests_duration_seconds =
            HistogramVec::new(http_requests_duration_seconds_opts, labels)?;

        let http_response_size_opts = HistogramOpts::new(
            self.metrics_configuration
                .http_response_size_name
                .to_owned(),
            "HTTP response size in bytes for all requests",
        )
        .namespace(&self.namespace)
        .buckets(self.size_buckets.to_vec())
        .const_labels(self.const_labels.clone());

        let http_response_size_bytes = HistogramVec::new(http_response_size_opts, labels)?;

        self.registry
            .register(Box::new(http_requests_total.clone()))?;
        self.registry
            .register(Box::new(http_requests_duration_seconds.clone()))?;
        self.registry
            .register(Box::new(http_response_size_bytes.clone()))?;
        Ok(PrometheusMetrics {
            http_requests_total,
            http_requests_duration_seconds,
            http_response_size_bytes,
            registry: self.registry,
            namespace: self.namespace,
            endpoint: self.endpoint,
            const_labels: self.const_labels,
            exclude: self.exclude,
            exclude_status: self.exclude_status,
            enable_http_version_label: self.metrics_configuration.labels.version.is_some(),
            unmatched_patterns_mask: self.unmatched_patterns_mask,
            expose_metrics_endpoint: !self.disable_metrics_endpoint,
        })
    }
}

#[derive(Debug, Clone)]
///Configurations for the labels used in metrics
pub struct LabelsConfiguration {
    endpoint: String,
    method: String,
    status: String,
    version: Option<String>,
}

pub(crate) const ENDPOINT_LABEL: &str = "endpoint";
pub(crate) const METHOD_LABEL: &str = "method";
pub(crate) const STATUS_LABEL: &str = "status";

impl Default for LabelsConfiguration {
    fn default() -> Self {
        Self {
            endpoint: ENDPOINT_LABEL.to_string(),
            method: METHOD_LABEL.to_string(),
            status: STATUS_LABEL.to_string(),
            version: None,
        }
    }
}

impl LabelsConfiguration {
    fn to_vec(&self) -> Vec<String> {
        let mut labels = vec![
            self.endpoint.clone(),
            self.method.clone(),
            self.status.clone(),
        ];
        if let Some(version) = self.version.clone() {
            labels.push(version);
        }
        labels
    }

    /// set http method label
    pub fn method(mut self, name: &str) -> Self {
        self.method = name.to_owned();
        self
    }

    /// set http endpoint label
    pub fn endpoint(mut self, name: &str) -> Self {
        self.endpoint = name.to_owned();
        self
    }

    /// set http status label
    pub fn status(mut self, name: &str) -> Self {
        self.status = name.to_owned();
        self
    }

    /// set http version label
    pub fn version(mut self, name: &str) -> Self {
        self.version = Some(name.to_owned());
        self
    }
}

#[derive(Debug, Clone)]
/// Configuration for the collected metrics
///
/// Stores individual metric configuration objects
pub struct ActixMetricsConfiguration {
    http_requests_total_name: String,
    http_requests_duration_seconds: String,
    http_response_size_name: String,
    labels: LabelsConfiguration,
}

pub(crate) const HTTP_REQUESTS_TOTAL: &str = "http_requests_total";
pub(crate) const HTTP_REQUESTS_DURATION: &str = "http_requests_duration_seconds";
pub(crate) const HTTP_RESPONSE_SIZE: &str = "http_response_size";

impl Default for ActixMetricsConfiguration {
    fn default() -> Self {
        Self {
            http_requests_total_name: HTTP_REQUESTS_TOTAL.to_string(),
            http_requests_duration_seconds: HTTP_REQUESTS_DURATION.to_string(),
            http_response_size_name: HTTP_RESPONSE_SIZE.to_string(),
            labels: LabelsConfiguration::default(),
        }
    }
}

impl ActixMetricsConfiguration {
    /// Set the labels collected for the metrics
    pub fn labels(mut self, labels: LabelsConfiguration) -> Self {
        self.labels = labels;
        self
    }

    /// Set name for `http_requests_total` metric
    pub fn http_requests_total_name(mut self, name: &str) -> Self {
        self.http_requests_total_name = name.to_owned();
        self
    }

    /// Set name for `http_requests_duration_seconds` metric
    pub fn http_requests_duration_seconds_name(mut self, name: &str) -> Self {
        self.http_requests_duration_seconds = name.to_owned();
        self
    }
    /// Set name for 'http_response_size' metric
    pub fn http_response_size_name(mut self, name: &str) -> Self {
        self.http_response_size_name = name.to_owned();
        self
    }
}

#[derive(Clone)]
#[must_use = "must be set up as middleware for actix-web"]
/// By default three metrics are tracked (this assumes the namespace ``):
///
///   - `http_requests_total` (labels: endpoint, method, status): the total
///     number of HTTP requests handled by the actix `HttpServer`.
///
///   - `http_requests_duration_milliseconds` (labels: endpoint, method,
///     status): the request duration for all HTTP requests handled by the actix
///     `HttpServer`.
///
///   - `http_server_response_size` (labels: endpoint, method, status): the request size for all HTTP requests handled by the actix `HttpServer`.
pub struct PrometheusMetrics {
    pub(crate) http_requests_total: IntCounterVec,
    pub(crate) http_requests_duration_seconds: HistogramVec,
    pub(crate) http_response_size_bytes: HistogramVec,

    /// exposed registry for custom prometheus metrics
    pub registry: Registry,
    #[allow(dead_code)]
    pub(crate) namespace: String,
    pub(crate) endpoint: Option<String>,
    #[allow(dead_code)]
    pub(crate) const_labels: HashMap<String, String>,
    pub(crate) expose_metrics_endpoint: bool,
    pub(crate) exclude: HashSet<String>,
    pub(crate) exclude_status: HashSet<StatusCode>,
    pub(crate) enable_http_version_label: bool,
    pub(crate) unmatched_patterns_mask: Option<String>,
}

struct MetricsUpdate<'a, 'b> {
    size: usize,
    mixed_pattern: &'a str,
    fallback_pattern: &'b str,
    method: &'a Method,
    status: StatusCode,
    clock: Instant,
    was_path_matched: bool,
}
impl PrometheusMetrics {
    fn metrics(&self) -> String {
        let mut buffer = vec![];
        TextEncoder::new()
            .encode(&self.registry.gather(), &mut buffer)
            .unwrap();
        String::from_utf8(buffer).unwrap()
    }

    fn matches(&self, path: &str, method: &Method) -> bool {
        self.endpoint.as_ref().is_some_and(|ep| ep == path) && method == Method::GET
    }

    fn update_metrics(
        &self,
        MetricsUpdate {
            size,
            mixed_pattern,
            fallback_pattern,
            method,
            status,
            clock,
            was_path_matched,
        }: MetricsUpdate,
    ) {
        if self.exclude.contains(mixed_pattern) || self.exclude_status.contains(&status) {
            return;
        }
        // do not record mixed patterns that were considered invalid by the server
        let final_pattern = if fallback_pattern != mixed_pattern && (status == 404 || status == 405)
        {
            fallback_pattern
        } else {
            mixed_pattern
        };

        let final_pattern = if was_path_matched {
            final_pattern
        } else if let Some(mask) = &self.unmatched_patterns_mask {
            mask
        } else {
            final_pattern
        };

        let label_values = [final_pattern, method.as_str(), status.as_str()];
        let label_values = if self.enable_http_version_label {
            &label_values[..]
        } else {
            &label_values[..3]
        };
        let elapsed = clock.elapsed();
        let duration = elapsed.as_secs_f64() + ((elapsed.subsec_nanos() as f64) / 1_000_000_000.0);
        self.http_requests_duration_seconds
            .with_label_values(label_values)
            .observe(duration);
        self.http_requests_total
            .with_label_values(label_values)
            .inc();
        self.http_response_size_bytes
            .with_label_values(label_values)
            .observe(size as f64);
    }
}

impl<S, B> Transform<S, ServiceRequest> for PrometheusMetrics
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
{
    type Response = ServiceResponse<EitherBody<StreamLog<B>, StreamLog<String>>>;
    type Error = actix_web::Error;
    type InitError = ();
    type Transform = PrometheusMetricsMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(PrometheusMetricsMiddleware {
            service,
            inner: Arc::new(self.clone()),
        }))
    }
}

pin_project! {
    #[doc(hidden)]
    pub struct LoggerResponse<S>
        where
        S: Service<ServiceRequest>,
    {
        #[pin]
        fut: S::Future,
        time: Instant,
        inner: Arc<PrometheusMetrics>,
        _t: PhantomData<()>,
    }
}

impl<S, B> Future for LoggerResponse<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
{
    type Output =
        Result<ServiceResponse<EitherBody<StreamLog<B>, StreamLog<String>>>, actix_web::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let res = match ready!(this.fut.poll(cx)) {
            Ok(res) => res,
            Err(e) => return Poll::Ready(Err(e)),
        };

        let time = *this.time;
        let req = res.request();
        let method = req.method().clone();
        let was_path_matched = req.match_pattern().is_some();

        // get metrics config for this specific route
        // piece of code to allow for more cardinality
        let params_keep_path_cardinality = match req.extensions_mut().get::<MetricsConfig>() {
            Some(config) => config.cardinality_keep_params.clone(),
            None => vec![],
        };

        let full_pattern = req.match_pattern();
        let path = req.path().to_string();
        let fallback_pattern = full_pattern.clone().unwrap_or(path.clone());

        // mixed_pattern is the final path used as label value in metrics
        let mixed_pattern = match full_pattern {
            None => path.clone(),
            Some(full_pattern) => {
                let mut params: HashMap<String, String> = HashMap::new();

                for (key, val) in req.match_info().iter() {
                    if params_keep_path_cardinality.contains(&key.to_string()) {
                        params.insert(key.to_string(), val.to_string());
                        continue;
                    }
                    params.insert(key.to_string(), format!("{{{key}}}"));
                }

                if let Ok(mixed_cardinality_pattern) = strfmt::strfmt(&full_pattern, &params) {
                    mixed_cardinality_pattern
                } else {
                    warn!(
                        "Cannot build mixed cardinality pattern {full_pattern}, with params {params:?}"
                    );
                    full_pattern
                }
            }
        };
        // get metrics config for this specific route
        // piece of code to allow for more cardinality

        let path = req.path().to_string();

        let edge_instance_data = req.app_data::<Data<EdgeInstanceData>>().cloned();
        let inner = this.inner.clone();

        Poll::Ready(Ok(res.map_body(move |head, body| {
            // We short circuit the response status and body to serve the endpoint
            // automagically. This way the user does not need to set the middleware *AND*
            // an endpoint to serve middleware results. The user is only required to set
            // the middleware and tell us what the endpoint should be.
            if inner.matches(&path, &method) {
                if !inner.expose_metrics_endpoint {
                    head.status = StatusCode::FORBIDDEN;
                    EitherBody::right(StreamLog {
                        body: "".to_string(),
                        size: 0,
                        clock: time,
                        inner,
                        status: head.status,
                        mixed_pattern,
                        fallback_pattern,
                        method,
                        was_path_matched: true,
                    })
                } else {
                    head.status = StatusCode::OK;
                    head.headers.insert(
                        CONTENT_TYPE,
                        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
                    );

                    EitherBody::right(StreamLog {
                        body: inner.metrics(),
                        size: 0,
                        clock: time,
                        inner,
                        status: head.status,
                        mixed_pattern,
                        fallback_pattern,
                        method,
                        was_path_matched: true,
                    })
                }
            } else {
                if let Some(instance_data) = edge_instance_data {
                    if let Some(endpoint) = super::edge_metrics::DESIRED_URLS
                        .into_iter()
                        .find(|u| path.ends_with(u))
                    {
                        instance_data.observe_request(endpoint, head.status.as_u16());
                    }
                }
                EitherBody::left(StreamLog {
                    body,
                    size: head
                        .headers()
                        .get(actix_http::header::CONTENT_LENGTH)
                        .map_or(0, |v| v.to_str().unwrap_or("0").parse().unwrap_or(0)),
                    clock: time,
                    inner,
                    status: head.status,
                    mixed_pattern,
                    fallback_pattern,
                    method,
                    was_path_matched,
                })
            }
        })))
    }
}

#[doc(hidden)]
/// Middleware service for PrometheusMetrics
pub struct PrometheusMetricsMiddleware<S> {
    service: S,
    inner: Arc<PrometheusMetrics>,
}

impl<S, B> Service<ServiceRequest> for PrometheusMetricsMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
{
    type Response = ServiceResponse<EitherBody<StreamLog<B>, StreamLog<String>>>;
    type Error = S::Error;
    type Future = LoggerResponse<S>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        LoggerResponse {
            fut: self.service.call(req),
            time: Instant::now(),
            inner: self.inner.clone(),
            _t: PhantomData,
        }
    }
}

pin_project! {
    #[doc(hidden)]
    pub struct StreamLog<B> {
        #[pin]
        body: B,
        size: usize,
        clock: Instant,
        inner: Arc<PrometheusMetrics>,
        status: StatusCode,
        // a route pattern with some params not-filled and some params filled in by user-defined
        mixed_pattern: String,
        fallback_pattern: String,
        method: Method,
        was_path_matched: bool
    }


    impl<B> PinnedDrop for StreamLog<B> {
        fn drop(this: Pin<&mut Self>) {
            // update the metrics for this request at the very end of responding
            this.inner
                .update_metrics(MetricsUpdate {
                    size: this.size,
                    mixed_pattern: &this.mixed_pattern,
                    fallback_pattern: &this.fallback_pattern,
                    method: &this.method,
                    status: this.status,
                    clock: this.clock,
                    was_path_matched: this.was_path_matched,
                });
        }
    }
}

impl<B: MessageBody> MessageBody for StreamLog<B> {
    type Error = B::Error;

    fn size(&self) -> BodySize {
        self.body.size()
    }

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, Self::Error>>> {
        let this = self.project();
        match ready!(this.body.poll_next(cx)) {
            Some(Ok(chunk)) => {
                *this.size += chunk.len();
                Poll::Ready(Some(Ok(chunk)))
            }
            Some(Err(err)) => Poll::Ready(Some(Err(err))),
            None => Poll::Ready(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::dev::Service;
    use actix_web::test::{TestRequest, call_and_read_body, call_service, init_service, read_body};
    use actix_web::{App, HttpMessage, HttpResponse, Resource, Scope, web};

    use prometheus::{Counter, Opts};

    #[actix_web::test]
    async fn middleware_basic() {
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/metrics")
            .build()
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus)
                .service(web::resource("/health_check").to(HttpResponse::Ok)),
        )
        .await;

        let res = call_service(&app, TestRequest::with_uri("/health_check").to_request()).await;
        assert!(res.status().is_success());
        assert_eq!(read_body(res).await, "");

        let res = call_service(&app, TestRequest::with_uri("/metrics").to_request()).await;
        assert_eq!(
            res.headers().get(CONTENT_TYPE).unwrap(),
            "text/plain; version=0.0.4; charset=utf-8"
        );
        let body = String::from_utf8(read_body(res).await.to_vec()).unwrap();
        println!("{body}");

        assert!(&body.contains(
            &String::from_utf8(web::Bytes::from(
                "# HELP actix_web_prom_http_requests_duration_seconds HTTP request duration in seconds for all requests
# TYPE actix_web_prom_http_requests_duration_seconds histogram
actix_web_prom_http_requests_duration_seconds_bucket{endpoint=\"/health_check\",method=\"GET\",status=\"200\",le=\"0.005\"} 1
"
        ).to_vec()).unwrap()));
        assert!(
            body.contains(
                &String::from_utf8(
                    web::Bytes::from(
                        "# HELP actix_web_prom_http_requests_total Total number of HTTP requests
# TYPE actix_web_prom_http_requests_total counter
actix_web_prom_http_requests_total{endpoint=\"/health_check\",method=\"GET\",status=\"200\"} 1
"
                    )
                    .to_vec()
                )
                .unwrap()
            )
        );
    }

    #[actix_web::test]
    async fn middleware_scope() {
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/internal/metrics")
            .build()
            .unwrap();

        let app = init_service(
            App::new().service(
                web::scope("/internal")
                    .wrap(prometheus)
                    .service(web::resource("/health_check").to(HttpResponse::Ok)),
            ),
        )
        .await;

        let res = call_service(
            &app,
            TestRequest::with_uri("/internal/health_check").to_request(),
        )
        .await;
        assert!(res.status().is_success());
        assert_eq!(read_body(res).await, "");

        let res = call_service(
            &app,
            TestRequest::with_uri("/internal/metrics").to_request(),
        )
        .await;
        assert_eq!(
            res.headers().get(CONTENT_TYPE).unwrap(),
            "text/plain; version=0.0.4; charset=utf-8"
        );
        let body = String::from_utf8(read_body(res).await.to_vec()).unwrap();
        assert!(&body.contains(
            &String::from_utf8(web::Bytes::from(
                "# HELP actix_web_prom_http_requests_duration_seconds HTTP request duration in seconds for all requests
# TYPE actix_web_prom_http_requests_duration_seconds histogram
actix_web_prom_http_requests_duration_seconds_bucket{endpoint=\"/internal/health_check\",method=\"GET\",status=\"200\",le=\"0.005\"} 1
"
        ).to_vec()).unwrap()));
        assert!(body.contains(
            &String::from_utf8(
                web::Bytes::from(
                    "# HELP actix_web_prom_http_requests_total Total number of HTTP requests
# TYPE actix_web_prom_http_requests_total counter
actix_web_prom_http_requests_total{endpoint=\"/internal/health_check\",method=\"GET\",status=\"200\"} 1
"
                )
                .to_vec()
            )
            .unwrap()
        ));
    }

    #[actix_web::test]
    async fn middleware_match_pattern() {
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/metrics")
            .build()
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus)
                .service(web::resource("/resource/{id}").to(HttpResponse::Ok)),
        )
        .await;

        let res = call_service(&app, TestRequest::with_uri("/resource/123").to_request()).await;
        assert!(res.status().is_success());
        assert_eq!(read_body(res).await, "");

        let res = call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let body = String::from_utf8(res.to_vec()).unwrap();
        assert!(&body.contains(
            &String::from_utf8(web::Bytes::from(
                "# HELP actix_web_prom_http_requests_duration_seconds HTTP request duration in seconds for all requests
# TYPE actix_web_prom_http_requests_duration_seconds histogram
actix_web_prom_http_requests_duration_seconds_bucket{endpoint=\"/resource/{id}\",method=\"GET\",status=\"200\",le=\"0.005\"} 1
"
        ).to_vec()).unwrap()));
        assert!(
            body.contains(
                &String::from_utf8(
                    web::Bytes::from(
                        "# HELP actix_web_prom_http_requests_total Total number of HTTP requests
# TYPE actix_web_prom_http_requests_total counter
actix_web_prom_http_requests_total{endpoint=\"/resource/{id}\",method=\"GET\",status=\"200\"} 1
"
                    )
                    .to_vec()
                )
                .unwrap()
            )
        );
    }

    #[actix_web::test]
    async fn middleware_with_mask_unmatched_pattern() {
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/metrics")
            .mask_unmatched_patterns("UNKNOWN")
            .build()
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus)
                .service(web::resource("/resource/{id}").to(HttpResponse::Ok)),
        )
        .await;

        let res = call_service(&app, TestRequest::with_uri("/not-real").to_request()).await;
        assert!(res.status().is_client_error());
        assert_eq!(read_body(res).await, "");

        let res = call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let body = String::from_utf8(res.to_vec()).unwrap();
        assert!(&body.contains(
            &String::from_utf8(web::Bytes::from(
                "actix_web_prom_http_requests_duration_seconds_bucket{endpoint=\"UNKNOWN\",method=\"GET\",status=\"404\",le=\"0.005\"} 1"
        ).to_vec()).unwrap()));
        assert!(body.contains(
            &String::from_utf8(
                web::Bytes::from(
                    "actix_web_prom_http_requests_total{endpoint=\"UNKNOWN\",method=\"GET\",status=\"404\"} 1"
                )
                .to_vec()
            )
            .unwrap()
        ));
    }

    #[actix_web::test]
    async fn middleware_with_mixed_params_cardinality() {
        // we want to keep metrics label on the "cheap param" but not on the "expensive" param
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/metrics")
            .build()
            .unwrap();

        let app = init_service(
            App::new().wrap(prometheus).service(
                web::resource("/resource/{cheap}/{expensive}")
                    .wrap_fn(|req, srv| {
                        req.extensions_mut().insert::<MetricsConfig>(MetricsConfig {
                            cardinality_keep_params: vec!["cheap".to_string()],
                        });
                        srv.call(req)
                    })
                    .to(|path: web::Path<(String, String)>| async {
                        let (cheap, _expensive) = path.into_inner();
                        if !["foo", "bar"].map(|x| x.to_string()).contains(&cheap) {
                            return HttpResponse::NotFound().finish();
                        }
                        HttpResponse::Ok().finish()
                    }),
            ),
        )
        .await;

        // first probe to check basic facts
        let res = call_service(
            &app,
            TestRequest::with_uri("/resource/foo/12345").to_request(),
        )
        .await;
        assert!(res.status().is_success());
        assert_eq!(read_body(res).await, "");

        let res = call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let body = String::from_utf8(res.to_vec()).unwrap();
        println!("Body: {}", body);
        assert!(&body.contains(
            &String::from_utf8(web::Bytes::from(
                "actix_web_prom_http_requests_duration_seconds_bucket{endpoint=\"/resource/foo/{expensive}\",method=\"GET\",status=\"200\",le=\"0.005\"} 1"
        ).to_vec()).unwrap()));
        assert!(body.contains(
            &String::from_utf8(
                web::Bytes::from(
                    "actix_web_prom_http_requests_total{endpoint=\"/resource/foo/{expensive}\",method=\"GET\",status=\"200\"} 1"
                )
                .to_vec()
            )
            .unwrap()
        ));

        // second probe to test 404 behavior
        let res = call_service(
            &app,
            TestRequest::with_uri("/resource/invalid/92945").to_request(),
        )
        .await;
        assert!(res.status() == 404);
        assert_eq!(read_body(res).await, "");

        let res = call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let body = String::from_utf8(res.to_vec()).unwrap();
        println!("Body: {}", body);
        assert!(body.contains(
            &String::from_utf8(
                web::Bytes::from(
                    "actix_web_prom_http_requests_total{endpoint=\"/resource/{cheap}/{expensive}\",method=\"GET\",status=\"404\"} 1"
                )
                .to_vec()
            )
            .unwrap()
        ));
    }

    #[actix_web::test]
    async fn middleware_metrics_exposed_with_conflicting_pattern() {
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/metrics")
            .build()
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus)
                .service(web::resource("/{path}").to(HttpResponse::Ok)),
        )
        .await;

        let res = call_service(&app, TestRequest::with_uri("/something").to_request()).await;
        assert!(res.status().is_success());
        assert_eq!(read_body(res).await, "");

        let res = call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let body = String::from_utf8(res.to_vec()).unwrap();
        assert!(&body.contains(
            &String::from_utf8(web::Bytes::from(
                "# HELP actix_web_prom_http_requests_duration_seconds HTTP request duration in seconds for all requests"
        ).to_vec()).unwrap()));
    }

    #[actix_web::test]
    async fn middleware_basic_failure() {
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/prometheus")
            .build()
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus)
                .service(web::resource("/health_check").to(HttpResponse::Ok)),
        )
        .await;

        call_service(&app, TestRequest::with_uri("/health_checkz").to_request()).await;
        let res = call_and_read_body(&app, TestRequest::with_uri("/prometheus").to_request()).await;
        assert!(
            String::from_utf8(res.to_vec()).unwrap().contains(
                &String::from_utf8(
                    web::Bytes::from(
                        "# HELP actix_web_prom_http_requests_total Total number of HTTP requests
# TYPE actix_web_prom_http_requests_total counter
actix_web_prom_http_requests_total{endpoint=\"/health_checkz\",method=\"GET\",status=\"404\"} 1
"
                    )
                    .to_vec()
                )
                .unwrap()
            )
        );
    }

    #[actix_web::test]
    async fn middleware_custom_counter() {
        let counter_opts = Opts::new("counter", "some random counter").namespace("actix_web_prom");
        let counter = IntCounterVec::new(counter_opts, &["endpoint", "method", "status"]).unwrap();

        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/metrics")
            .build()
            .unwrap();

        prometheus
            .registry
            .register(Box::new(counter.clone()))
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus)
                .service(web::resource("/health_check").to(HttpResponse::Ok)),
        )
        .await;

        // Verify that 'counter' does not appear in the output before we use it
        call_service(&app, TestRequest::with_uri("/health_check").to_request()).await;
        let res = call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        assert!(
            !String::from_utf8(res.to_vec()).unwrap().contains(
                &String::from_utf8(
                    web::Bytes::from(
                        "# HELP actix_web_prom_counter some random counter
# TYPE actix_web_prom_counter counter
actix_web_prom_counter{endpoint=\"endpoint\",method=\"method\",status=\"status\"} 1
"
                    )
                    .to_vec()
                )
                .unwrap()
            )
        );

        // Verify that 'counter' appears after we use it
        counter
            .with_label_values(&["endpoint", "method", "status"])
            .inc();
        counter
            .with_label_values(&["endpoint", "method", "status"])
            .inc();
        call_service(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let res = call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        assert!(
            String::from_utf8(res.to_vec()).unwrap().contains(
                &String::from_utf8(
                    web::Bytes::from(
                        "# HELP actix_web_prom_counter some random counter
# TYPE actix_web_prom_counter counter
actix_web_prom_counter{endpoint=\"endpoint\",method=\"method\",status=\"status\"} 2
"
                    )
                    .to_vec()
                )
                .unwrap()
            )
        );
    }

    #[actix_web::test]
    async fn middleware_none_endpoint() {
        // Init PrometheusMetrics with none URL
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .build()
            .unwrap();

        let app = init_service(App::new().wrap(prometheus.clone()).service(
            web::resource("/metrics").to(|| async { HttpResponse::Ok().body("not prometheus") }),
        ))
        .await;

        let response =
            call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;

        // Assert app works
        assert_eq!(
            String::from_utf8(response.to_vec()).unwrap(),
            "not prometheus"
        );

        // Assert counter counts
        let mut buffer = Vec::new();
        let encoder = TextEncoder::new();
        let metric_families = prometheus.registry.gather();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(
            "actix_web_prom_http_requests_total{endpoint=\"/metrics\",method=\"GET\",status=\"200\"} 1"
        ));
    }

    #[actix_web::test]
    async fn middleware_custom_registry_works() {
        // Init Prometheus Registry
        let registry = Registry::new();

        let counter_opts = Opts::new("test_counter", "test counter help");
        let counter = Counter::with_opts(counter_opts).unwrap();
        registry.register(Box::new(counter.clone())).unwrap();

        counter.inc_by(10_f64);

        // Init PrometheusMetrics
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .registry(registry)
            .endpoint("/metrics")
            .build()
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus.clone())
                .service(web::resource("/test").to(|| async { HttpResponse::Ok().finish() })),
        )
        .await;

        // all http counters are 0 because this is the first http request,
        // so we should get only 10 on test counter
        let response =
            call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let body = String::from_utf8(response.to_vec()).unwrap();

        let ten_test_counter =
            "# HELP test_counter test counter help\n# TYPE test_counter counter\ntest_counter 10\n";
        assert!(body.contains(ten_test_counter));

        // all http counters are 1 because this is the second http request,
        // plus 10 on test counter
        let response =
            call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let response_string = String::from_utf8(response.to_vec()).unwrap();

        let one_http_counters = "# HELP actix_web_prom_http_requests_total Total number of HTTP requests\n# TYPE actix_web_prom_http_requests_total counter\nactix_web_prom_http_requests_total{endpoint=\"/metrics\",method=\"GET\",status=\"200\"} 1";

        assert!(response_string.contains(ten_test_counter));
        assert!(response_string.contains(one_http_counters));
    }

    #[actix_web::test]
    async fn middleware_const_labels() {
        let mut labels = HashMap::new();
        labels.insert("label1".to_string(), "value1".to_string());
        labels.insert("label2".to_string(), "value2".to_string());
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/metrics")
            .const_labels(labels)
            .build()
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus)
                .service(web::resource("/health_check").to(HttpResponse::Ok)),
        )
        .await;

        let res = call_service(&app, TestRequest::with_uri("/health_check").to_request()).await;
        assert!(res.status().is_success());
        assert_eq!(read_body(res).await, "");

        let res = call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let body = String::from_utf8(res.to_vec()).unwrap();
        assert!(&body.contains(
            &String::from_utf8(web::Bytes::from(
                "# HELP actix_web_prom_http_requests_duration_seconds HTTP request duration in seconds for all requests
# TYPE actix_web_prom_http_requests_duration_seconds histogram
actix_web_prom_http_requests_duration_seconds_bucket{endpoint=\"/health_check\",label1=\"value1\",label2=\"value2\",method=\"GET\",status=\"200\",le=\"0.005\"} 1
"
        ).to_vec()).unwrap()));
        assert!(body.contains(
            &String::from_utf8(
                web::Bytes::from(
                    "# HELP actix_web_prom_http_requests_total Total number of HTTP requests
# TYPE actix_web_prom_http_requests_total counter
actix_web_prom_http_requests_total{endpoint=\"/health_check\",label1=\"value1\",label2=\"value2\",method=\"GET\",status=\"200\"} 1
"
                )
                .to_vec()
            )
            .unwrap()
        ));
    }

    #[actix_web::test]
    async fn middleware_metrics_configuration() {
        let metrics_config = ActixMetricsConfiguration::default()
            .http_requests_duration_seconds_name("my_http_request_duration")
            .http_requests_total_name("my_http_requests_total");

        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/metrics")
            .metrics_configuration(metrics_config)
            .build()
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus)
                .service(web::resource("/health_check").to(HttpResponse::Ok)),
        )
        .await;

        let res = call_service(&app, TestRequest::with_uri("/health_check").to_request()).await;
        assert!(res.status().is_success());
        assert_eq!(read_body(res).await, "");

        let res = call_and_read_body(&app, TestRequest::with_uri("/metrics").to_request()).await;
        let body = String::from_utf8(res.to_vec()).unwrap();
        assert!(&body.contains(
            &String::from_utf8(web::Bytes::from(
                "# HELP actix_web_prom_my_http_request_duration HTTP request duration in seconds for all requests
# TYPE actix_web_prom_my_http_request_duration histogram
actix_web_prom_my_http_request_duration_bucket{endpoint=\"/health_check\",method=\"GET\",status=\"200\",le=\"0.005\"} 1
"
        ).to_vec()).unwrap()));
        assert!(
            body.contains(
                &String::from_utf8(
                    web::Bytes::from(
                        "# HELP actix_web_prom_my_http_requests_total Total number of HTTP requests
# TYPE actix_web_prom_my_http_requests_total counter
actix_web_prom_my_http_requests_total{endpoint=\"/health_check\",method=\"GET\",status=\"200\"} 1
"
                    )
                    .to_vec()
                )
                .unwrap()
            )
        );
    }

    #[test]
    fn compat_with_non_boxed_middleware() {
        let _app = App::new()
            .wrap(PrometheusMetricsBuilder::new("").build().unwrap())
            .wrap(actix_web::middleware::Logger::default())
            .route("", web::to(|| async { "" }));

        let _app = App::new()
            .wrap(actix_web::middleware::Logger::default())
            .wrap(PrometheusMetricsBuilder::new("").build().unwrap())
            .route("", web::to(|| async { "" }));

        let _scope = Scope::new("")
            .wrap(PrometheusMetricsBuilder::new("").build().unwrap())
            .route("", web::to(|| async { "" }));

        let _resource = Resource::new("")
            .wrap(PrometheusMetricsBuilder::new("").build().unwrap())
            .route(web::to(|| async { "" }));
    }

    #[actix_web::test]
    async fn middleware_excludes() {
        let prometheus = PrometheusMetricsBuilder::new("actix_web_prom")
            .endpoint("/metrics")
            .exclude("/ping")
            .exclude_status(StatusCode::NOT_FOUND)
            .build()
            .unwrap();

        let app = init_service(
            App::new()
                .wrap(prometheus)
                .service(web::resource("/health_check").to(HttpResponse::Ok))
                .service(web::resource("/ping").to(HttpResponse::Ok)),
        )
        .await;

        let res = call_service(&app, TestRequest::with_uri("/health_check").to_request()).await;
        assert!(res.status().is_success());
        assert_eq!(read_body(res).await, "");

        let res = call_service(&app, TestRequest::with_uri("/ping").to_request()).await;
        assert!(res.status().is_success());
        assert_eq!(read_body(res).await, "");

        let res = call_service(&app, TestRequest::with_uri("/notfound").to_request()).await;
        assert!(res.status().is_client_error());
        assert_eq!(read_body(res).await, "");

        let res = call_service(&app, TestRequest::with_uri("/metrics").to_request()).await;
        assert_eq!(
            res.headers().get(CONTENT_TYPE).unwrap(),
            "text/plain; version=0.0.4; charset=utf-8"
        );
        let body = String::from_utf8(read_body(res).await.to_vec()).unwrap();
        assert!(
            &body.contains(
                &String::from_utf8(
                    web::Bytes::from(
                        "# HELP actix_web_prom_http_requests_total Total number of HTTP requests
# TYPE actix_web_prom_http_requests_total counter
actix_web_prom_http_requests_total{endpoint=\"/health_check\",method=\"GET\",status=\"200\"} 1
"
                    )
                    .to_vec()
                )
                .unwrap()
            )
        );

        assert!(!&body.contains("endpoint=\"/ping\""));
        assert!(!body.contains("endpoint=\"/notfound"));
    }
}
