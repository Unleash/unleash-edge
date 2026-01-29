use base64::Engine;
use prometheus::gather;
use prometheus_reqwest_remote_write::WriteRequest;
use reqwest::{Client, header};
use tracing::debug;
use unleash_edge_types::BackgroundTask;

pub struct PrometheusWriteTaskArgs {
    pub url: String,
    pub interval: u64,
    pub app_name: String,
    pub client_id: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

pub fn create_prometheus_write_task(
    PrometheusWriteTaskArgs {
        url,
        interval,
        app_name,
        client_id,
        username,
        password,
    }: PrometheusWriteTaskArgs,
) -> BackgroundTask {
    let client = get_client(username.clone(), password.clone());
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(interval);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    remote_write_prom(url.clone(), client.clone(), app_name.clone(), client_id.clone()).await;
                }
            }
        }
    })
}

fn get_client(username: Option<String>, password: Option<String>) -> Client {
    if let Some(uname) = username.clone() {
        let mut headers = header::HeaderMap::new();
        let mut value = header::HeaderValue::from_str(&format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!(
                "{}:{}",
                uname,
                password.clone().unwrap_or_default()
            ))
        ))
        .expect("Could not create header");
        value.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, value);
        Client::builder()
            .default_headers(headers)
            .build()
            .expect("Could not build client")
    } else {
        Client::new()
    }
}

async fn remote_write_prom(
    url: String,
    client: Client,
    app_name: String,
    client_id: Option<String>,
) {
    let write_request = WriteRequest::from_metric_families(
        gather(),
        Some(
            std::iter::once(("app_name".into(), app_name))
                .chain(client_id.into_iter().map(|c| ("client_id".into(), c)))
                .collect(),
        ),
    )
    .expect("Could not format write request");
    let http_request = write_request
        .build_http_request(client.clone(), &url, "unleash_edge")
        .expect("Failed to build http request");

    match client.execute(http_request).await {
        Ok(r) => {
            if !r.status().is_success() {
                tracing::warn!("Prometheus push failed with status: {}", r.status());
            }
            debug!("Prometheus push successful");
        }
        Err(e) => {
            tracing::warn!("Prometheus push failed with error: {:?}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use axum_test::TestServer;
    use base64::Engine;
    use reqwest::StatusCode;

    async fn handle_posted_data(req: Request) -> impl IntoResponse {
        let b64 = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", "username", "password"));
        let b64 = format!("Basic {b64}");
        let auth_header = req
            .headers()
            .get("Authorization")
            .unwrap()
            .to_str()
            .unwrap();

        assert_eq!(&b64, auth_header);
        Response::builder()
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    pub async fn client_includes_username_and_password_as_base64_header() {
        let client = super::get_client(Some("username".into()), Some("password".into()));
        let router = Router::new().route("/prometheus", post(handle_posted_data));
        let srv = TestServer::builder()
            .http_transport()
            .build(router)
            .unwrap();
        let _ = super::remote_write_prom(
            srv.server_url("/prometheus").unwrap().to_string(),
            client,
            "hosted-edge".into(),
            Some("hosted".into()),
        )
        .await;
    }
}
