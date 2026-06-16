//! smart feature registry の読み込み outbound port。

use crate::domain::{FeatureRegistry, FeatureRegistryError};

pub trait FeatureRegistryLoader: Send + Sync {
    fn load(&self) -> Result<FeatureRegistry, FeatureRegistryError>;
}
