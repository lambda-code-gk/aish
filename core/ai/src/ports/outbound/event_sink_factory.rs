//! イベント Sink を生成する Outbound ポート
//!
//! run_query ごとに新しい Sink 列を取得するために使用する。

use common::sink::EventSink;

/// イベント Sink の列を生成する（呼び出しごとに新しいインスタンスを返してよい）
pub trait EventSinkFactory: Send + Sync {
    fn create_sinks(&self) -> Vec<Box<dyn EventSink>>;
}
