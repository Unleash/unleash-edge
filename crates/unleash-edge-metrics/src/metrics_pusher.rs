use base64::Engine;
use prometheus::gather;
use prometheus_reqwest_remote_write::WriteRequest;
use reqwest::{Client, header};
use tracing::debug;
use unleash_edge_types::BackgroundTask;

pub fn create_prometheus_write_task(
    url: String,
    interval: u64,
    app_name: String,
    username: Option<String>,
    password: Option<String>,
) -> BackgroundTask {
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(interval);
        let client = get_client(username.clone(), password.clone());
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    remote_write_prom(url.clone(), client.clone(), app_name.clone()).await;
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

async fn remote_write_prom(url: String, client: Client, app_name: String) {
    let write_request =
        WriteRequest::from_metric_families(gather(), Some(vec![("app_name".into(), app_name)]))
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
