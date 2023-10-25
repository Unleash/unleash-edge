use actix_http::header::CONTENT_LENGTH;
use actix_web::dev;
use actix_web::dev::ServiceRequest;
use actix_web::http::{header, Method, StatusCode, Version};
use futures::{future, FutureExt};
use futures_core::future::LocalBoxFuture;
use opentelemetry::metrics::{Histogram, Meter, MeterProvider, MetricsError, Unit, UpDownCounter};
use opentelemetry::trace::OrderMap;
use opentelemetry::{global, Key, KeyValue, Value};
use opentelemetry_semantic_conventions::trace::{
    CLIENT_ADDRESS, CLIENT_SOCKET_ADDRESS, HTTP_REQUEST_METHOD, HTTP_RESPONSE_STATUS_CODE,
    HTTP_ROUTE, NETWORK_PROTOCOL_NAME, NETWORK_PROTOCOL_VERSION, SERVER_ADDRESS, SERVER_PORT,
    URL_PATH, URL_SCHEME, USER_AGENT_ORIGINAL,
};
use prometheus::{Encoder, TextEncoder};
use std::sync::Arc;
use std::time::SystemTime;

use crate::metrics::route_formatter::RouteFormatter;
const HTTP_SERVER_ACTIVE_REQUESTS: &str = "http.server.active_requests";
const HTTP_SERVER_DURATION: &str = "http.server.duration";
const HTTP_SERVER_REQUEST_SIZE: &str = "http.server.request.size";
const HTTP_SERVER_RESPONSE_SIZE: &str = "http.server.response.size";

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
pub(super) fn http_version(version: Version) -> Value {
    match version {
        Version::HTTP_09 => "0.9".into(),
        Version::HTTP_10 => "1.0".into(),
        Version::HTTP_11 => "1.1".into(),
        Version::HTTP_2 => "2.0".into(),
        Version::HTTP_3 => "3.0".into(),
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
    attributes.insert(HTTP_REQUEST_METHOD, http_method_str(req.method()));
    attributes.insert(NETWORK_PROTOCOL_NAME, "http".into());
    attributes.insert(NETWORK_PROTOCOL_VERSION, http_version(req.version()));
    attributes.insert(CLIENT_ADDRESS, conn_info.host().to_string().into());
    attributes.insert(HTTP_ROUTE, http_route.to_owned().into());
    attributes.insert(URL_SCHEME, http_scheme(conn_info.scheme()));

    let server_name = req.app_config().host();
    if server_name != conn_info.host() {
        attributes.insert(SERVER_ADDRESS, server_name.to_string().into());
    }
    if let Some(port) = conn_info
        .host()
        .split_terminator(':')
        .nth(1)
        .and_then(|port| port.parse::<i64>().ok())
    {
        if port != 80 && port != 443 {
            attributes.insert(SERVER_PORT, port.into());
        }
    }
    if let Some(path) = req.uri().path_and_query() {
        attributes.insert(URL_PATH, path.as_str().to_string().into());
    }
    if let Some(user_agent) = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|s| s.to_str().ok())
    {
        attributes.insert(USER_AGENT_ORIGINAL, user_agent.to_string().into());
    }
    let remote_addr = conn_info.realip_remote_addr();
    if let Some(remote) = remote_addr {
        attributes.insert(CLIENT_ADDRESS, remote.to_string().into());
    }
    if let Some(peer_addr) = req.peer_addr().map(|socket| socket.ip().to_string()) {
        if Some(peer_addr.as_str()) != remote_addr {
            // Client is going through a proxy
            attributes.insert(CLIENT_SOCKET_ADDRESS, peer_addr.into());
        }
    }

    attributes
}

pub(super) fn metrics_attributes_from_request(
    req: &ServiceRequest,
    http_target: &str,
) -> Vec<KeyValue> {
    use opentelemetry_semantic_conventions::trace::SERVER_SOCKET_ADDRESS;

    let conn_info = req.connection_info();

    let mut attributes = Vec::with_capacity(11);
    attributes.push(KeyValue::new(
        HTTP_REQUEST_METHOD,
        http_method_str(req.method()),
    ));
    attributes.push(KeyValue::new(NETWORK_PROTOCOL_NAME, "http"));
    attributes.push(KeyValue::new(
        NETWORK_PROTOCOL_VERSION,
        http_version(req.version()),
    ));
    attributes.push(SERVER_SOCKET_ADDRESS.string(conn_info.host().to_string()));
    attributes.push(URL_PATH.string(http_target.to_owned()));
    attributes.push(KeyValue::new(URL_SCHEME, http_scheme(conn_info.scheme())));

    let server_name = req.app_config().host();
    if server_name != conn_info.host() {
        attributes.push(SERVER_ADDRESS.string(server_name.to_string()));
    }
    if let Some(port) = conn_info
        .host()
        .split_terminator(':')
        .nth(1)
        .and_then(|port| port.parse().ok())
    {
        attributes.push(SERVER_PORT.i64(port))
    }

    let remote_addr = conn_info.realip_remote_addr();
    if let Some(peer_addr) = req.peer_addr().map(|socket| socket.ip().to_string()) {
        if Some(peer_addr.as_str()) != remote_addr {
            // Client is going through a proxy
            attributes.push(CLIENT_SOCKET_ADDRESS.string(peer_addr))
        }
    }

    attributes
}

