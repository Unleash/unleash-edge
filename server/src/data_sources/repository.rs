#[derive(Debug, Clone, Default)]
pub struct Repository {
    storage: Storage,
}

pub const FEATURE_PREFIX: &str = "unleash-edge-feature-";
pub const TOKENS_KEY: &str = "unleash-edge-tokens";

impl Repository {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub fn get_features(&self) -> Result<Vec<Feature>, Error> {
        let features = self.storage.get()?;
        Ok(features)
    }

    pub fn sink_features(&self, features: Vec<Feature>) -> Result<(), Error> {
        self.storage.set(features)?;
        Ok(())
    }
}