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

    #[tokio::test]
    async fn trims_all_amount_of_slashes() {
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
            .call(
                Request::builder()
                    .uri("/////foo////////////bar//////")
                    .body(())
                    .unwrap(),
            )
            .await
            .unwrap()
            .into_body();
        assert_eq!(body, "/foo/bar");
    }

    #[test]
    fn is_noop_if_no_trailing_slash() {
        let mut uri = "/foo".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/foo");
    }

    #[test]
    fn maintains_query() {
        let mut uri = "/foo/?a=a".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/foo?a=a");
    }

    #[test]
    fn removes_multiple_trailing_slashes() {
        let mut uri = "/foo////".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/foo");
    }

    #[test]
    fn removes_multiple_trailing_slashes_even_with_query() {
        let mut uri = "/foo////?a=a".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/foo?a=a");
    }

    #[test]
    fn is_noop_on_index() {
        let mut uri = "/".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/");
    }

    #[test]
    fn removes_multiple_trailing_slashes_on_index() {
        let mut uri = "////".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/");
    }

    #[test]
    fn removes_multiple_trailing_slashes_on_index_even_with_query() {
        let mut uri = "////?a=a".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/?a=a");
    }

    #[test]
    fn removes_multiple_slashes_in_the_middle_of_path() {
        let mut uri = "/foo//////bar////baz////".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/foo/bar/baz");
    }

    #[test]
    fn removes_multiple_preceding_slashes_even_with_query() {
        let mut uri = "///foo//?a=a".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/foo?a=a");
    }

    #[test]
    fn removes_multiple_preceding_slashes() {
        let mut uri = "///foo".parse::<Uri>().unwrap();
        trim_trailing_and_double_slashes(&mut uri);
        assert_eq!(uri, "/foo");
    }
}
