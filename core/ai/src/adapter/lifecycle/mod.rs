//! ライフサイクルフックのアダプタ（コンポジット + 個別ハンドラ）

mod composite;
mod self_improve;

pub(crate) use composite::{CompositeLifecycleHooks, LifecycleHandler};
pub(crate) use self_improve::SelfImproveHandler;
