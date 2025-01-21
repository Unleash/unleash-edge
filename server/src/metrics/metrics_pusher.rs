use base64::Engine;
use prometheus_reqwest_remote_write::WriteRequest;
use reqwest::{header, Client};
use tracing::debug;

fn get_http_client(username: Option<String>, password: Option<String>) -> Client {
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
        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("Could not build client")
    } else {
        reqwest::Client::new()
    }
}

pub async fn prometheus_remote_write(
    registry: prometheus::Registry,
    url: Option<String>,
    interval: u64,
    username: Option<String>,
    password: Option<String>,
    app_name: String,
) {
    let sleep_duration = tokio::time::Duration::from_secs(interval);
    let client = get_http_client(username, password);
    if let Some(address) = url {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    remote_write_prom(registry.clone(), address.clone(), client.clone(), app_name.clone()).await;
                }
            }
        }
    } else {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                }
            }
        }
    }
}

async fn remote_write_prom(
    registry: prometheus::Registry,
    url: String,
    client: reqwest::Client,
    app_name: String,
) {
    let write_request = WriteRequest::from_metric_families(
        registry.gather(),
        Some(vec![("app_name".into(), app_name)]),
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
