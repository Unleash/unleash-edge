use std::collections::HashSet;
use std::env;
use std::pin::Pin;
use std::sync::Arc;

use dashmap::DashMap;
use lazy_static::lazy_static;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::trace;
use unleash_edge_feature_refresh::HydratorType;
use unleash_edge_http_client::UnleashClient;
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{
    EdgeResult, TokenCache, TokenType, TokenValidationStatus, ValidateTokensRequest,
};
use unleash_types::Upsert;

lazy_static! {
    pub static ref SHOULD_DEFER_VALIDATION: bool = {
        env::var("EDGE_DEFER_TOKEN_VALIDATION")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
    };
}

#[derive(Clone)]
pub struct TokenValidator {
    pub unleash_client: Arc<UnleashClient>,
    pub token_cache: Arc<TokenCache>,
    pub persistence: Option<Arc<dyn EdgePersistence>>,
    pub deferred_validation_tx: Option<UnboundedSender<String>>,
}

pub trait TokenRegister {
    fn register_token(&self, token: String) -> impl Future<Output = EdgeResult<EdgeToken>> + Send;
}

impl TokenRegister for TokenValidator {
    async fn register_token(&self, token: String) -> EdgeResult<EdgeToken> {
        Ok(self
            .register_tokens(vec![token])
            .await?
            .first()
            .expect("Couldn't validate token")
            .clone())
    }
}

pub fn create_revalidation_task(
    validator: &Arc<TokenValidator>,
    validation_interval_seconds: u64,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    let validator_clone = validator.clone();
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(validation_interval_seconds);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    let _ = validator_clone.revalidate_known_tokens().await;
                }
            }
        }
    })
}

pub fn create_revalidation_of_startup_tokens_task(
    validator: &Arc<TokenValidator>,
    tokens: Vec<String>,
    refresher: HydratorType,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    let validator = validator.clone();
    let mut tokens_not_yet_known: HashSet<String> =
        tokens.clone().into_iter().collect::<HashSet<String>>();
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(1);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                        trace!("{} tokens not yet validated", tokens_not_yet_known.len());
                        if tokens_not_yet_known.is_empty() {
                            break;
                        }
                        let tokens_not_yet_known_list = tokens_not_yet_known.iter().cloned().collect();
                        let token_result = validator.register_tokens(tokens_not_yet_known_list).await;
                        if let Ok(good_tokens) = token_result {
                            for token in good_tokens {
                                tokens_not_yet_known.remove(&token.token);
                                match &refresher {
                                    HydratorType::Polling(refresher) => {
                                        let _ = refresher.register_and_hydrate_token(&token).await;
                                    }
                                    HydratorType::Streaming(refresher) => {
                                        let _ = refresher.register_and_hydrate_token(&token).await;
                                    }
                                }
                            }
                        }
                }
            }
        }
    })
}

pub fn create_deferred_validation_task(
    validator: Arc<TokenValidator>,
    rx: UnboundedReceiver<String>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        validator.schedule_deferred_validation(rx).await;
    })
}

impl TokenValidator {
    pub fn new(
        unleash_client: Arc<UnleashClient>,
        token_cache: Arc<DashMap<String, EdgeToken>>,
        persistence: Option<Arc<dyn EdgePersistence>>,
    ) -> Self {
        TokenValidator {
            unleash_client,
            token_cache,
            persistence,
            deferred_validation_tx: None,
        }
    }

    pub fn new_lazy(
        unleash_client: Arc<UnleashClient>,
        token_cache: Arc<DashMap<String, EdgeToken>>,
        persistence: Option<Arc<dyn EdgePersistence>>,
        deferred_validation_tx: Option<UnboundedSender<String>>,
    ) -> Self {
        TokenValidator {
            unleash_client,
            token_cache,
            persistence,
            deferred_validation_tx,
        }
    }

    fn get_unknown_and_known_tokens(
        &self,
        tokens: Vec<String>,
    ) -> (Vec<EdgeToken>, Vec<EdgeToken>) {
        let tokens_with_valid_format: Vec<EdgeToken> = tokens
            .into_iter()
            .filter_map(|t| EdgeToken::try_from(t).ok())
            .collect();

        if tokens_with_valid_format.is_empty() {
            (vec![], vec![])
        } else {
            let mut tokens: Vec<EdgeToken> = vec![];
            for token in tokens_with_valid_format {
                let owned_token = self
                    .token_cache
                    .get(&token.token.clone())
                    .map(|t| t.value().clone())
                    .unwrap_or_else(|| token.clone());
                tokens.push(owned_token);
            }
            tokens.into_iter().partition(|t| t.token_type.is_none())
        }
    }

