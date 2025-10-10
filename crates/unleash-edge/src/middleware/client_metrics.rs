use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use prometheus::{IntCounterVec, register_int_counter_vec};
use std::sync::LazyLock;
use unleash_edge_appstate::AppState;

pub const INSTANCE_ID: &str = "instance_id";
pub const APP_NAME: &str = "app_name";
pub const SDK_NAME: &str = "sdk_name";
pub const SDK_VERSION: &str = "sdk_version";
pub const SPEC_VERSION: &str = "spec_version";
pub const CONTENT_ENCODING: &str = "content_encoding";

pub static CLIENT_SPEC_VERSION: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "client_spec_version",
        "The client SDK version",
        &[APP_NAME, INSTANCE_ID, SPEC_VERSION]
    )
    .unwrap()
});

pub static CLIENT_SDK_VERSION: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "client_sdk_version",
        "The client SDK version",
        &[APP_NAME, INSTANCE_ID, SDK_NAME, SDK_VERSION]
    )
    .unwrap()
});

pub static CONTENT_ENCODING_COUNTER: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "request_content_encoding",
        "The value of the Accept-encoding header",
        &[APP_NAME, INSTANCE_ID, CONTENT_ENCODING]
    )
    .unwrap()
});

pub async fn extract_request_metrics(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let app_name = state.connect_via.app_name.clone();
    let instance_id = state.connect_via.instance_id.clone();
    let headers = req.headers().clone();
    let res = next.run(req).await;
    if res.status().is_success() {
        if let Some(content_encoding) = headers.get(http::header::ACCEPT_ENCODING) {
            if let Ok(content_encoding) = content_encoding.to_str() {
                CONTENT_ENCODING_COUNTER
                    .with_label_values(&[
                        &app_name.as_str(),
                        &instance_id.as_str(),
                        &content_encoding,
                    ])
                    .inc()
            }
        } else {
            CONTENT_ENCODING_COUNTER
                .with_label_values(&[&app_name.as_str(), &instance_id.as_str(), &"none"])
                .inc()
        }
        if let Some(sdk_version) = headers.get("unleash-sdk")
            && let Ok(sdk_version) = sdk_version.to_str()
        {
            let (sdk_name, version) = sdk_version
                .split_once(':')
                .unwrap_or((sdk_version, "unknown"));
            CLIENT_SDK_VERSION
                .with_label_values(&[
                    &app_name.as_str(),
                    &instance_id.as_str(),
                    &sdk_name,
                    &version,
                ])
                .inc();
        }
        if let Some(client_spec) = headers.get("unleash-client-spec")
            && let Ok(client_spec) = client_spec.to_str()
        {
            CLIENT_SPEC_VERSION
                .with_label_values(&[&app_name.as_str(), &instance_id.as_str(), &client_spec])
                .inc();
        }
    }
    res
}
