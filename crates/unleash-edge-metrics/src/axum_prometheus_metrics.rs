use axum::body::HttpBody;
use axum::extract::{MatchedPath, Request};
use axum::response::{IntoResponse, Response};
use prometheus::{
    GaugeVec, HistogramVec, IntCounterVec, Registry, TextEncoder, gather, register_gauge_vec,
    register_histogram_vec, register_int_counter_vec,
};
use reqwest::StatusCode;
use std::pin::Pin;
use std::sync::{LazyLock, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;
use tower::{Layer, Service};
use unleash_edge_types::metrics::HTTP_REQUESTS_DURATION;
use unleash_edge_types::metrics::HTTP_REQUESTS_PENDING;
use unleash_edge_types::metrics::HTTP_REQUESTS_TOTAL;
use unleash_edge_types::metrics::HTTP_RESPONSE_SIZE;
use unleash_types::client_metrics::ConnectVia;

pub const ENDPOINT_LABEL: &str = "endpoint";
pub const METHOD_LABEL: &str = "method";
pub const STATUS_LABEL: &str = "status";
pub const INSTANCE_ID: &str = "instance_id";
pub const APP_NAME: &str = "app_name";

static HTTP_REQUESTS_TOTAL_METRIC: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        HTTP_REQUESTS_TOTAL,
        "Total number of HTTP requests",
        &[
            METHOD_LABEL,
            ENDPOINT_LABEL,
            STATUS_LABEL,
            APP_NAME,
            INSTANCE_ID
        ]
    )
    .unwrap()
});

static HTTP_REQUEST_DURATION_MILLISECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HTTP_REQUESTS_DURATION,
        "HTTP request latencies in milliseconds",
        &[
            METHOD_LABEL,
            ENDPOINT_LABEL,
            STATUS_LABEL,
            APP_NAME,
            INSTANCE_ID
        ],
        vec![
            0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0,
            10000.0
        ]
    )
    .unwrap()
});

static HTTP_RESPONSE_SIZE_METRIC: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HTTP_RESPONSE_SIZE,
        "Size of HTTP response bodies in bytes",
        &[METHOD_LABEL, ENDPOINT_LABEL, APP_NAME, INSTANCE_ID],
        vec![
            5.0, 10.0, 100.0, 250.0, 500.0, 750.0, 1000.0, 2500.0, 5000.0, 10000.0, 25000.0,
            50000.0, 100000.0,
        ]
    )
    .unwrap()
});

static HTTP_REQUESTS_PENDING_METRIC: LazyLock<GaugeVec> = LazyLock::new(|| {
    register_gauge_vec!(
        HTTP_REQUESTS_PENDING,
        "Number of pending HTTP requests",
        &[METHOD_LABEL, ENDPOINT_LABEL, APP_NAME, INSTANCE_ID]
    )
    .unwrap()
});

static EXCLUDED_PATHS: LazyLock<Mutex<Vec<&'static str>>> =
    LazyLock::new(|| Mutex::new(vec!["favicon.ico", "/internal-backstage", "/metrics"]));

/// Adds one or more paths to the list of excluded paths for metrics collection, every url that starts with one
/// of the paths in the list is excluded.
/// This is useful for paths that you do not want to track metrics for, such as health checks or static assets,
/// NOTE: the /metrics endpoint, used by prometheus to scrape the service is in the list by default.
pub fn add_excluded_paths(paths: &[&'static str]) {
    EXCLUDED_PATHS
        .lock()
        .expect("Failed to lock EXCLUDED_PATHS")
        .extend_from_slice(paths);
}

fn excluded_path(path: &str) -> bool {
    EXCLUDED_PATHS
        .lock()
        .expect("Failed to lock")
        .iter()
        .any(|&p| path.starts_with(p))
}

#[derive(Clone)]
pub struct PrometheusAxumLayer {
    /// Exposed registry for custom prometheus metrics
    pub registry: Registry,
    pub connect_via: ConnectVia,
}

impl PrometheusAxumLayer {
    pub fn new(app_name: &str, instance_id: &str) -> Self {
        Self {
            registry: Registry::default(),
            connect_via: ConnectVia {
                app_name: app_name.to_string(),
                instance_id: instance_id.to_string(),
            },
        }
    }
}

impl<S> Layer<S> for PrometheusAxumLayer {
    type Service = PrometheusService<S>;

    fn layer(&self, service: S) -> Self::Service {
        PrometheusService {
            service,
            connect_via: self.connect_via.clone(),
        }
    }
}

#[derive(Clone)]
pub struct PrometheusService<S> {
    service: S,
    connect_via: ConnectVia,
}

impl<S, B> Service<Request<B>> for PrometheusService<S>
where
    S: Service<Request<B>, Response = Response> + Send + Clone + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let method = req.method().as_str().to_owned();
        let path = req
            .extensions()
            .get::<MatchedPath>()
            .map(|p| p.as_str().to_owned())
            .unwrap_or_else(|| req.uri().path().to_owned());

        let skip = excluded_path(&path);
        let app_name = self.connect_via.app_name.clone();
        let instance_id = self.connect_via.instance_id.clone();
        if !skip {
            HTTP_REQUESTS_PENDING_METRIC
                .with_label_values(&[&method, &path, &app_name, &instance_id])
                .inc();
        }
        let start = Instant::now();
        let mut service = self.service.clone();
        Box::pin(async move {
            let response = service.call(req).await?;
            let status = response.status().as_u16().to_string();
            if !skip {
                // To prevent all 404s exploding cardinality
                let used_path = match response.status() {
                    StatusCode::NOT_FOUND => "/{unknown}",
                    _ => &path,
                }
                .to_string();
                HTTP_REQUESTS_PENDING_METRIC
                    .with_label_values(&[&method, &path, &app_name, &instance_id])
                    .dec();
                HTTP_REQUESTS_TOTAL_METRIC
                    .with_label_values(&[&method, &used_path, &status, &app_name, &instance_id])
                    .inc();
                let elapsed = start.elapsed().as_secs_f64();
                HTTP_REQUEST_DURATION_MILLISECONDS
                    .with_label_values(&[&method, &used_path, &status, &app_name, &instance_id])
                    .observe(elapsed * 1000.0);
                let size = response.body().size_hint().lower();
                HTTP_RESPONSE_SIZE_METRIC
                    .with_label_values(&[&method, &path, &app_name, &instance_id])
                    .observe(size as f64);
            }
            Ok(response)
        })
    }
}

