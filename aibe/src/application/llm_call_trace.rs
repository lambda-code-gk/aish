//! LLM 呼び出しの latency trace ヘルパ（port 経由）。

use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use crate::ports::outbound::LlmCallTracer;

pub async fn trace_llm_result<T, E, F, Fut>(
    tracer: &Arc<dyn LlmCallTracer>,
    site: &'static str,
    profile: Option<&str>,
    f: F,
) -> Result<T, E>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    tracer.start(site, profile, None);
    let started = Instant::now();
    let result = f().await;
    tracer.end(site, started.elapsed().as_millis() as u64, result.is_ok());
    result
}