    pub fn deferred_token_registration(&self, tokens: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        let (unknown_tokens, known_tokens) = self.get_unknown_and_known_tokens(tokens);
        if unknown_tokens.is_empty() {
            Ok(known_tokens)
        } else {
            for token in unknown_tokens.iter() {
                trace!("Deferring token validation for {}", token.token);
                let invalid = EdgeToken {
                    status: TokenValidationStatus::Invalid,
                    token_type: Some(TokenType::Invalid),
                    ..token.clone()
                };
                self.token_cache
                    .insert(token.token.clone(), invalid.clone());

                if let Some(sender) = &self.deferred_validation_tx {
                    let _ = sender.send(token.token.clone());
                }
            }

            let updated_tokens = unknown_tokens.upsert(known_tokens);
            Ok(updated_tokens)
        }
    }

    pub async fn immediate_token_registration(
        &self,
        tokens: Vec<String>,
    ) -> EdgeResult<Vec<EdgeToken>> {
        let (unknown_tokens, known_tokens) = self.get_unknown_and_known_tokens(tokens);
        if unknown_tokens.is_empty() {
            Ok(known_tokens)
        } else {
            let token_strings_to_validate: Vec<String> =
                unknown_tokens.iter().map(|t| t.token.clone()).collect();

            let validation_result = self
                .unleash_client
                .validate_tokens(ValidateTokensRequest {
                    tokens: token_strings_to_validate,
                })
                .await?;
            let tokens_to_sink: Vec<EdgeToken> = unknown_tokens
                .into_iter()
                .map(|maybe_valid| {
                    if let Some(validated_token) = validation_result
                        .iter()
                        .find(|v| maybe_valid.token == v.token)
                    {
                        EdgeToken {
                            status: TokenValidationStatus::Validated,
                            ..validated_token.clone()
                        }
                    } else {
                        EdgeToken {
                            status: TokenValidationStatus::Invalid,
                            token_type: Some(TokenType::Invalid),
                            ..maybe_valid
                        }
                    }
                })
                .collect();
            tokens_to_sink.iter().for_each(|t| {
                self.token_cache.insert(t.token.clone(), t.clone());
            });
            let updated_tokens = tokens_to_sink.upsert(known_tokens);
            if let Some(persist) = self.persistence.clone() {
                let _ = persist.save_tokens(updated_tokens.clone()).await;
            }
            Ok(updated_tokens)
        }
    }

    pub async fn register_tokens(&self, tokens: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        if *SHOULD_DEFER_VALIDATION {
            self.deferred_token_registration(tokens)
        } else {
            self.immediate_token_registration(tokens).await
        }
    }

    pub async fn schedule_deferred_validation(&self, mut rx: UnboundedReceiver<String>) {
        let mut batch = HashSet::new();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(token) = rx.recv() => {
                    trace!("Received a token to validate");
                    batch.insert(token);
                },
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        trace!("We have a batch to validate");
                        let tokens: Vec<String> = batch.drain().collect();
                        match self.unleash_client.validate_tokens(ValidateTokensRequest { tokens }).await {
                            Ok(results) => {
                                for token in results.iter() {
                                    trace!("Background validated token: {}", token.token);
                                    self.token_cache.insert(token.token.clone(), token.clone());
                                }
                                if let Some(persist) = self.persistence.clone() {
                                    let _ = persist.save_tokens(results).await;
                                }
                            },
                            Err(e) => {
                                trace!("Background token validation failed: {:?}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    pub async fn revalidate_known_tokens(&self) -> EdgeResult<()> {
        let tokens_to_validate: Vec<String> = self
            .token_cache
            .iter()
            .filter(|t| t.value().status == TokenValidationStatus::Validated)
            .map(|e| e.key().clone())
            .collect();
        if !tokens_to_validate.is_empty() {
            let validation_result = self
                .unleash_client
                .validate_tokens(ValidateTokensRequest {
                    tokens: tokens_to_validate.clone(),
                })
                .await;

            if let Ok(valid_tokens) = validation_result {
                let invalid = tokens_to_validate
                    .into_iter()
                    .filter(|t| !valid_tokens.iter().any(|e| &e.token == t));
                for token in invalid {
                    self.token_cache
                        .entry(token)
                        .and_modify(|t| t.status = TokenValidationStatus::Invalid);
                }
            }
        }
        Ok(())
    }
}
