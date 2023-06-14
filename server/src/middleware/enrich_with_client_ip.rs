use crate::cli::TrustProxy;
use crate::types::ClientIp;
use actix_http::body::MessageBody;
use actix_http::HttpMessage;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::web::Data;
use std::net::IpAddr;
use std::str::FromStr;
use tracing::{debug, trace};

pub async fn enrich_with_client_ip(
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    debug!("Calling enricher for ip");
    let trust_proxy_config: Option<&Data<TrustProxy>> = req.app_data::<Data<TrustProxy>>();
    let ip = match trust_proxy_config {
        Some(config) => {
            if config.trust_proxy {
                trace!("Trust proxy was configured and enabled");
                req.connection_info().realip_remote_addr().and_then(|r| {
                    trace!("{r}");
                    IpAddr::from_str(r).ok()
                })
            } else {
                trace!("Trust proxy was configured and disabled");
                req.peer_addr().map(|s| s.ip())
            }
        }
        None => {
            trace!("Trust proxy was not configured. Will use peer ip");
            req.peer_addr().map(|s| s.ip())
        }
    };
    if let Some(ip) = ip {
        trace!("Found client ip to be: {}", ip);
        req.extensions_mut().insert(ClientIp { ip });
    }
    srv.call(req).await
}

#[cfg(test)]
mod tests {
    use crate::cli::TrustProxy;
    use crate::middleware::as_async_middleware::as_async_middleware;
    use crate::middleware::enrich_with_client_ip::enrich_with_client_ip;
    use crate::types::{ClientIp, EdgeJsonResult};
    use actix_http::HttpMessage;
    use actix_web::get;
    use actix_web::web::{Data, Json};
    use actix_web::{test, App, HttpRequest};
    use std::net::{IpAddr, SocketAddr};
    use std::str::FromStr;
    use tracing_test::traced_test;

    #[get("/")]
    pub async fn hello_ip(req: HttpRequest) -> EdgeJsonResult<String> {
        let ip = req.extensions().get::<ClientIp>().cloned();
        Ok(Json(ip.map(|ip| ip.to_string()).unwrap_or("No ip".into())))
    }

    #[tokio::test]
    #[traced_test]
    pub async fn should_insert_peer_addr_with_no_trust_configured() {
        let app = test::init_service(
            App::new()
                .wrap(as_async_middleware(enrich_with_client_ip))
                .service(hello_ip),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/")
            .peer_addr(SocketAddr::new(
                IpAddr::from_str("127.0.0.1").unwrap(),
                1337,
            ))
            .to_request();
        let ip: String = test::call_and_read_body_json(&app, req).await;
        assert_eq!(ip, "127.0.0.1");
    }
    #[tokio::test]
    #[traced_test]
    pub async fn should_insert_conn_info_with_trust_configured() {
        let trust_proxy = TrustProxy {
            trust_proxy: true,
            proxy_trusted_servers: vec![],
        };
        let app = test::init_service(
            App::new()
                .app_data(Data::new(trust_proxy))
                .wrap(as_async_middleware(enrich_with_client_ip))
                .service(hello_ip),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/")
            .insert_header(("Forwarded", "For=192.168.0.1"))
            .to_request();
        let ip: String = test::call_and_read_body_json(&app, req).await;
        assert_eq!(ip, "192.168.0.1");
    }

    #[tokio::test]
    #[traced_test]
    pub async fn should_insert_conn_info_from_x_forwarded_for_with_trust_configured() {
        let trust_proxy = TrustProxy {
            trust_proxy: true,
            proxy_trusted_servers: vec![],
        };
        let app = test::init_service(
            App::new()
                .app_data(Data::new(trust_proxy))
                .wrap(as_async_middleware(enrich_with_client_ip))
                .service(hello_ip),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/")
            .insert_header(("X-Forwarded-For", "192.168.0.1,217.0.0.1,255.255.255.0"))
            .to_request();
        let ip: String = test::call_and_read_body_json(&app, req).await;
        assert_eq!(ip, "192.168.0.1");
    }
}
