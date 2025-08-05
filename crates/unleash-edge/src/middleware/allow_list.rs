use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use unleash_edge_appstate::AppState;
use unleash_edge_types::ClientIp;

pub async fn allow_middleware(state: State<AppState>, connect_info: ConnectInfo<ClientIp>, req: Request, next: Next) -> Response {
    if !state.allow_list.is_empty() && !state.allow_list.iter().any(|ip| ip.contains(&connect_info.ip)) {
        Response::builder().status(StatusCode::FORBIDDEN).body(Body::empty()).unwrap()
    } else {
        next.run(req).await
    }
}