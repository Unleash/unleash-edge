use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use unleash_edge_appstate::AppState;
use unleash_edge_types::headers::UNLEASH_INTERVAL;

fn should_observe_connection_consumption(path: &str, status_code: u16) -> bool {
    let is_valid_path = path.starts_with("/api/client/features")
        || path.starts_with("/api/client/delta")
        || path.starts_with("/api/client/metrics");

    is_valid_path && ((200..300).contains(&status_code) || status_code == 304)
}

pub async fn connection_consumption(state: State<AppState>, req: Request, next: Next) -> Response {
    let url = req.uri().clone();
    let path = url.path();
    let should_observe = path.starts_with("/api/client/features")
        || path.starts_with("/api/client/delta")
        || path.starts_with("/api/client/metrics");
    let interval = if should_observe {
        req.headers()
            .get(UNLEASH_INTERVAL)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
    } else {
        None
    };

    let instance_data = state.edge_instance_data.clone();
    let res = next.run(req).await;
    let status_code = res.status().as_u16();
    if !should_observe_connection_consumption(&path, status_code) {
        return res;
    }
    instance_data.observe_connection_consumption(path, interval);
    res

}