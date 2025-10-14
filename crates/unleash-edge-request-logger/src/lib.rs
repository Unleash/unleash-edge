use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use http::{HeaderValue, header};
use tracing::trace;

pub async fn log_request_middleware(req: Request, next: Next) -> Response {
    let uri = req.uri().clone();
    let mut headers = req.headers().clone();
    let method = req.method().clone();
    headers.insert(
        header::AUTHORIZATION,
        headers
            .get(header::AUTHORIZATION)
            .map(|f| {
                if f.is_empty() {
                    HeaderValue::from_str("AuthorizationEmpty").unwrap()
                } else {
                    HeaderValue::from_str("***redacted***").unwrap()
                }
            })
            .unwrap_or_else(|| HeaderValue::from_str("No authorization header").unwrap()),
    );
    trace!(
        "Request: uri=[{}], method=[{}], headers=[{:?}]",
        uri, method, headers
    );
    next.run(req).await
}
