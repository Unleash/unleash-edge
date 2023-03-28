use actix_web::dev;
use actix_web::dev::ServiceRequest;
use actix_web::http::{header, Method, StatusCode, Version};
use futures::{future, FutureExt};
use futures_core::future::LocalBoxFuture;
use opentelemetry::metrics::{Histogram, Meter, MetricsError, Unit, UpDownCounter};
use opentelemetry::trace::OrderMap;
use opentelemetry::{global, Context, Key, KeyValue, Value};
use opentelemetry_prometheus::PrometheusExporter;
use opentelemetry_semantic_conventions::trace::{
    HTTP_CLIENT_IP, HTTP_FLAVOR, HTTP_METHOD, HTTP_ROUTE, HTTP_SCHEME, HTTP_STATUS_CODE,
    HTTP_TARGET, HTTP_USER_AGENT, NET_HOST_PORT, NET_PEER_NAME, NET_SOCK_PEER_ADDR,
};
use prometheus::{Encoder, TextEncoder};
use std::sync::Arc;
use std::time::SystemTime;

use crate::metrics::route_formatter::RouteFormatter;
const HTTP_SERVER_ACTIVE_REQUESTS: &str = "http.server.active_requests";
const HTTP_SERVER_DURATION: &str = "http.server.duration";

#[inline]
pub(super) fn http_method_str(method: &Method) -> Value {
    match method {
        &Method::OPTIONS => "OPTIONS".into(),
        &Method::GET => "GET".into(),
        &Method::POST => "POST".into(),
        &Method::PUT => "PUT".into(),
        &Method::DELETE => "DELETE".into(),
        &Method::HEAD => "HEAD".into(),
        &Method::TRACE => "TRACE".into(),
        &Method::CONNECT => "CONNECT".into(),
        &Method::PATCH => "PATCH".into(),
        other => other.to_string().into(),
    }
}

#[inline]
pub(super) fn http_flavor(version: Version) -> Value {
    match version {
        Version::HTTP_09 => "HTTP/0.9".into(),
        Version::HTTP_10 => "HTTP/1.0".into(),
        Version::HTTP_11 => "HTTP/1.1".into(),
        Version::HTTP_2 => "HTTP/2".into(),
        Version::HTTP_3 => "HTTP/3".into(),
        other => format!("{:?}", other).into(),
    }
}

#[inline]
pub(super) fn http_scheme(scheme: &str) -> Value {
    match scheme {
        "http" => "http".into(),
        "https" => "https".into(),
        other => other.to_string().into(),
    }
}

pub(crate) fn trace_attributes_from_request(
    req: &ServiceRequest,
    http_route: &str,
) -> OrderMap<Key, Value> {
    let conn_info = req.connection_info();

    let mut attributes = OrderMap::with_capacity(11);
    attributes.insert(HTTP_METHOD, http_method_str(req.method()));
    attributes.insert(HTTP_FLAVOR, http_flavor(req.version()));
    attributes.insert(NET_PEER_NAME, conn_info.host().to_string().into());
    attributes.insert(HTTP_ROUTE, http_route.to_owned().into());
    attributes.insert(HTTP_SCHEME, http_scheme(conn_info.scheme()));

    let server_name = req.app_config().host();
    if server_name != conn_info.host() {
        attributes.insert(NET_PEER_NAME, server_name.to_string().into());
    }
    if let Some(port) = conn_info
        .host()
        .split_terminator(':')
        .nth(1)
        .and_then(|port| port.parse::<i64>().ok())
    {
        if port != 80 && port != 443 {
            attributes.insert(NET_HOST_PORT, port.into());
        }
    }
    if let Some(path) = req.uri().path_and_query() {
        attributes.insert(HTTP_TARGET, path.as_str().to_string().into());
    }
    if let Some(user_agent) = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|s| s.to_str().ok())
    {
        attributes.insert(HTTP_USER_AGENT, user_agent.to_string().into());
    }
    let remote_addr = conn_info.realip_remote_addr();
    if let Some(remote) = remote_addr {
        attributes.insert(HTTP_CLIENT_IP, remote.to_string().into());
    }
    if let Some(peer_addr) = req.peer_addr().map(|socket| socket.ip().to_string()) {
        if Some(peer_addr.as_str()) != remote_addr {
            // Client is going through a proxy
            attributes.insert(NET_SOCK_PEER_ADDR, peer_addr.into());
        }
    }

    attributes
}

