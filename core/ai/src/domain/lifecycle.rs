//! ライフサイクルフック用のイベント型
//!
//! usecase が「いつ」フックを発火するかを表す。ハンドラは関心のあるイベントだけ処理する。

use common::domain::SessionDir;
use common::msg::Msg;
use std::path::PathBuf;

/// クエリ終了の種別（正常終了 or 上限到達で終了）
#[derive(Debug, Clone)]
pub enum QueryOutcome {
    /// 正常終了（LLM が Stop 等で終了）
    Done,
    /// 最大ターン数に達して終了（continue しなかった）
    ReachedLimit,
}

/// ライフサイクルイベント（発火タイミングとコンテキスト）
#[derive(Debug, Clone)]
pub enum LifecycleEvent {
    /// クエリが正常終了した直後（Done / ReachedLimit で continue しなかった）
    #[allow(dead_code)] // session_dir / outcome は将来のハンドラや拡張で利用
    QueryEnd {
        session_dir: SessionDir,
        memory_dir_project: Option<PathBuf>,
        memory_dir_global: PathBuf,
        outcome: QueryOutcome,
        /// 今回のクエリでやり取りしたメッセージ列（自己改善等で要約に利用）
        messages: Vec<Msg>,
    },
}
