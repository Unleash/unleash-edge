use http::{Request, Response, Uri};
use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;
use std::task::{Context, Poll};
use tower::{Layer, Service};

static MULTIPLE_SLASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/{2,}").unwrap());

pub struct NormalizePathFullLayer;
#[derive(Debug, Copy, Clone)]
pub struct NormalizePathFull<S> {
    inner: S,
}

impl<S> Layer<S> for NormalizePathFullLayer {
    type Service = NormalizePathFull<S>;

    fn layer(&self, inner: S) -> Self::Service {
        NormalizePathFull { inner }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for NormalizePathFull<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        trim_trailing_and_double_slashes(req.uri_mut());
        self.inner.call(req)
    }
}

fn trim_trailing_and_double_slashes(uri: &mut Uri) {
    if !uri.path().ends_with('/') && !uri.path().contains("//") {
        return;
    }

    let with_leading_slash = format!("/{}", uri.path().trim_matches('/'));
    let new_path = MULTIPLE_SLASHES.replace_all(&with_leading_slash, "/");
    let mut parts = uri.clone().into_parts();
    let new_path_and_query = if let Some(path_and_query) = &parts.path_and_query {
        let new_path_and_query = if let Some(query) = path_and_query.query() {
            Cow::Owned(format!("{}?{}", new_path, query))
        } else {
            new_path
        }
        .parse()
        .unwrap();
        Some(new_path_and_query)
    } else {
        None
    };

    parts.path_and_query = new_path_and_query;
    if let Ok(new_uri) = Uri::from_parts(parts) {
        *uri = new_uri
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use tower::{ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn trim_works() {
        async fn handle(request: Request<()>) -> Result<Response<String>, Infallible> {
            Ok(Response::new(request.uri().to_string()))
        }

        let mut svc = ServiceBuilder::new()
            .layer(NormalizePathFullLayer)
            .service_fn(handle);
        let body = svc
            .ready()
            .await
            .unwrap()
            .call(Request::builder().uri("//foo//bar/").body(()).unwrap())
            .await
            .unwrap()
            .into_body();
        assert_eq!(body, "/foo/bar");
    }
}
