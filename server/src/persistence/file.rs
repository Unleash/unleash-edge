use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::{path::PathBuf, str::FromStr};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use unleash_types::client_features::ClientFeatures;

use crate::types::EdgeToken;
use crate::{
    error::EdgeError,
    types::{EdgeResult, TokenRefresh},
};

use super::EdgePersistence;

pub struct FilePersister {
    pub storage_path: PathBuf,
}

impl TryFrom<&str> for FilePersister {
    type Error = EdgeError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        PathBuf::from_str(value)
            .map(|path| Self { storage_path: path })
            .map_err(|_e| {
                EdgeError::PersistenceError(format!("Could not build a path from {value}"))
            })
    }
}

impl FilePersister {
    pub fn token_path(&self) -> PathBuf {
        let mut token_path = self.storage_path.clone();
        token_path.push("unleash_tokens.json");
        token_path
    }

    pub fn features_path(&self) -> PathBuf {
        let mut features_path = self.storage_path.clone();
        features_path.push("unleash_features.json");
        features_path
    }

    pub fn refresh_target_path(&self) -> PathBuf {
        let mut refresh_target_path = self.storage_path.clone();
        refresh_target_path.push("unleash_refresh_targets.json");
        refresh_target_path
    }

    pub fn new(storage_path: &Path) -> Self {
        let _ = std::fs::create_dir_all(storage_path);
        FilePersister {
            storage_path: storage_path.to_path_buf(),
        }
    }
}

#[async_trait]
impl EdgePersistence for FilePersister {
    async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let mut file = tokio::fs::File::open(self.token_path())
            .await
            .map_err(|_| {
                EdgeError::PersistenceError(
                    "Cannot load tokens from backup, opening backup file failed".to_string(),
                )
            })?;

        let mut contents = vec![];

