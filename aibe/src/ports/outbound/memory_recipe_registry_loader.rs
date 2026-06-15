//! effective MemoryRecipeRegistry の読み込み port。

use crate::domain::{MemoryRecipeRegistry, MemoryRecipeRegistryError};

pub trait MemoryRecipeRegistryLoader: Send + Sync {
    fn load_strict(&self) -> Result<MemoryRecipeRegistry, MemoryRecipeRegistryError>;
    fn load_best_effort(&self) -> MemoryRecipeRegistry;
}
