use actix_service::{Service, Transform};
use actix_web::{
    Error,
    dev::{ServiceRequest, ServiceResponse},
};
use futures::future::{LocalBoxFuture, Ready, ok};
use std::task::{Context, Poll};
use tracing::info;

pub struct LogStatus;

impl<S, B> Transform<S, ServiceRequest> for LogStatus
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = LogStatusMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(LogStatusMiddleware { service })
    }
}

pub struct LogStatusMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for LogStatusMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let method = req.method().clone();
        let path = req.path().to_string();

        let fut = self.service.call(req);

        Box::pin(async move {
            let res = fut.await;

            match &res {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_client_error() || status.is_server_error() {
                        info!("→ {} {} => {}", method, path, status);
                    }
                }
                Err(err) => {
                    info!("→ {} {} => internal error: {:?}", method, path, err);
                }
            }

            res
        })
    }
}