pub(super) fn metrics_attributes_from_request(
    req: &ServiceRequest,
    http_target: &str,
) -> Vec<KeyValue> {
    use opentelemetry_semantic_conventions::trace::NET_SOCK_HOST_ADDR;

    let conn_info = req.connection_info();

    let mut attributes = Vec::with_capacity(11);
    attributes.push(KeyValue::new(HTTP_METHOD, http_method_str(req.method())));
    attributes.push(KeyValue::new(HTTP_FLAVOR, http_flavor(req.version())));
    attributes.push(NET_SOCK_HOST_ADDR.string(conn_info.host().to_string()));
    attributes.push(HTTP_TARGET.string(http_target.to_owned()));
    attributes.push(KeyValue::new(HTTP_SCHEME, http_scheme(conn_info.scheme())));

    let server_name = req.app_config().host();
    if server_name != conn_info.host() {
        attributes.push(NET_PEER_NAME.string(server_name.to_string()));
    }
    if let Some(port) = conn_info
        .host()
        .split_terminator(':')
        .nth(1)
        .and_then(|port| port.parse().ok())
    {
        attributes.push(NET_HOST_PORT.i64(port))
    }

    let remote_addr = conn_info.realip_remote_addr();
    if let Some(peer_addr) = req.peer_addr().map(|socket| socket.ip().to_string()) {
        if Some(peer_addr.as_str()) != remote_addr {
            // Client is going through a proxy
            attributes.push(NET_SOCK_PEER_ADDR.string(peer_addr))
        }
    }

    attributes
}

#[derive(Clone, Debug)]
struct Metrics {
    http_server_active_requests: UpDownCounter<i64>,
    http_server_duration: Histogram<f64>,
}

impl Metrics {
    /// Create a new [`RequestMetrics`]
    fn new(meter: Meter) -> Self {
        let http_server_active_requests = meter
            .i64_up_down_counter(HTTP_SERVER_ACTIVE_REQUESTS)
            .with_description("HTTP concurrent in-flight requests per route")
            .init();

        let http_server_duration = meter
            .f64_histogram(HTTP_SERVER_DURATION)
            .with_description("HTTP inbound request duration per route")
            .with_unit(Unit::new("ms"))
            .init();

        Metrics {
            http_server_active_requests,
            http_server_duration,
        }
    }
}

/// Builder for [RequestMetrics]
#[derive(Clone, Debug, Default)]
pub struct RequestMetricsBuilder {
    route_formatter: Option<Arc<dyn RouteFormatter + Send + Sync + 'static>>,
}

impl RequestMetricsBuilder {
    /// Create a new `RequestMetricsBuilder`
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a route formatter to customize metrics match patterns
    pub fn with_route_formatter<R>(mut self, route_formatter: R) -> Self
    where
        R: RouteFormatter + Send + Sync + 'static,
    {
        self.route_formatter = Some(Arc::new(route_formatter));
        self
    }

    /// Build the `RequestMetrics` middleware
    pub fn build(self, meter: Meter) -> RequestMetrics {
        RequestMetrics {
            route_formatter: self.route_formatter,
            metrics: Arc::new(Metrics::new(meter)),
        }
    }
}

/// Request metrics tracking
///
/// # Examples
///
/// ```no_run
/// use actix_web::{dev, http, web, App, HttpRequest, HttpServer};
/// use unleash_edge::metrics::actix_web_metrics::{PrometheusMetricsHandler,
///     RequestMetricsBuilder};
/// use unleash_edge::middleware::request_tracing::RequestTracing;
/// use opentelemetry::{
///     global,
///     sdk::{
///         export::metrics::aggregation,
///         metrics::{controllers, processors, selectors},
///         propagation::TraceContextPropagator,
///     },
/// };
///
/// #[actix_web::main]
/// async fn main() -> std::io::Result<()> {
///     // Request metrics middleware
///     let meter = global::meter("actix_web");
///     let request_metrics = RequestMetricsBuilder::new().build(meter);
///
///     // Prometheus request metrics handler
///     let controller = controllers::basic(
///         processors::factory(
///             selectors::simple::histogram([1.0, 2.0, 5.0, 10.0, 20.0, 50.0]),
///             aggregation::cumulative_temporality_selector(),
///         )
///     )
///     .build();
///     let exporter = opentelemetry_prometheus::exporter(controller).init();
///     let metrics_handler = PrometheusMetricsHandler::new(exporter);
///
///     // Run actix server, metrics are now available at http://localhost:8080/metrics
///     HttpServer::new(move || {
///         App::new()
///             .wrap(RequestTracing::new())
///             .wrap(request_metrics.clone())
///             .route("/metrics", web::get().to(metrics_handler.clone()))
///     })
///     .bind("localhost:8080")?
///     .run()
///     .await
/// }
/// ```
#[derive(Clone, Debug)]
pub struct RequestMetrics {
    route_formatter: Option<Arc<dyn RouteFormatter + Send + Sync + 'static>>,
    metrics: Arc<Metrics>,
}

