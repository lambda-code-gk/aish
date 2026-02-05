//! ユーザークエリのドメイン型（LLM に送るメッセージ）

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query(String);

impl Query {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::ops::Deref for Query {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for Query {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
