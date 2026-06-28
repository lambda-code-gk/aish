//! contextual memory pack plugin 実装（Phase D）。

pub mod contextual_memory_pack;
pub mod memory_recipe_service;
pub mod memory_service;
pub mod memory_subscribe_service;
pub mod work_service;

pub use contextual_memory_pack::{
    contextual_pack_arc, contextual_pack_with_work_arc, ContextualMemoryPack,
};
pub use memory_recipe_service::MemoryRecipeService;
pub use memory_service::MemoryService;
pub use memory_subscribe_service::MemorySubscribeService;
pub use work_service::WorkService;
