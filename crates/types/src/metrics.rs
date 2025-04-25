pub const HTTP_REQUESTS_TOTAL: &str = "http_requests_total";
pub const HTTP_REQUESTS_DURATION: &str = "http_server_duration_milliseconds";
pub const HTTP_RESPONSE_SIZE: &str = "http_response_size";
pub const ENDPOINT_LABEL: &str = "endpoint";
pub const METHOD_LABEL: &str = "method";
pub const STATUS_LABEL: &str = "status";

pub const DESIRED_URLS: [&str; 6] = [
    "/api/client/features",
    "/api/client/metrics",
    "/api/client/metrics/bulk",
    "/api/client/metrics/edge",
    "/api/frontend",
    "/api/proxy",
];
