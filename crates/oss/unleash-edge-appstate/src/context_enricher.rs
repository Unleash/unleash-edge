#[cfg(feature = "enterprise")]
pub use unleash_edge_context_enrichers::ContextEnricher;

#[cfg(not(feature = "enterprise"))]
mod disabled {
    use axum::http::HeaderMap;
    use std::time::Duration;
    use unleash_types::client_features::Context;

    #[derive(Clone, Default)]
    pub struct ContextEnricher;

    impl ContextEnricher {
        pub fn disabled() -> Self {
            Self
        }

        pub async fn try_enrich(&self, context: Context, _: &HeaderMap, _: Duration) -> Context {
            context
        }
    }
}

#[cfg(not(feature = "enterprise"))]
pub use disabled::ContextEnricher;