impl<S, B> dev::Transform<S, dev::ServiceRequest> for RequestMetrics
where
    S: dev::Service<
        dev::ServiceRequest,
        Response = dev::ServiceResponse<B>,
        Error = actix_web::Error,
    >,
    S::Future: 'static,
    B: 'static,
{
    type Response = dev::ServiceResponse<B>;
    type Error = actix_web::Error;
    type Transform = RequestMetricsMiddleware<S>;
    type InitError = ();
    type Future = future::Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        let service = RequestMetricsMiddleware {
            service,
            metrics: self.metrics.clone(),
            route_formatter: self.route_formatter.clone(),
        };

        future::ok(service)
    }
}

/// Request metrics middleware
#[allow(missing_debug_implementations)]
pub struct RequestMetricsMiddleware<S> {
    service: S,
    metrics: Arc<Metrics>,
    route_formatter: Option<Arc<dyn RouteFormatter + Send + Sync + 'static>>,
}

impl<S, B> dev::Service<dev::ServiceRequest> for RequestMetricsMiddleware<S>
where
    S: dev::Service<
        dev::ServiceRequest,
        Response = dev::ServiceResponse<B>,
        Error = actix_web::Error,
    >,
    S::Future: 'static,
    B: 'static,
{
    type Response = dev::ServiceResponse<B>;
    type Error = actix_web::Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    dev::forward_ready!(service);

    fn call(&self, req: dev::ServiceRequest) -> Self::Future {
        let timer = SystemTime::now();

        let mut http_target = req.match_pattern().unwrap_or_else(|| "default".to_string());
        if let Some(formatter) = &self.route_formatter {
            http_target = formatter.format(&http_target);
        }

        let mut attributes = metrics_attributes_from_request(&req, &http_target);
        let cx = Context::current();

        self.metrics
            .http_server_active_requests
            .add(&cx, 1, &attributes);

        let request_metrics = self.metrics.clone();
        Box::pin(self.service.call(req).map(move |res| {
            request_metrics
                .http_server_active_requests
                .add(&cx, -1, &attributes);

            // Ignore actix errors for metrics
            if let Ok(res) = res {
                attributes.push(HTTP_STATUS_CODE.string(res.status().as_str().to_owned()));

                request_metrics.http_server_duration.record(
                    &cx,
                    timer
                        .elapsed()
                        .map(|t| t.as_secs_f64() * 1000.0)
                        .unwrap_or_default(),
                    &attributes,
                );

                Ok(res)
            } else {
                res
            }
        }))
    }
}

#[derive(Clone, Debug)]
pub struct PrometheusMetricsHandler {
    prometheus_exporter: PrometheusExporter,
}

impl PrometheusMetricsHandler {
    /// Build a route to serve Prometheus metrics
    pub fn new(exporter: PrometheusExporter) -> Self {
        Self {
            prometheus_exporter: exporter,
        }
    }
}

impl PrometheusMetricsHandler {
    fn metrics(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.prometheus_exporter.registry().gather();
        let mut buf = Vec::new();
        if let Err(err) = encoder.encode(&metric_families[..], &mut buf) {
            global::handle_error(MetricsError::Other(err.to_string()));
        }

        String::from_utf8(buf).unwrap_or_default()
    }
}

impl dev::Handler<actix_web::HttpRequest> for PrometheusMetricsHandler {
    type Output = Result<actix_web::HttpResponse<String>, actix_web::error::Error>;
    type Future = LocalBoxFuture<'static, Self::Output>;

    fn call(&self, _req: actix_web::HttpRequest) -> Self::Future {
        Box::pin(future::ok(actix_web::HttpResponse::with_body(
            StatusCode::OK,
            self.metrics(),
        )))
    }
}
