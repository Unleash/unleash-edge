use rustls::pki_types::PrivateKeyDer;
use rustls::ServerConfig;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::path::PathBuf;
use std::{fs, fs::File, io::BufReader};

use crate::cli::TlsOptions;
use crate::error::{CertificateError, EdgeError};
use crate::types::EdgeResult;

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
    let config = ServerConfig::builder().with_no_client_auth();
    let mut cert_file = BufReader::new(
        File::open(
            tls_config
                .tls_server_cert
                .expect("No TLS server cert")
                .as_path(),
        )
        .map_err(|_| EdgeError::TlsError)?,
    );
    let mut key_file = BufReader::new(
        File::open(tls_config.tls_server_key.expect("No server key").as_path())
            .expect("Could not read cert file"),
    );
    let cert_chain = certs(&mut cert_file).filter_map(|f| f.ok()).collect();
    let mut keys: Vec<PrivateKeyDer> = pkcs8_private_keys(&mut key_file)
        .filter_map(|f| f.map(PrivateKeyDer::from).ok())
        .collect();
    config
        .with_single_cert(cert_chain, keys.remove(0))
        .map_err(|_e| EdgeError::TlsError)
}
