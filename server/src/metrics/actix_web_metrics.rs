use actix_http::header::CONTENT_LENGTH;
use actix_web::dev;
use actix_web::dev::ServiceRequest;
use actix_web::http::{Method, StatusCode, Version};
use actix_web::web::Data;
use futures::{future, FutureExt};
use futures_core::future::LocalBoxFuture;
use opentelemetry::metrics::{Histogram, Meter, MeterProvider, UpDownCounter};
use opentelemetry::{InstrumentationScope, KeyValue, Value};
use opentelemetry_semantic_conventions::trace::{
    CLIENT_ADDRESS, HTTP_REQUEST_METHOD, HTTP_RESPONSE_STATUS_CODE, NETWORK_PROTOCOL_NAME,
    NETWORK_PROTOCOL_VERSION, SERVER_ADDRESS, SERVER_PORT, URL_PATH, URL_SCHEME,
};
use prometheus::{Encoder, TextEncoder};
use std::sync::Arc;
use std::time::SystemTime;

use crate::metrics::route_formatter::RouteFormatter;

use super::edge_metrics::EdgeInstanceData;
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

pub(super) fn metrics_attributes_from_request(
    req: &ServiceRequest,
    http_target: &str,
) -> Vec<KeyValue> {
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
    attributes.push(KeyValue::new(SERVER_ADDRESS, conn_info.host().to_string()));
    attributes.push(KeyValue::new(URL_PATH, http_target.to_owned()));
    attributes.push(KeyValue::new(URL_SCHEME, http_scheme(conn_info.scheme())));

    let server_name = req.app_config().host();
    if server_name != conn_info.host() {
        attributes.push(KeyValue::new(SERVER_ADDRESS, server_name.to_string()));
    }
    if let Some(port) = conn_info
        .host()
        .split_terminator(':')
        .nth(1)
        .and_then(|port| port.parse::<i64>().ok())
    {
        attributes.push(KeyValue::new(SERVER_PORT, port))
    }

    let remote_addr = conn_info.realip_remote_addr();
    if let Some(peer_addr) = req.peer_addr().map(|socket| socket.ip().to_string()) {
        if Some(peer_addr.as_str()) != remote_addr {
            // Client is going through a proxy
            attributes.push(KeyValue::new(CLIENT_ADDRESS, peer_addr))
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
            .build();

        let http_server_duration = meter
            .f64_histogram(HTTP_SERVER_DURATION)
            .with_description("HTTP inbound request duration per route")
            .with_unit("ms")
            .with_boundaries(vec![
                1.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 100.0, 200.0, 300.0, 400.0, 500.0, 750.0,
                1000.0, 1500.0, 2000.0,
            ])
            .build();

        let http_server_request_size = meter
            .u64_histogram(HTTP_SERVER_REQUEST_SIZE)
            .with_description("Measures the size of HTTP request messages (compressed).")
            .with_unit("By")
            .build();

        let http_server_response_size = meter
            .u64_histogram(HTTP_SERVER_RESPONSE_SIZE)
            .with_description("Measures the size of HTTP request messages (compressed).")
            .with_unit("By")
            .build();

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
        let meter = self.meter.unwrap();

        RequestMetrics {
            route_formatter: self.route_formatter,
            metrics: Arc::new(Metrics::new(meter)),
        }
    }
}

/// construct meters for this crate
fn get_versioned_meter(meter_provider: impl MeterProvider) -> Meter {
    meter_provider.meter_with_scope(
        InstrumentationScope::builder("unleash_edge")
            .with_version(env!("CARGO_PKG_VERSION"))
            .with_schema_url(opentelemetry_semantic_conventions::SCHEMA_URL)
            .build(),
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
        let edge_instance_data = req.app_data::<Data<EdgeInstanceData>>().cloned();
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

            let status_code = match &res {
                Ok(res) => res.status(),
                Err(e) => e.as_response_error().status_code(),
            }
            .as_u16() as i64;

            attributes.push(KeyValue::new(HTTP_RESPONSE_STATUS_CODE, status_code));

            let response_size = res
                .as_ref()
                .map(|res| {
                    res.response()
                        .headers()
                        .get(CONTENT_LENGTH)
                        .and_then(|len| len.to_str().ok().and_then(|s| s.parse().ok()))
                        .unwrap_or(0u64)
                })
                .unwrap_or(0);
            request_metrics
                .http_server_response_size
                .record(response_size, &attributes);

            request_metrics.http_server_duration.record(
                timer.elapsed().map(|t| t.as_secs_f64()).unwrap_or_default(),
                &attributes,
            );
            if let Some(instance_data) = edge_instance_data {
                if let Some(endpoint) = super::edge_metrics::DESIRED_URLS
                    .into_iter()
                    .find(|u| http_target.ends_with(u))
                {
                    instance_data.observe_request(endpoint, status_code);
                }
            }

            res
        }))
    }
}

