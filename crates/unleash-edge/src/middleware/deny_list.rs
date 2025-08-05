use std::net::SocketAddr;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use reqwest::StatusCode;
use unleash_edge_appstate::AppState;
use unleash_edge_types::ClientIp;

pub async fn deny_middleware(state: State<AppState>, connect_info: ConnectInfo<SocketAddr>, req: Request, next: Next) -> Response {
    if !state.deny_list.is_empty() && state.deny_list.iter().any(|ip| ip.contains(&connect_info.ip())) {
        Response::builder().status(StatusCode::FORBIDDEN).body(Body::empty()).unwrap()
    } else {
        next.run(req).await
    }
}