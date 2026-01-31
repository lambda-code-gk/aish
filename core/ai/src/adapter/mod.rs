pub(crate) mod sinks;
pub(crate) mod task;
pub(crate) use sinks::StdoutSink;
pub(crate) use task::run_task_if_exists;
