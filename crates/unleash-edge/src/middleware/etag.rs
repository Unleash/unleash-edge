use axum::{extract::Request, middleware::Next, response::{IntoResponse}};
use etag::EntityTag;
use http::{header, HeaderValue, StatusCode};
use tracing::{trace, warn};

pub async fn etag_middleware(
    req: Request,
    next: Next,
)  -> impl IntoResponse {
    let if_none_match_header = req.headers().get(header::IF_NONE_MATCH).cloned();
    let path = req.uri().path().to_owned();
    let res = next.run(req).await;
    let (mut parts, body) = res.into_parts();

    if let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await {
          match bytes.is_empty() {
        true => {
            trace!(path, "response without body, skipping etag");
            Ok(parts.into_response())
        }
        false => match if_none_match_header {
            None => {
                let etag = EntityTag::from_data(&bytes);

                parts.headers.insert(
                    header::ETAG,
                    HeaderValue::from_str(&etag.to_string()).unwrap(),
                );

                trace!(path, %etag, "no if-none-match header");

                Ok((parts, bytes).into_response())
            }
            Some(if_none_match) => {
                let if_none_match_etag = if_none_match.to_str().unwrap().parse::<EntityTag>();
                match if_none_match_etag {
                    Err(ref err) => {
                        warn!("{} - {:?}", err, &if_none_match_etag);
                        let etag = EntityTag::from_data(&bytes);
                        parts.headers.insert(
                            header::ETAG,
                            HeaderValue::from_str(&etag.to_string()).unwrap(),
                        );
                        Ok((parts, bytes).into_response())
                    }
                    Ok(if_none_match_etag) => {
                        let etag = EntityTag::from_data(&bytes);

                        parts.headers.insert(
                            header::ETAG,
                            HeaderValue::from_str(&etag.to_string()).unwrap(),
                        );

                        let some_match = etag.strong_eq(&if_none_match_etag);

                        trace!(
                            path,
                            %etag,
                            some_match,
                            "if-none-match" = %if_none_match_etag
                        );

                        if some_match {
                            Ok((StatusCode::NOT_MODIFIED, parts).into_response())
                        } else {
                            Ok((parts, bytes).into_response())
                        }
                    }
                }
            }
        },
    } 
    } else {
      Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}