use std::sync::Arc;

use axum::extract::{FromRef, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use unleash_edge_appstate::AppState;
use unleash_edge_types::headers::UNLEASH_INTERVAL;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;

#[derive(Clone)]
pub struct ConsumptionState {
    pub edge_instance_data: Arc<EdgeInstanceData>,
}

impl FromRef<AppState> for ConsumptionState {
    fn from_ref(app: &AppState) -> Self {
        Self {
            edge_instance_data: Arc::clone(&app.edge_instance_data),
        }
    }
}

fn should_observe_connection_consumption(path: &str, status_code: u16) -> bool {
    let is_valid_path = path.contains("client/features")
        || path.contains("client/delta")
        || path.contains("client/metrics");

    is_valid_path && ((200..300).contains(&status_code) || status_code == 304)
}

fn should_observe_request_consumption(path: &str, status_code: u16) -> bool {
    let is_valid_path = path.starts_with("/api/frontend") || path.starts_with("/api/proxy");

    is_valid_path && ((200..300).contains(&status_code) || status_code == 304)
}

pub async fn connection_consumption(
    State(state): State<ConsumptionState>,
    req: Request,
    next: Next,
) -> Response {
    let url = req.uri().clone();
    let path = url.path();
    let should_observe = path.contains("client/features")
        || path.contains("client/delta")
        || path.contains("client/metrics");
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
    if !should_observe_connection_consumption(path, status_code) {
        return res;
    }
    instance_data.observe_connection_consumption(path, interval);
    res
}

pub async fn request_consumption(
    State(state): State<ConsumptionState>,
    req: Request,
    next: Next,
) -> Response {
    let url = req.uri().clone();
    let path = url.path();

    let instance_data = state.edge_instance_data.clone();
    let res = next.run(req).await;
    let status_code = res.status().as_u16();
    if !should_observe_request_consumption(path, status_code) {
        return res;
    }
    instance_data.observe_request_consumption();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::Router;
    use axum::http::StatusCode;
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use axum_test::TestServer;
    use serde_json::Value;
    use std::sync::Arc;
    use ulid::Ulid;
    use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};

    fn connection_totals(instance: &EdgeInstanceData) -> (u64, u64) {
        fn sum_groups(value: &Value, key: &str) -> u64 {
            value
                .get(key)
                .and_then(|groups| groups.as_array())
                .map(|groups| {
                    groups
                        .iter()
                        .map(|group| {
                            group
                                .get("dataPoints")
                                .and_then(|points| points.as_array())
                                .map(|points| {
                                    points
                                        .iter()
                                        .map(|point| {
                                            point
                                                .get("requests")
                                                .and_then(Value::as_u64)
                                                .unwrap_or(0)
                                        })
                                        .sum::<u64>()
                                })
                                .unwrap_or(0)
                        })
                        .sum()
                })
                .unwrap_or(0)
        }

        let serialized = serde_json::to_value(instance).expect("EdgeInstanceData should serialize");
        let connection = serialized
            .get("connectionConsumptionSinceLastReport")
            .unwrap_or(&Value::Null);

        (
            sum_groups(connection, "features"),
            sum_groups(connection, "metrics"),
        )
    }

    fn frontend_requests(instance: &EdgeInstanceData) -> u64 {
        instance
            .request_consumption_since_last_report
            .get_requests("default")
    }

    #[tokio::test]
    async fn test_backend_consumption() {
        let edge_instance_data = Arc::new(EdgeInstanceData::new(
            "test",
            &Ulid::new(),
            Some(Hosting::SelfHosted),
        ));
        let app_state = ConsumptionState {
            edge_instance_data: edge_instance_data.clone(),
        };

        let router = Router::new()
            .route("/api/client/features", get(|| async { StatusCode::OK }))
            .layer(from_fn_with_state(
                app_state.clone(),
                connection_consumption,
            ))
            .with_state(app_state);

        let server = TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build test server");

        let response = server.get("/api/client/features").await;
        assert_eq!(response.status_code(), StatusCode::OK);

        let (feature_requests, metric_requests) = connection_totals(&edge_instance_data);
        assert_eq!(feature_requests, 1);
        assert_eq!(metric_requests, 0);
        assert_eq!(frontend_requests(&edge_instance_data), 0);
    }

    #[tokio::test]
    async fn test_frontend_consumption() {
        let edge_instance_data = Arc::new(EdgeInstanceData::new(
            "test",
            &Ulid::new(),
            Some(Hosting::SelfHosted),
        ));
        let app_state = ConsumptionState {
            edge_instance_data: edge_instance_data.clone(),
        };

        let router = Router::new()
            .route("/api/frontend/features", get(|| async { StatusCode::OK }))
            .layer(from_fn_with_state(app_state.clone(), request_consumption))
            .with_state(app_state);

        let server = TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build test server");

        let response = server.get("/api/frontend/features").await;
        assert_eq!(response.status_code(), StatusCode::OK);

        let (feature_requests, metric_requests) = connection_totals(&edge_instance_data);
        assert_eq!(feature_requests, 0);
        assert_eq!(metric_requests, 0);
        assert_eq!(frontend_requests(&edge_instance_data), 1);
    }
}
