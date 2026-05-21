use crate::frontend::FrontendState;
use axum::extract::{ConnectInfo, FromRef, FromRequestParts};
use axum::http::HeaderMap;
use axum::http::header::FORWARDED;
use axum::http::request::Parts;
use ipnet::IpNet;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use unleash_edge_types::errors::EdgeError;

pub struct ClientIp(pub Option<IpAddr>);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
    FrontendState: FromRef<S>,
{
    type Rejection = EdgeError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let frontend_state = FrontendState::from_ref(state);
        let peer_ip = ConnectInfo::<SocketAddr>::from_request_parts(parts, state)
            .await
            .ok()
            .map(|ConnectInfo(peer_addr)| peer_addr.ip());

        if frontend_state.trust_proxy {
            Ok(ClientIp(
                forwarded_ip(
                    &parts.headers,
                    peer_ip,
                    &frontend_state.proxy_trusted_servers,
                )
                .or_else(|| x_forwarded_for_ip(&parts.headers, peer_ip, &frontend_state))
                .or(peer_ip),
            ))
        } else {
            Ok(ClientIp(peer_ip))
        }
    }
}

fn forwarded_ips(headers: &HeaderMap) -> Vec<IpAddr> {
    headers
        .get(FORWARDED)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .filter_map(|forwarded_element| {
                    forwarded_element.split(';').find_map(|part| {
                        let (name, value) = part.trim().split_once('=')?;
                        if name.eq_ignore_ascii_case("for") {
                            parse_forwarded_ip(value)
                        } else {
                            None
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn forwarded_ip(
    headers: &HeaderMap,
    peer_ip: Option<IpAddr>,
    trusted_proxies: &[IpNet],
) -> Option<IpAddr> {
    client_ip_from_proxy_chain(forwarded_ips(headers), peer_ip, trusted_proxies)
}

fn x_forwarded_for_ips(headers: &HeaderMap) -> Vec<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(',').filter_map(parse_forwarded_ip).collect())
        .unwrap_or_default()
}

fn x_forwarded_for_ip(
    headers: &HeaderMap,
    peer_ip: Option<IpAddr>,
    frontend_state: &FrontendState,
) -> Option<IpAddr> {
    client_ip_from_proxy_chain(
        x_forwarded_for_ips(headers),
        peer_ip,
        &frontend_state.proxy_trusted_servers,
    )
}

fn client_ip_from_proxy_chain(
    mut chain: Vec<IpAddr>,
    peer_ip: Option<IpAddr>,
    trusted_proxies: &[IpNet],
) -> Option<IpAddr> {
    if chain.is_empty() {
        return None;
    }

    if trusted_proxies.is_empty() {
        return chain.first().copied();
    }

    chain.push(peer_ip?);

    while chain
        .last()
        .is_some_and(|ip| trusted_proxies.iter().any(|trusted| trusted.contains(ip)))
    {
        chain.pop();
    }

    chain.pop()
}

fn parse_forwarded_ip(value: &str) -> Option<IpAddr> {
    let value = value.trim().trim_matches('"');
    if let Ok(ip) = IpAddr::from_str(value) {
        return Some(ip);
    }
    if let Ok(addr) = SocketAddr::from_str(value) {
        return Some(addr.ip());
    }
    value
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .and_then(|(ip, _)| IpAddr::from_str(ip).ok())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;
    use std::net::IpAddr;

    #[test]
    fn parses_forwarded_ipv6_with_port() {
        let mut headers = HeaderMap::new();
        headers.insert("Forwarded", r#"for="[2001:db8::1]:1234""#.parse().unwrap());

        assert_eq!(super::forwarded_ips(&headers), vec![ip("2001:db8::1")]);
    }

    #[test]
    fn trusted_proxy_chain_strips_trusted_forwarded_hops_from_the_right() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Forwarded",
            "for=203.0.113.9, for=192.168.0.1, for=10.0.0.1"
                .parse()
                .unwrap(),
        );
        let trusted_proxies = vec!["10.0.0.0/24".parse().unwrap()];

        assert_eq!(
            super::forwarded_ip(&headers, Some(ip("10.0.0.2")), &trusted_proxies),
            Some(ip("192.168.0.1"))
        );
    }

    #[test]
    fn untrusted_peer_ignores_forwarded_chain_when_trusted_proxies_are_configured() {
        let mut headers = HeaderMap::new();
        headers.insert("Forwarded", "for=192.168.0.1".parse().unwrap());
        let trusted_proxies = vec!["127.0.0.1/32".parse().unwrap()];

        assert_eq!(
            super::forwarded_ip(&headers, Some(ip("10.0.0.2")), &trusted_proxies),
            Some(ip("10.0.0.2"))
        );
    }

    #[test]
    fn forwarded_without_configured_trusted_proxies_keeps_leftmost_behavior() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Forwarded",
            "for=192.168.0.1, for=10.0.0.1".parse().unwrap(),
        );

        assert_eq!(
            super::forwarded_ip(&headers, Some(ip("10.0.0.2")), &[]),
            Some(ip("192.168.0.1"))
        );
    }

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }
}