pub async fn render_prometheus_metrics() -> impl IntoResponse {
    let metrics = gather();
    let encoder = TextEncoder::new();
    encoder
        .encode_to_string(&metrics)
        .expect("Failed to encode metrics")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing};
    use tower::ServiceExt;

    const APP_NAME: &str = "unleash_edge_test";
    const INSTANCE_ID: &str = "unleash_edge_test";
    #[tokio::test]
    async fn test_metrics_layer_basic() {
        let app = Router::new()
            .route("/test", routing::get(async || "Hello, World!"))
            .layer(PrometheusAxumLayer::new(APP_NAME, INSTANCE_ID));

        // we do not know the initial value of the counter since we may use it in multiple tests
        let counter = HTTP_REQUESTS_TOTAL_METRIC
            .get_metric_with_label_values(&["GET", "/test", "200", APP_NAME, INSTANCE_ID])
            .unwrap()
            .get();
        let another_counter = HTTP_REQUESTS_TOTAL_METRIC
            .get_metric_with_label_values(&["GET", "/test2", "200", APP_NAME, INSTANCE_ID])
            .unwrap()
            .get();
        assert_eq!(another_counter, 0);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);

        let updated_counter = HTTP_REQUESTS_TOTAL_METRIC
            .get_metric_with_label_values(&["GET", "/test", "200", APP_NAME, INSTANCE_ID])
            .unwrap()
            .get();
        let another_counter = HTTP_REQUESTS_TOTAL_METRIC
            .get_metric_with_label_values(&["GET", "/test2", "200", APP_NAME, INSTANCE_ID])
            .unwrap()
            .get();
        assert_eq!(another_counter, 0);
        assert_eq!(updated_counter, counter + 1);
    }

    #[test]
    fn test_excluded_path() {
        let paths = vec!["/healthcheck"];
        add_excluded_paths(&paths);
        assert!(excluded_path("/metrics"));
        assert!(excluded_path("/internal-backstage/health"));
        assert!(excluded_path("/internal-backstage/ready"));
        assert!(!excluded_path("/test"));
        assert!(!excluded_path("/api/v1/resource"));
    }

    #[tokio::test]
    async fn test_metrics_layer_body_size() {
        let app = Router::new()
            .route("/test_body_size", routing::get(async || "Hello, World!"))
            .layer(PrometheusAxumLayer::new(APP_NAME, INSTANCE_ID));

        // we do not know the initial value of the counter since we may use it in multiple tests
        let counter = HTTP_REQUESTS_TOTAL_METRIC
            .get_metric_with_label_values(&["GET", "/test_body_size", "200", APP_NAME, INSTANCE_ID])
            .unwrap()
            .get();
        assert_eq!(counter, 0);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/test_body_size")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);

        let updated_counter = HTTP_REQUESTS_TOTAL_METRIC
            .get_metric_with_label_values(&["GET", "/test_body_size", "200", APP_NAME, INSTANCE_ID])
            .unwrap()
            .get();
        assert_eq!(updated_counter, counter + 1);
        let body_size = HTTP_RESPONSE_SIZE_METRIC
            .get_metric_with_label_values(&["GET", "/test_body_size", APP_NAME, INSTANCE_ID])
            .unwrap()
            .get_sample_sum();
        assert_eq!(
            body_size, 13.0,
            "it should be 13 bytes for \"Hello, World!\""
        );
    }

    async fn call_metrics(app: Router) -> String {
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), i32::MAX as usize)
            .await
            .expect("Body should be there");
        String::from_utf8(body.to_vec()).expect("Response should be valid UTF-8")
    }

    #[tokio::test]
    async fn test_render_and_path_skipped() {
        let app = Router::new()
            .route("/test_new", routing::get(async || "Hello, World!"))
            .route("/metrics", routing::get(render_prometheus_metrics))
            .layer(PrometheusAxumLayer::new(APP_NAME, INSTANCE_ID));

        let body_str = call_metrics(app.clone()).await;
        assert!(!body_str.contains("endpoint=\"/metrics\""));
        assert!(!body_str.contains("endpoint=\"/test_new\""));

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/test_new")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body_str = call_metrics(app.clone()).await;
        assert!(body_str.contains("http_server_duration_milliseconds_bucket"));
        assert!(body_str.contains(&format!("{}_bucket", HTTP_RESPONSE_SIZE)));
        assert!(body_str.contains("endpoint=\"/test_new\""));
        assert!(body_str.contains("# TYPE "));
    }
}
