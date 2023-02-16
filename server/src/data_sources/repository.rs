use std::sync::Arc;

use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use tokio::sync::RwLock;
use unleash_types::client_features::{ClientFeature, ClientFeatures};

use crate::types::{
    EdgeResult, EdgeToken, FeatureRefresh, FeatureSource, TokenSource, TokenValidationStatus,
};

trait ProjectFilter<T> {
    fn filter_by_projects(&self, token: &EdgeToken) -> Vec<T>;
}

impl ProjectFilter<ClientFeature> for Vec<ClientFeature> {
    fn filter_by_projects(&self, token: &EdgeToken) -> Vec<ClientFeature> {
        self.iter()
            .filter(|feature| {
                if let Some(feature_project) = &feature.project {
                    token.projects.contains(&"*".to_string())
                        || token.projects.contains(feature_project)
                } else {
                    false
                }
            })
            .cloned()
            .collect::<Vec<ClientFeature>>()
    }
}

#[derive(Clone)]
pub struct SourceFacade {
    pub(crate) token_source: Arc<RwLock<dyn DataSource>>,
    pub(crate) feature_source: Arc<RwLock<dyn DataSource>>,
}

#[derive(Clone)]
pub struct SinkFacade {
    token_sink: Arc<RwLock<dyn DataSink>>,
    feature_sink: Arc<RwLock<dyn DataSink>>,
}

#[async_trait]
pub trait DataSource: Send + Sync {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    async fn get_token(&self, secret: &str) -> EdgeResult<Option<EdgeToken>>;
    async fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<FeatureRefresh>>;
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<Option<ClientFeatures>>;
}

#[async_trait]
pub trait DataSink: Send + Sync {
    async fn sink_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()>;
    async fn sink_features(
        &self,
        token: &EdgeToken,
        features: ClientFeatures,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()>;
}

#[async_trait]
impl TokenSource for SourceFacade {
    async fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let lock = self.token_source.read().await;
        lock.get_tokens().await
    }

    async fn get_valid_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let lock = self.token_source.read().await;
        lock.get_tokens().await.map(|result| {
            result
                .iter()
                .filter(|t| t.status == TokenValidationStatus::Validated)
                .cloned()
                .collect()
        })
    }

    async fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        let lock = self.token_source.read().await;
        lock.get_token(secret.as_str()).await
    }

    async fn filter_valid_tokens(&self, tokens: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        let lock = self.token_source.read().await;
        let mut known_tokens = lock.get_tokens().await?;
        drop(lock);
        known_tokens.retain(|t| tokens.contains(&t.token));
        Ok(known_tokens)
    }

    async fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<FeatureRefresh>> {
        let lock = self.token_source.read().await;
        lock.get_tokens_due_for_refresh().await
    }
}

#[async_trait]
impl FeatureSource for SourceFacade {
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        let token = self
            .token_details(token.token.clone())
            .await?
            .unwrap_or(token.clone());
        let environment_features = self
            .feature_source
            .read()
            .await
            .get_client_features(&token)
            .await
            .unwrap();
        Ok(environment_features
            .map(|client_features| ClientFeatures {
                features: client_features.features.filter_by_projects(&token),
                ..client_features
            })
            .unwrap())
    }
}
