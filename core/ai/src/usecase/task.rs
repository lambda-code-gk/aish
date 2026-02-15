//! タスク実行ユースケース（タスクがあれば実行、なければクエリに委譲）

use crate::domain::{Query, TaskName};
use crate::ports::outbound::{RunQuery, TaskRunner};
use common::domain::{ModelName, ProviderName, SessionDir};
use common::error::Error;
use std::sync::Arc;

/// タスク実行のユースケース
///
/// タスクスクリプトが存在すれば実行し、存在しなければ RunQuery に委譲する。
pub struct TaskUseCase {
    task_runner: Arc<dyn TaskRunner>,
    run_query: Arc<dyn RunQuery>,
}

impl TaskUseCase {
    pub fn new(task_runner: Arc<dyn TaskRunner>, run_query: Arc<dyn RunQuery>) -> Self {
        Self {
            task_runner,
            run_query,
        }
    }

    /// 利用可能なタスク名一覧を返す（補完用）。
    pub fn list_names(&self) -> Result<Vec<String>, Error> {
        self.task_runner.list_names()
    }

    /// タスクを実行する。タスクが存在しなければクエリとして LLM に送る。
    pub fn run(
        &self,
        session_dir: Option<SessionDir>,
        name: &TaskName,
        args: &[String],
        provider: Option<ProviderName>,
        model: Option<ModelName>,
        system_instruction: Option<&str>,
        tool_allowlist: Option<&[String]>,
    ) -> Result<i32, Error> {
        if let Some(code) = self.task_runner.run_if_exists(name.as_ref(), args)? {
            return Ok(code);
        }
        let mut query_parts = vec![name.as_ref().to_string()];
        query_parts.extend(args.iter().cloned());
        let query = Query::new(query_parts.join(" "));
        if query.trim().is_empty() {
            return Err(Error::invalid_argument(
                "No query provided. Please provide a message to send to the LLM.",
            ));
        }
        self.run_query.run_query(
            session_dir,
            provider,
            model,
            Some(&query),
            system_instruction,
            None,
            tool_allowlist,
        )
    }
}
