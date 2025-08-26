use axum::body::Body;
use axum::response::Response;
use axum::{extract::Request, middleware::Next, response::IntoResponse};
use etag::EntityTag;
use http::response::Parts;
use http::{HeaderValue, StatusCode, header};
use tracing::{trace, warn};

pub async fn etag_middleware(req: Request, next: Next) -> impl IntoResponse {
    let if_none_match_header = req.headers().get(header::IF_NONE_MATCH).cloned();
    let content_type = req.headers().get(header::CONTENT_TYPE).cloned();
    let path = req.uri().path().to_owned();
    let res = next.run(req).await;
    if path.ends_with("/streaming") {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    process_body(if_none_match_header, path, parts, body).await
}

async fn process_body(
    if_none_match_header: Option<HeaderValue>,
    path: String,
    mut parts: Parts,
    body: Body,
) -> Response {
    match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => {
            if bytes.is_empty() {
                trace!(path, "response without body, skipping etag");
                return Response::from_parts(parts, Body::empty());
            }

            let etag = EntityTag::from_data(&bytes);
            parts.headers.insert(
                header::ETAG,
                HeaderValue::from_str(&etag.to_string()).unwrap(),
            );

            match if_none_match_header {
                None => {
                    trace!(path, %etag, "no if-none-match header");
                    Response::from_parts(parts, Body::from(bytes))
                }
                Some(if_none_match) => {
                    let parsed = if_none_match
                        .to_str()
                        .ok()
                        .and_then(|s| s.parse::<EntityTag>().ok());

                    match parsed {
                        None => {
                            warn!("invalid If-None-Match header: {:?}", if_none_match);
                            Response::from_parts(parts, Body::from(bytes))
                        }
                        Some(if_none_match_etag) => {
                            let some_match = etag.strong_eq(&if_none_match_etag);
                            trace!(path, %etag, some_match, "if-none-match" = %if_none_match_etag);

                            if some_match {
                                parts.status = StatusCode::NOT_MODIFIED;
                                Response::from_parts(parts, Body::empty())
                            } else {
                                Response::from_parts(parts, Body::from(bytes))
                            }
                        }
                    }
                }
            }
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
