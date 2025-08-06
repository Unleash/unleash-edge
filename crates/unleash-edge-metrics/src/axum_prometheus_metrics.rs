use std::pin::Pin;
use std::sync::{LazyLock, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;
use axum::body::HttpBody;
use axum::extract::{MatchedPath, Request};
use axum::response::{IntoResponse, Response};
use prometheus::{gather, register_gauge_vec, register_histogram_vec, register_int_counter_vec, GaugeVec, HistogramVec, IntCounterVec, Registry, TextEncoder};
use reqwest::StatusCode;
use tower::{Layer, Service};


pub const HTTP_REQUESTS_TOTAL: &str = "http_requests_total";
pub const HTTP_REQUESTS_DURATION: &str = "http_requests_duration_seconds";
pub const HTTP_REQUESTS_PENDING: &str = "http_requests_pending";
pub const HTTP_RESPONSE_SIZE: &str = "http_response_body_size";
pub const ENDPOINT_LABEL: &str = "endpoint";
pub const METHOD_LABEL: &str = "method";
pub const STATUS_LABEL: &str = "status";



static HTTP_REQUESTS_TOTAL_METRIC: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        HTTP_REQUESTS_TOTAL,
        "Total number of HTTP requests",
        &[METHOD_LABEL, ENDPOINT_LABEL, STATUS_LABEL]
    ).unwrap()
});

static HTTP_REQUEST_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HTTP_REQUESTS_DURATION,
        "HTTP request latencies in seconds",
        &[METHOD_LABEL, ENDPOINT_LABEL, STATUS_LABEL]
    ).unwrap()
});

static HTTP_RESPONSE_BODY_SIZE: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        HTTP_RESPONSE_SIZE,
        "Size of HTTP response bodies in bytes",
        &[METHOD_LABEL, ENDPOINT_LABEL],
        vec![0.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0]
    ).unwrap()
});

static HTTP_REQUESTS_PENDING_METRIC: LazyLock<GaugeVec> = LazyLock::new(|| {
    register_gauge_vec!(
        HTTP_REQUESTS_PENDING,
        "Number of pending HTTP requests",
        &[METHOD_LABEL, ENDPOINT_LABEL]
    ).unwrap()
});

static EXCLUDED_PATHS: LazyLock<Mutex<Vec<&'static str>>> = LazyLock::new(|| Mutex::new(vec!["favicon.ico", "/internal-backstage", "/metrics"]));

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
    EXCLUDED_PATHS.lock().expect("Failed to lock").iter().any(|&p| path.starts_with(p))
}

#[derive(Clone)]
pub struct PrometheusAxumLayer {
    /// Exposed registry for custom prometheus metrics
    pub registry: Registry
}

impl PrometheusAxumLayer {
    pub fn new() -> Self {
        Self {
            registry: Registry::default()
        }
    }
    pub fn new_with_registry(registry: Registry) -> Self {
        Self {
            registry
        }
    }
}
impl Default for PrometheusAxumLayer {
    fn default() -> Self {
        Self::new()
    }
}
impl<S> Layer<S> for PrometheusAxumLayer {
    type Service = PrometheusService<S>;

    fn layer(&self, service: S) -> Self::Service {
        PrometheusService { service }
    }
}

#[derive(Clone)]
pub struct PrometheusService<S> {
    service: S,
}

impl<S, B> Service<Request<B>> for PrometheusService<S>
where
    S: Service<Request<B>, Response = Response> + Send + Clone + 'static ,
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
        let path = req.extensions().get::<MatchedPath>()
            .map(|p| p.as_str().to_owned())
            .unwrap_or_else(|| req.uri().path().to_owned());

        let skip = excluded_path(&path);
        if !skip {
            HTTP_REQUESTS_PENDING_METRIC.with_label_values(&[&method, &path]).inc();
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
                    _ => &path
                }.to_string();
                HTTP_REQUESTS_PENDING_METRIC.with_label_values(&[&method, &used_path]).dec();
                HTTP_REQUESTS_TOTAL_METRIC.with_label_values(&[&method, &used_path, &status]).inc();
                let elapsed = start.elapsed().as_secs_f64();
                HTTP_REQUEST_DURATION_SECONDS.with_label_values(&[&method, &used_path, &status]).observe(elapsed);
                let size = response.body().size_hint().lower();
                HTTP_RESPONSE_BODY_SIZE.with_label_values(&[&method, &path]).observe(size as f64);
            }
            Ok(response)
        })
    }
}

pub async fn render_prometheus_metrics() -> impl IntoResponse {
    let metrics = gather();
    let encoder = TextEncoder::new();
    encoder.encode_to_string(&metrics).expect("Failed to encode metrics")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_metrics_layer_basic() {
        let app = Router::new()
            .route("/test", routing::get(async || "Hello, World!"))
            .layer(PrometheusAxumLayer::new());

        // we do not know the initial value of the counter since we may use it in multiple tests
        let counter = HTTP_REQUESTS_TOTAL_METRIC.get_metric_with_label_values(&["GET", "/test", "200"]).unwrap().get();
        let another_counter = HTTP_REQUESTS_TOTAL_METRIC.get_metric_with_label_values(&["GET", "/test2", "200"]).unwrap().get();
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

        let updated_counter = HTTP_REQUESTS_TOTAL_METRIC.get_metric_with_label_values(&["GET", "/test", "200"]).unwrap().get();
        let another_counter = HTTP_REQUESTS_TOTAL_METRIC.get_metric_with_label_values(&["GET", "/test2", "200"]).unwrap().get();
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
            .layer(PrometheusAxumLayer::new());

        // we do not know the initial value of the counter since we may use it in multiple tests
        let counter = HTTP_REQUESTS_TOTAL_METRIC.get_metric_with_label_values(&["GET", "/test_body_size", "200"]).unwrap().get();
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

        let updated_counter = HTTP_REQUESTS_TOTAL_METRIC.get_metric_with_label_values(&["GET", "/test_body_size", "200"]).unwrap().get();
        assert_eq!(updated_counter, counter + 1);
        let body_size = HTTP_RESPONSE_BODY_SIZE
            .get_metric_with_label_values(&["GET", "/test_body_size"])
            .unwrap()
            .get_sample_sum();
        assert_eq!(body_size, 13.0, "it should be 13 bytes for \"Hello, World!\"");
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
        let body = axum::body::to_bytes(response.into_body(), i32::MAX as usize).await.expect("Body should be there");
        String::from_utf8(body.to_vec()).expect("Response should be valid UTF-8")
    }

    #[tokio::test]
    async fn test_render_and_path_skipped() {
        let app = Router::new()
            .route("/test_new", routing::get(async || "Hello, World!"))
            .route("/metrics", routing::get(render_prometheus_metrics))
            .layer(PrometheusAxumLayer::new());

        let body_str = call_metrics(app.clone()).await;
        println!("{}", body_str);
        assert!(!body_str.contains("endpoint=\"/metrics\""));
        assert!(!body_str.contains("endpoint=\"/test_new\""));

        let response = app.clone()
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
        assert!(body_str.contains("http_requests_duration_seconds_bucket"));
        assert!(body_str.contains("http_response_body_size_bucket"));
        assert!(body_str.contains("endpoint=\"/test_new\""));
        assert!(body_str.contains("# TYPE "));
    }
}