#[derive(Clone, Debug)]
pub struct PrometheusMetricsHandler {
    pub registry: prometheus::Registry,
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
        let _ = encoder.encode(&metric_families[..], &mut buf);
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

#[cfg(test)]
mod tests {
    use crate::prom_metrics;
    use actix_web::{http::StatusCode, test, web, App, HttpResponse};
    use prometheus::{Encoder, Registry, TextEncoder};

    async fn test_ok_endpoint() -> HttpResponse {
        HttpResponse::Ok()
            .append_header(("Content-length", 7))
            .body("Test OK")
    }

    async fn test_client_error_endpoint() -> HttpResponse {
        HttpResponse::BadRequest()
            .append_header(("Content-length", 17))
            .body("Test Client Error")
    }

    async fn test_server_error_endpoint() -> HttpResponse {
        HttpResponse::InternalServerError()
            .append_header(("Content-length", 17))
            .body("Test Server Error")
    }

    fn parse_metrics_for_status_code(metrics_output: &str, status_code: i64) -> Option<f64> {
        metrics_output
            .lines()
            .filter(|line| {
                line.contains("http_server_response_size_bytes_sum")
                    && line.contains(&format!("http_response_status_code=\"{}\"", status_code))
            })
            .flat_map(|line| line.split_whitespace().last())
            .flat_map(|value| value.parse::<f64>().ok())
            .next()
    }

    #[tokio::test]
    async fn test_middleware_response_metrics() {
        let registry = Registry::new();
        let (_, request_metrics) =
            prom_metrics::test_instantiate_without_tracing_and_logging(Some(registry.clone()));

        let app = test::init_service(
            App::new()
                .wrap(request_metrics.clone())
                .service(web::resource("/test_ok").to(test_ok_endpoint))
                .service(web::resource("/test_client_error").to(test_client_error_endpoint))
                .service(web::resource("/test_server_error").to(test_server_error_endpoint)),
        )
        .await;

        let req_ok = test::TestRequest::get().uri("/test_ok").to_request();
        let resp_ok = test::call_service(&app, req_ok).await;
        assert_eq!(resp_ok.status(), StatusCode::OK);

        let req_client_error = test::TestRequest::get()
            .uri("/test_client_error")
            .to_request();
        let resp_client_error = test::call_service(&app, req_client_error).await;
        assert_eq!(resp_client_error.status(), StatusCode::BAD_REQUEST);

        let req_server_error = test::TestRequest::get()
            .uri("/test_server_error")
            .to_request();
        let resp_server_error = test::call_service(&app, req_server_error).await;
        assert_eq!(
            resp_server_error.status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );

        let mut buffer = Vec::new();
        let encoder = TextEncoder::new();
        let metric_families = registry.gather();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        let metrics_output = String::from_utf8(buffer).unwrap();

        let value_ok = parse_metrics_for_status_code(&metrics_output, 200)
            .expect("Metric with status code 200 not found");
        assert_eq!(
            value_ok, 7.0,
            "Metric value for status code 200 did not match expected"
        );

        let value_client_error = parse_metrics_for_status_code(&metrics_output, 400)
            .expect("Metric with status code 400 not found");
        assert_eq!(
            value_client_error, 17.0,
            "Metric value for status code 400 did not match expected"
        );

        let value_server_error = parse_metrics_for_status_code(&metrics_output, 500)
            .expect("Metric with status code 500 not found");
        assert_eq!(
            value_server_error, 17.0,
            "Metric value for status code 500 did not match expected"
        );
    }
}
