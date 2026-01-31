//! タスク名のドメイン型

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskName(String);

impl TaskName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::ops::Deref for TaskName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for TaskName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
