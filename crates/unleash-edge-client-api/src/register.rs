use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use tracing::instrument;
use unleash_edge_appstate::AppState;
use unleash_edge_types::EDGE_VERSION;
use unleash_edge_types::tokens::EdgeToken;
use unleash_types::client_metrics::ClientApplication;

#[utoipa::path(
    path = "/register",
    post,
    context_path = "/api/client",
    responses(
        (status = 202, description = "Accepted client application registration"),
        (status = 403, description = "Was not allowed to register client application"),
    ),
    request_body = ClientApplication,
    security(
        ("Authorization" = [])
    )
)]
#[instrument(skip(app_state, edge_token, client_application))]
pub async fn register(
    app_state: State<AppState>,
    edge_token: EdgeToken,
    client_application: Json<ClientApplication>,
) -> impl IntoResponse {
    unleash_edge_metrics::client_metrics::register_client_application(
        edge_token,
        &app_state.connect_via,
        client_application.0,
        app_state.metrics_cache.clone(),
    );
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .header("X-Edge-Version", EDGE_VERSION)
        .body(Body::empty())
        .unwrap()
}

pub fn router() -> Router<AppState> {
    Router::new().route("/register", post(register))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderValue, StatusCode};
    use axum_test::TestServer;
    use std::str::FromStr;
    use std::sync::Arc;
    use unleash_edge_appstate::AppState;
    use unleash_edge_types::metrics::{ApplicationKey, MetricsCache};
    use unleash_edge_types::tokens::EdgeToken;
    use unleash_edge_types::{EDGE_VERSION, TokenCache};
    use unleash_types::client_metrics::{ClientApplication, ConnectVia};

    fn build_server(metrics_cache: Arc<MetricsCache>, token_cache: Arc<TokenCache>) -> TestServer {
        let app_state = AppState::builder()
            .with_metrics_cache(metrics_cache.clone())
            .with_token_cache(token_cache.clone())
            .with_connect_via(ConnectVia {
                app_name: "unleash-edge".into(),
                instance_id: "unleash-edge-test-server".into(),
            })
            .build();
        let router = super::router().with_state(app_state);
        TestServer::builder()
            .http_transport()
            .build(router)
            .unwrap()
    }

    #[tokio::test]
    async fn register_endpoint_correctly_aggregates_applications() {
        let metrics_cache = Arc::new(MetricsCache::default());
        let token_cache = Arc::new(TokenCache::default());
        let token =
            EdgeToken::from_str("*:development.somesecretstring").expect("Could not parse token");
        token_cache.insert(token.token.clone(), token.clone());
        let mut client_app = ClientApplication::new("test_application", 15);
        client_app.instance_id = Some("test_instance".into());
        let server = build_server(metrics_cache.clone(), token_cache);
        make_register_post_request(&server, &token.token, client_app.clone()).await;
        assert_eq!(metrics_cache.applications.len(), 1);
        let application_key = ApplicationKey {
            app_name: client_app.app_name.clone(),
            instance_id: client_app.instance_id.unwrap(),
        };
        let saved_app = metrics_cache
            .applications
            .get(&application_key)
            .unwrap()
            .value()
            .clone();
        assert_eq!(saved_app.app_name, client_app.app_name);
        assert_eq!(saved_app.connect_via.unwrap()[0].app_name, "unleash-edge");
    }
    async fn make_register_post_request(server: &TestServer, token: &str, app: ClientApplication) {
        let r = server
            .post("/register")
            .add_header("Authorization", token)
            .json(&serde_json::to_value(app).unwrap())
            .await;
        assert_eq!(r.status_code(), StatusCode::ACCEPTED);
        assert_eq!(
            r.headers().get("X-Edge-Version"),
            Some(&HeaderValue::from_static(EDGE_VERSION))
        );
    }
}