#[derive(Clone, Debug)]
struct Metrics {
    http_server_active_requests: UpDownCounter<i64>,
    http_server_duration: Histogram<f64>,
    http_server_request_size: Histogram<u64>,
    http_server_response_size: Histogram<u64>,
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

        let http_server_request_size = meter
            .u64_histogram(HTTP_SERVER_REQUEST_SIZE)
            .with_description("Measures the size of HTTP request messages (compressed).")
            .with_unit(Unit::new("By"))
            .init();

        let http_server_response_size = meter
            .u64_histogram(HTTP_SERVER_RESPONSE_SIZE)
            .with_description("Measures the size of HTTP request messages (compressed).")
            .with_unit(Unit::new("By"))
            .init();

        Metrics {
            http_server_active_requests,
            http_server_duration,
            http_server_request_size,
            http_server_response_size,
        }
    }
}

/// Builder for [RequestMetrics]
#[derive(Clone, Debug, Default)]
pub struct RequestMetricsBuilder {
    route_formatter: Option<Arc<dyn RouteFormatter + Send + Sync + 'static>>,
    meter: Option<Meter>,
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

    /// Set the meter provider this middleware should use to construct meters
    pub fn with_meter_provider(mut self, meter_provider: impl MeterProvider) -> Self {
        self.meter = Some(get_versioned_meter(meter_provider));
        self
    }

    /// Build the `RequestMetrics` middleware
    pub fn build(self) -> RequestMetrics {
        let meter = self
            .meter
            .unwrap_or_else(|| get_versioned_meter(global::meter_provider()));

        RequestMetrics {
            route_formatter: self.route_formatter,
            metrics: Arc::new(Metrics::new(meter)),
        }
    }
}

/// construct meters for this crate
fn get_versioned_meter(meter_provider: impl MeterProvider) -> Meter {
    meter_provider.versioned_meter(
        "actix_web_opentelemetry",
        Some(env!("CARGO_PKG_VERSION")),
        Some(opentelemetry_semantic_conventions::SCHEMA_URL),
        None,
    )
}

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

        let mut http_target = req
            .match_pattern()
            .map(std::borrow::Cow::Owned)
            .unwrap_or(std::borrow::Cow::Borrowed("default"));

        if let Some(formatter) = &self.route_formatter {
            http_target = std::borrow::Cow::Owned(formatter.format(&http_target));
        }

        let mut attributes = metrics_attributes_from_request(&req, &http_target);
        self.metrics.http_server_active_requests.add(1, &attributes);

        let content_length = req
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|len| len.to_str().ok().and_then(|s| s.parse().ok()))
            .unwrap_or(0);
        self.metrics
            .http_server_request_size
            .record(content_length, &attributes);

        let request_metrics = self.metrics.clone();
        Box::pin(self.service.call(req).map(move |res| {
            request_metrics
                .http_server_active_requests
                .add(-1, &attributes);

            // Ignore actix errors for metrics
            if let Ok(res) = res {
                attributes.push(HTTP_RESPONSE_STATUS_CODE.i64(res.status().as_u16() as i64));
                let response_size = res
                    .response()
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|len| len.to_str().ok().and_then(|s| s.parse().ok()))
                    .unwrap_or(0);
                request_metrics
                    .http_server_response_size
                    .record(response_size, &attributes);

                request_metrics.http_server_duration.record(
                    timer.elapsed().map(|t| t.as_secs_f64()).unwrap_or_default(),
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
    registry: prometheus::Registry,
}

impl PrometheusMetricsHandler {
    /// Build a route to serve Prometheus metrics
    pub fn new(registry: prometheus::Registry) -> Self {
        Self { registry }
    }
}

impl PrometheusMetricsHandler {
    fn metrics(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
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
