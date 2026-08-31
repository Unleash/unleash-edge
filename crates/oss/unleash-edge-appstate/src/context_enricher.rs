#[cfg(feature = "enterprise")]
pub use unleash_edge_context_enrichers::ContextEnricher;

#[cfg(not(feature = "enterprise"))]
mod disabled {
    use std::{collections::HashMap, time::Duration};
    use unleash_types::client_features::Context;

    #[derive(Clone, Default)]
    pub struct ContextEnricher;

    impl ContextEnricher {
        pub fn disabled() -> Self {
            Self
        }

        pub async fn enrich_or_original(
            &self,
            context: Context,
            _: HashMap<String, String>,
            _: Duration,
        ) -> Context {
            context
        }
    }
}

#[cfg(not(feature = "enterprise"))]
pub use disabled::ContextEnricher;
