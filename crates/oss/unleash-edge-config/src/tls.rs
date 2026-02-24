use reqwest::{ClientBuilder, Identity};
use rustls::ServerConfig;
use rustls::pki_types::PrivateKeyDer;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use tracing::{debug, info};
use unleash_edge_cli::{ClientIdentity, TlsOptions};
use unleash_edge_types::EdgeResult;
use unleash_edge_types::errors::{CertificateError, EdgeError};

fn load_pkcs12(id: &ClientIdentity) -> EdgeResult<Identity> {
    let p12_file = fs::read(id.pkcs12_identity_file.clone().unwrap()).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pkcs12ArchiveNotFound(format!("{e:?}")))
    })?;
    let p12_keystore =
        p12_keystore::KeyStore::from_pkcs12(&p12_file, &id.pkcs12_passphrase.clone().unwrap())
            .map_err(|e| {
                EdgeError::ClientCertificateError(CertificateError::Pkcs12ParseError(format!(
                    "{e:?}"
                )))
            })?;
    let mut pem = vec![];
    for (alias, entry) in p12_keystore.entries() {
        debug!("P12 entry: {alias}");
        match entry {
            p12_keystore::KeyStoreEntry::Certificate(_) => {
                info!(
                    "Direct Certificate, skipping. We want chain because client identity needs the private key"
                );
            }
            p12_keystore::KeyStoreEntry::PrivateKeyChain(chain) => {
                let key_pem = pkix::pem::der_to_pem(chain.key(), pkix::pem::PEM_PRIVATE_KEY);
                pem.extend(key_pem.as_bytes());
                pem.push(0x0a); // Added new line
                for cert in chain.chain() {
                    let cert_pem = pkix::pem::der_to_pem(cert.as_der(), pkix::pem::PEM_CERTIFICATE);
                    pem.extend(cert_pem.as_bytes());
                    pem.push(0x0a); // Added new line
                }
            }
            p12_keystore::KeyStoreEntry::Secret(_) => {
                info!(
                    "Direct secret, skipping. We want chain because client identity needs the private key"
                )
            }
        }
    }

    Identity::from_pem(&pem).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pkcs12X509Error(format!("{e:?}")))
    })
}

fn load_pkcs8_identity(id: &ClientIdentity) -> EdgeResult<Vec<u8>> {
    let cert = File::open(id.pkcs8_client_certificate_file.clone().unwrap()).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pem8ClientCertNotFound(format!("{e:}")))
    })?;
    let key = File::open(id.pkcs8_client_key_file.clone().unwrap()).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pem8ClientKeyNotFound(format!("{e:?}")))
    })?;
    let mut cert_reader = BufReader::new(cert);
    let mut key_reader = BufReader::new(key);
    let mut pem = vec![];
    let _ = key_reader.read_to_end(&mut pem);
    pem.push(0x0a);
    let _ = cert_reader.read_to_end(&mut pem);
    Ok(pem)
}

fn load_pkcs8(id: &ClientIdentity) -> EdgeResult<Identity> {
    Identity::from_pem(&load_pkcs8_identity(id)?).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pem8IdentityGeneration(format!(
            "{e:?}"
        )))
    })
}

fn load_pem_identity(pem_file: PathBuf) -> EdgeResult<Vec<u8>> {
    let mut pem = vec![];
    let mut pem_reader = BufReader::new(File::open(pem_file).expect("No such file"));
    let _ = pem_reader.read_to_end(&mut pem);
    Ok(pem)
}

fn load_pem(id: &ClientIdentity) -> EdgeResult<Identity> {
    Identity::from_pem(&load_pem_identity(id.pem_cert_file.clone().unwrap())?).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pem8IdentityGeneration(format!(
            "{e:?}"
        )))
    })
}

pub fn build_identity(tls: Option<ClientIdentity>) -> EdgeResult<ClientBuilder> {
    tls.map_or_else(
        || Ok(ClientBuilder::new().use_rustls_tls()),
        |tls| {
            let req_identity = if tls.pkcs12_identity_file.is_some() {
                // We're going to assume that we're using pkcs#12
                load_pkcs12(&tls)
            } else if tls.pkcs8_client_certificate_file.is_some() {
                load_pkcs8(&tls)
            } else if tls.pem_cert_file.is_some() {
                load_pem(&tls)
            } else {
                Err(EdgeError::ClientCertificateError(
                    CertificateError::NoCertificateFiles,
                ))
            };
            req_identity.map(|id| ClientBuilder::new().use_rustls_tls().identity(id))
        },
    )
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

pub fn build_upstream_certificate(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn provider_installation_does_not_fail_if_already_installed_by_another_subsystem() {
        // setup a default provider and run it within our test
        let _ = rustls::crypto::ring::default_provider().install_default();
        // now we should be able to call config safely without failing but raising a warning instead
        let tls_options = TlsOptions {
            tls_server_cert: Some("../../examples/server.crt".into()),
            tls_enable: true,
            tls_server_key: Some("../../examples/server.key".into()),
            tls_server_port: 443,
            redirect_http_to_https: false,
        };

        let _ = config(tls_options);
    }
}
