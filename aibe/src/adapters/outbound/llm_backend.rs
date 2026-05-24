//! LLM backend 単位の HTTP 接続コンテキスト（プロファイル間で共有）。

use std::sync::Arc;

use reqwest::Client;

#[derive(Clone)]
pub struct HttpBackendContext {
    pub client: Client,
    pub base_url: String,
    pub api_key: String,
}

impl HttpBackendContext {
    pub fn new(base_url: String, api_key: String) -> Arc<Self> {
        Arc::new(Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        })
    }
}
