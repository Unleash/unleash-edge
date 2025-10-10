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
pub const ACCEPT_ENCODING: &str = "accept_encoding";
pub const PATH: &str = "path";

pub static CLIENT_SPEC_VERSION: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "client_spec_version",
        "The client spec version",
        &[APP_NAME, INSTANCE_ID, SDK_NAME, SDK_VERSION, SPEC_VERSION]
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
        &[
            APP_NAME,
            INSTANCE_ID,
            ACCEPT_ENCODING,
            SDK_NAME,
            SDK_VERSION,
            PATH
        ]
    )
    .unwrap()
});

pub async fn extract_request_metrics(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let uri = req.uri().clone();
    let path = uri.path();
    let app_name = state.connect_via.app_name.as_str();
    let instance_id = state.connect_via.instance_id.as_str();
    let headers = req.headers().clone();
    let res = next.run(req).await;
    if res.status().is_success() && !path.contains("internal-backstage") {
        let (sdk_name, version) = match headers.get("unleash-sdk") {
            Some(sdk_version) => {
                if let Ok(sdk_version) = sdk_version.to_str() {
                    sdk_version
                        .split_once(':')
                        .unwrap_or((sdk_version, "unknown"))
                } else {
                    ("unknown", "unknown")
                }
            }
            None => ("unknown", "unknown"),
        };
        if let Some(accept_encoding) = headers.get(http::header::ACCEPT_ENCODING) {
            if let Ok(accept_encoding) = accept_encoding.to_str() {
                CONTENT_ENCODING_COUNTER
                    .with_label_values(&[
                        &app_name,
                        &instance_id,
                        &accept_encoding,
                        &sdk_name,
                        &version,
                        &path,
                    ])
                    .inc()
            }
        } else {
            CONTENT_ENCODING_COUNTER
                .with_label_values(&[&app_name, &instance_id, &"none", &sdk_name, &version, &path])
                .inc()
        }

        CLIENT_SDK_VERSION
            .with_label_values(&[&app_name, &instance_id, &sdk_name, &version])
            .inc();

        if let Some(client_spec) = headers.get("unleash-client-spec")
            && let Ok(client_spec) = client_spec.to_str()
        {
            CLIENT_SPEC_VERSION
                .with_label_values(&[&app_name, &instance_id, &sdk_name, &version, &client_spec])
                .inc();
        }
    }
    res
}
