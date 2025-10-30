#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::extract::connect_info::MockConnectInfo;
    use clap::Parser;
    use hyper::StatusCode;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use tower::ServiceBuilder;
    use tower_http::normalize_path::NormalizePathLayer;
    use unleash_edge_cli::CliArgs;
    #[tokio::test]
    async fn normalizes_paths() {
        let args = vec![
            "unleash-edge",
            "offline",
            "--client-tokens",
            "[]:development.secret-token2",
            "--frontend-tokens",
            "[]:development.secret-token",
            "-b",
            "../../examples/features.json",
        ];
        let args = CliArgs::parse_from(args);
        let (router, _) = unleash_edge::configure_server(args)
            .await
            .expect("Failed to configure server");
        let svc = ServiceBuilder::new()
            .layer(NormalizePathLayer::trim_trailing_slash())
            .layer(MockConnectInfo(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(127, 0, 0, 1),
                1337,
            ))))
            .service(router);
        let make_it_axum_test_compatible = Router::new().fallback_service(svc);
        let srv = axum_test::TestServer::new(make_it_axum_test_compatible).expect("Fall");

        srv.get("/api/client/features")
            .add_header("Authorization", "[]:development.secret-token2")
            .await
            .assert_status(StatusCode::OK);
        srv.get("/api/client/features/")
            .add_header("Authorization", "[]:development.secret-token2")
            .await
            .assert_status(StatusCode::OK);
        srv.get("/api/client/features2")
            .await
            .assert_status(StatusCode::NOT_FOUND);
        srv.get("/api/frontend")
            .add_header("Authorization", "[]:development.secret-token")
            .await
            .assert_status(StatusCode::OK);
    }
}
