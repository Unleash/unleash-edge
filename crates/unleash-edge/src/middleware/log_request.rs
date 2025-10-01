use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use http::StatusCode;
use tracing::trace;

pub async fn log_request_middleware(req: Request, next: Next) -> Response {
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    let method = req.method().clone();
    let res = next.run(req).await;
    if res.status() == StatusCode::FORBIDDEN {
        trace!(
            "Forbidden request. uri=[{}], method=[{}], headers=[{:?}]",
            uri, method, headers
        );
    }
    res
}
