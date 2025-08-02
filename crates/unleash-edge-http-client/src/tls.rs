use rustls::ServerConfig;
use rustls::pki_types::PrivateKeyDer;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::path::PathBuf;
use std::{fs, fs::File, io::BufReader};
use tracing::info;
use unleash_edge_cli::TlsOptions;
use unleash_edge_types::EdgeResult;
use unleash_edge_types::errors::{CertificateError, EdgeError};

pub(crate) fn build_upstream_certificate(
    upstream_certificate: Option<PathBuf>,
) -> EdgeResult<Option<reqwest::tls::Certificate>> {
    upstream_certificate
        .map(|cert| {
            fs::read(cert)
                .map_err(|e| {
                    EdgeError::ClientCertificateError(CertificateError::RootCertificatesError(
                        format!("{e:?}"),
                    ))
                })
                .and_then(|bytes| {
                    reqwest::Certificate::from_pem(&bytes).map_err(|e| {
                        EdgeError::ClientCertificateError(CertificateError::RootCertificatesError(
                            format!("{e:?}"),
                        ))
                    })
                })
                .map(Some)
        })
        .unwrap_or(Ok(None))
}

pub fn config(tls_config: TlsOptions) -> Result<ServerConfig, EdgeError> {
    if let Err(err) = rustls::crypto::ring::default_provider().install_default() {
        info!(
            "Failed to install default crypto provider, this is likely because another system has already installed it: {:?}",
            err
        );
    }
    let mut cert_file = BufReader::new(
        File::open(
            tls_config
                .tls_server_cert
                .expect("No TLS server cert")
                .as_path(),
        )
            .map_err(|_| EdgeError::TlsError("Failed to open certfile".to_string()))?,
    );
    let mut key_file = BufReader::new(
        File::open(tls_config.tls_server_key.expect("No server key").as_path())
            .expect("Could not read cert file"),
    );
    let cert_chain = certs(&mut cert_file).filter_map(|f| f.ok()).collect();
    let mut keys: Vec<PrivateKeyDer> = pkcs8_private_keys(&mut key_file)
        .filter_map(|f| f.map(PrivateKeyDer::from).ok())
        .collect();
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, keys.remove(0))
        .map_err(|e| EdgeError::TlsError(format!("Failed to configure ServerConfig: {e:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn provider_installation_does_not_fail_if_already_installed_by_another_subsystem() {
        // setup a default provider and run it within our test
        let _ = rustls::crypto::ring::default_provider().install_default();
        // now we should be able to call config safely without failing but raising a warning instead
        let tls_options = TlsOptions {
            tls_server_cert: Some("../examples/server.crt".into()),
            tls_enable: true,
            tls_server_key: Some("../examples/server.key".into()),
            tls_server_port: 443,
        };

        let _ = config(tls_options);
    }
}
