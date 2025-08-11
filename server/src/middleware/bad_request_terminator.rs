// Forces a close connection for all client errors. This means the LB will not reuse the connection here.
// This is a work around for https://github.com/actix/actix-web/issues/3715 which causes sporadic timeouts when
// Edge is behind a pipeline LB and a bad request with a body is sent.

use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

use actix_http::ConnectionType;
use actix_service::{Service, Transform};
use actix_web::{
    Error,
    body::{BoxBody, EitherBody, MessageBody},
    dev::{ServiceRequest, ServiceResponse},
    http::header::{CONNECTION, HeaderValue},
};

pub struct BadRequestTerminator;

impl<S, B> Transform<S, ServiceRequest> for BadRequestTerminator
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<B, BoxBody>>;
    type Error = Error;
    type InitError = ();
    type Transform = ConnectionTerminatorMiddleware<S, B>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Transform, Self::InitError>>>>;

    fn new_transform(&self, service: S) -> Self::Future {
        Box::pin(async move {
            Ok(ConnectionTerminatorMiddleware {
                service: Rc::new(service),
                _phantom: PhantomData,
            })
        })
    }
}

pub struct ConnectionTerminatorMiddleware<S, B> {
    service: Rc<S>,
    _phantom: PhantomData<B>,
}

impl<S, B> Service<ServiceRequest> for ConnectionTerminatorMiddleware<S, B>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<B, BoxBody>>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);

        Box::pin(async move {
            let mut res = service.call(req).await?;

            if res.status().is_client_error() {
                res.headers_mut()
                    .insert(CONNECTION, HeaderValue::from_static("close"));
                res.response_mut()
                    .head_mut()
                    .set_connection_type(ConnectionType::Close);
                Ok(res.map_into_left_body())
            } else {
                Ok(res.map_into_left_body())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{
        App, HttpResponse,
        http::{StatusCode, header::CONNECTION},
        test::{TestRequest, call_service, init_service},
        web,
    };

    async fn bad_request_handler() -> HttpResponse {
        HttpResponse::BadRequest().body("oops")
    }

    async fn unauthorized_handler() -> HttpResponse {
        HttpResponse::Unauthorized().finish()
    }

    async fn ok_handler() -> HttpResponse {
        HttpResponse::Ok().body("all good")
    }

    #[tokio::test]
    async fn adds_connection_close_on_4xx() {
        let app = init_service(
            App::new()
                .wrap(BadRequestTerminator)
                .route("/bad", web::get().to(bad_request_handler))
                .route("/unauth", web::get().to(unauthorized_handler))
                .route("/ok", web::get().to(ok_handler)),
        )
        .await;

        let req = TestRequest::get().uri("/bad").to_request();
        let res = call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            res.headers()
                .get(CONNECTION)
                .expect("Missing Connection header"),
            "close"
        );

        let req = TestRequest::get().uri("/unauth").to_request();
        let res = call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            res.headers()
                .get(CONNECTION)
                .expect("Missing Connection header"),
            "close"
        );
    }

    #[tokio::test]
    async fn does_not_add_connection_close_on_2xx() {
        let app = init_service(
            App::new()
                .wrap(BadRequestTerminator)
                .route("/ok", web::get().to(ok_handler)),
        )
        .await;

        let req = TestRequest::get().uri("/ok").to_request();
        let res = call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get(CONNECTION).is_none());
    }
}