        file.read_to_end(&mut contents).await.map_err(|_| {
            EdgeError::PersistenceError(
                "Cannot load tokens from backup, reading backup file failed".to_string(),
            )
        })?;
        serde_json::from_slice(&contents).map_err(|_| {
            EdgeError::PersistenceError(
                "Cannot load tokens from backup, parsing backup file failed".to_string(),
            )
        })
    }

    async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        let mut file = tokio::fs::File::create(self.token_path())
            .await
            .map_err(|_| {
                EdgeError::PersistenceError(
                    "Cannot write tokens to backup. Opening backup file for writing failed"
                        .to_string(),
                )
            })?;
        file.write_all(
            &serde_json::to_vec(&tokens).map_err(|_| {
                EdgeError::PersistenceError("Failed to serialize tokens".to_string())
            })?,
        )
        .await
        .map_err(|_| EdgeError::PersistenceError("Could not serialize tokens to disc".to_string()))
        .map(|_| ())
    }

    async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>> {
        let mut file = tokio::fs::File::open(self.features_path())
            .await
            .map_err(|_| {
                EdgeError::PersistenceError(
                    "Cannot load features from backup, opening backup file failed".to_string(),
                )
            })?;

        let mut contents = vec![];

        file.read_to_end(&mut contents).await.map_err(|_| {
            EdgeError::PersistenceError(
                "Cannot load features from backup, reading backup file failed".to_string(),
            )
        })?;
        let contents: Vec<(String, ClientFeatures)> =
            serde_json::from_slice(&contents).map_err(|_| {
                EdgeError::PersistenceError(
                    "Cannot load features from backup, parsing backup file failed".to_string(),
                )
            })?;
        Ok(contents.into_iter().collect())
    }

    async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()> {
        let mut file = tokio::fs::File::create(self.features_path())
            .await
            .map_err(|_| {
                EdgeError::PersistenceError(
                    "Cannot write tokens to backup. Opening backup file for writing failed"
                        .to_string(),
                )
            })?;
        file.write_all(
            &serde_json::to_vec(&features).map_err(|_| {
                EdgeError::PersistenceError("Failed to serialize features".to_string())
            })?,
        )
        .await
        .map_err(|_| EdgeError::PersistenceError("Could not serialize tokens to disc".to_string()))
        .map(|_| ())
    }

    async fn load_refresh_targets(&self) -> EdgeResult<Vec<TokenRefresh>> {
        let mut file = tokio::fs::File::open(self.refresh_target_path())
            .await
            .map_err(|_| {
                EdgeError::PersistenceError(
                    "Cannot load tokens from backup, opening backup file failed".to_string(),
                )
            })?;

        let mut contents = vec![];

        file.read_to_end(&mut contents).await.map_err(|_| {
            EdgeError::PersistenceError(
                "Cannot load tokens from backup, reading backup file failed".to_string(),
            )
        })?;
        serde_json::from_slice(&contents).map_err(|_| {
            EdgeError::PersistenceError(
                "Cannot load tokens from backup, parsing backup file failed".to_string(),
            )
        })
    }
    async fn save_refresh_targets(&self, refresh_targets: Vec<TokenRefresh>) -> EdgeResult<()> {
        let mut file = tokio::fs::File::create(self.refresh_target_path())
            .await
            .map_err(|_| {
                EdgeError::PersistenceError(
                    "Cannot write tokens to backup. Opening backup file for writing failed"
                        .to_string(),
                )
            })?;

        file.write_all(&serde_json::to_vec(&refresh_targets).map_err(|_| {
            EdgeError::PersistenceError("Failed to serialize refresh tokens".to_string())
        })?)
        .await
        .map_err(|_| EdgeError::PersistenceError("Could not serialize tokens to disc".to_string()))
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::env::temp_dir;

    use actix_web::http::header::EntityTag;
    use chrono::Utc;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};

    use crate::persistence::file::FilePersister;
    use crate::persistence::EdgePersistence;
    use crate::types::{EdgeToken, TokenRefresh, TokenType, TokenValidationStatus};

    #[tokio::test]
    async fn file_persister_can_save_and_load_features() {
        let persister = FilePersister::try_from(temp_dir().to_str().unwrap()).unwrap();
        let client_features = ClientFeatures {
            features: vec![
                ClientFeature {
                    name: "test1".to_string(),
                    enabled: true,
                    strategies: Some(vec![]),
                    variants: None,
                    project: Some("default".to_string()),
                    feature_type: Some("experiment".to_string()),
                    description: Some("For testing".to_string()),
                    created_at: None,
                    last_seen_at: None,
                    stale: Some(false),
                    impression_data: Some(false),
                    dependencies: None,
                },
                ClientFeature {
                    name: "test2".to_string(),
                    ..ClientFeature::default()
                },
            ],
            version: 2,
            segments: None,
            query: None,
        };

        let formatted_data = vec![("some-environment".into(), client_features)];

        persister
            .save_features(formatted_data.clone())
            .await
            .unwrap();
        let reloaded = persister.load_features().await.unwrap();
        assert_eq!(reloaded, formatted_data.into_iter().collect());
    }

    #[tokio::test]
    async fn file_persister_can_save_and_load_refresh_targets() {
        let persister = FilePersister::try_from(temp_dir().to_str().unwrap()).unwrap();
        let tokens = vec![
            TokenRefresh {
                token: EdgeToken {
                    token: "default:development:ajsdkajnsdlsan".into(),
                    token_type: Some(TokenType::Client),
                    environment: Some("development".into()),
                    projects: vec!["default".into()],
                    status: TokenValidationStatus::Validated,
                },
                etag: Some(EntityTag::new_weak("1234".into())),
                next_refresh: None,
                last_refreshed: Some(Utc::now()),
                last_check: Some(Utc::now()),
                failure_count: 0,
                use_client_bulk_endpoint: false,
            },
            TokenRefresh {
                token: EdgeToken {
                    token: "otherthing:otherthing:aljjsdnasd".into(),
                    ..EdgeToken::default()
                },
                etag: None,
                next_refresh: None,
                last_refreshed: None,
                last_check: None,
                failure_count: 0,
                use_client_bulk_endpoint: false,
            },
        ];

        persister
            .save_refresh_targets(tokens.clone())
            .await
            .unwrap();

        let reloaded = persister.load_refresh_targets().await.unwrap();

        assert_eq!(reloaded, tokens);
    }

    #[tokio::test]
    async fn file_persister_can_save_and_load_tokens() {
        let persister = FilePersister::try_from(temp_dir().to_str().unwrap()).unwrap();
        let tokens = vec![
            EdgeToken {
                token: "default:development:ajsdkajnsdlsan".into(),
                token_type: Some(TokenType::Client),
                environment: Some("development".into()),
                projects: vec!["default".into()],
                status: TokenValidationStatus::Validated,
            },
            EdgeToken {
                token: "otherthing:otherthing:aljjsdnasd".into(),
                ..EdgeToken::default()
            },
        ];

        persister.save_tokens(tokens.clone()).await.unwrap();

        let reloaded = persister.load_tokens().await.unwrap();

        assert_eq!(reloaded, tokens);
    }

    #[test]
    fn can_read_token_refresh_without_use_client_bulk_field() {
        let json = r#"{
            "token": {
                "token": "default:development:ajsdkajnsdlsan",
                "token_type": "client",
                "environment": "development",
                "projects": [
                    "default"
                ],
                "status": "Validated"
            },
            "etag": "W/\"1234\"",
            "next_refresh": null,
            "last_refreshed": "2021-03-09T13:00:00Z",
            "last_check": "2021-03-09T13:00:00Z",
            "failure_count": 0
        }"#;

        let token_refresh: TokenRefresh = serde_json::from_str(json).unwrap();

        assert_eq!(token_refresh.use_client_bulk_endpoint, false);
    }
}
