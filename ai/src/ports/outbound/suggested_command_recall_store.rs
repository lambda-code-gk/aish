//! suggested command recall cache の outbound port。

use crate::domain::SuggestedCommandCache;

#[derive(Debug, thiserror::Error)]
pub enum SuggestedCommandRecallStoreError {
    #[error("failed to read suggestion cache: {0}")]
    Read(String),
    #[error("failed to write suggestion cache: {0}")]
    Write(String),
}

pub trait SuggestedCommandRecallStore {
    fn load(&self) -> Result<Option<SuggestedCommandCache>, SuggestedCommandRecallStoreError>;
    fn save(&self, cache: &SuggestedCommandCache) -> Result<(), SuggestedCommandRecallStoreError>;
    fn cache_path(&self) -> &std::path::Path;
}
