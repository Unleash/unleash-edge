use std::pin::Pin;
use std::sync::Arc;

use actix_service::{Service, Transform};
use actix_web::body::{BodySize, BoxBody, EitherBody, MessageBody, None as BodyNone};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::header::{ETag, EntityTag, IfNoneMatch, TryIntoHeaderPair};
use actix_web::http::Method;
use actix_web::web::Bytes;
use actix_web::{HttpMessage, HttpRequest, HttpResponse};
use base64::Engine;
use core::fmt::Write;
use futures::{
    future::{ok, Ready},
    Future,
};
use tracing::debug;
use xxhash_rust::xxh3::xxh3_128;

use crate::http::feature_refresher::FeatureRefresher;

#[derive(Default, Clone)]
pub struct EdgeETag {
    pub feature_refresher: Option<Arc<FeatureRefresher>>,
}

impl<S, B> Transform<S, ServiceRequest> for EdgeETag
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<BoxBody>>;
    type Error = actix_web::Error;
    type Transform = EdgeETagMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(EdgeETagMiddleware {
            service: service,
            feature_refresher: self.feature_refresher.clone(),
        })
    }
}
type Buffer = str_buf::StrBuf<62>;

pub struct EdgeETagMiddleware<S> {
    service: S,
    feature_refresher: Option<Arc<FeatureRefresher>>,
}

impl<S, B> Service<ServiceRequest> for EdgeETagMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<BoxBody>>;
    type Error = actix_web::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<ServiceResponse<EitherBody<BoxBody>>, Self::Error>>>>;

    actix_service::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let request_etag_header: Option<IfNoneMatch> = req.get_header();
        let method = req.method().clone();
        let feature_refresher = self.feature_refresher.clone();
        debug!("Evaluating ETag");
        if we_know_this_etag_from_upstream(feature_refresher, &request_etag_header) {
            Box::pin(async move {
                return Ok(ServiceResponse::new(
                    req.request().clone(),
                    HttpResponse::NotModified().body(BodyNone::new()),
                )
                .into_response(HttpResponse::NotModified().body(BodyNone::new()))
                .map_into_right_body());
            })
        } else {
            let fut = self.service.call(req);
            Box::pin(async move {
                let res: ServiceResponse<B> = fut.await?;
                match method {
                    Method::GET => {
                        let mut modified = true;
                        let mut payload: Option<Bytes> = None;
                        let mut res = res.map_body(|_h, body| match body.size() {
                            BodySize::Sized(_size) => {
                                let bytes = body.try_into_bytes().unwrap_or_else(|_| Bytes::new());
                                payload = Some(bytes.clone());

                                bytes.clone().boxed()
                            }
                            _ => body.boxed(),
                        });
                        match payload {
                            Some(bytes) => {
                                let response_hash = xxh3_128(&bytes);
                                let base64 = base64::prelude::BASE64_URL_SAFE
                                    .encode(response_hash.to_le_bytes());
                                let mut buff = Buffer::new();
                                let _ = write!(buff, "{:x}-{}", bytes.len(), base64);
                                let tag = EntityTag::new_weak(buff.to_string());
                                if let Some(request_etag_header) = request_etag_header {
                                    if request_etag_header == IfNoneMatch::Any
                                        || request_etag_header.to_string() == tag.to_string()
                                    {
                                        modified = false
                                    }
                                }
                                if modified {
                                    if let Ok((name, value)) = ETag(tag.clone()).try_into_pair() {
                                        res.headers_mut().insert(name, value);
                                    }
                                }
                            }
                            None => {}
                        }

                        Ok(match modified {
                            false => res
                                .into_response(HttpResponse::NotModified().body(BodyNone::new()))
                                .map_into_right_body(),
                            true => res.map_into_left_body(),
                        })
                    }
                    _ => Ok(res.map_into_boxed_body().map_into_left_body()),
                }
            })
        }
    }
}

fn we_know_this_etag_from_upstream(
    feature_refresher: Option<Arc<FeatureRefresher>>,
    if_none_match: &Option<IfNoneMatch>,
) -> bool {
    match if_none_match {
        Some(none_match) => match feature_refresher {
            Some(fr) => fr.has_token_with_etag(&none_match),
            None => false,
        },
        None => false,
    }
}
