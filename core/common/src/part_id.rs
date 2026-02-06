//! Part ID生成: 固定長ASCII・辞書順＝時系列・同一ms内単調増加
//!
//! 形式: base62(0-9,A-Z,a-z) 8文字。値 = (ms since 2020-01-01)<<8 | seq(0..255)。辞書順＝数値順。
//! 既存の part_&lt;ID&gt;_user.txt / part_&lt;ID&gt;_assistant.txt のID部分のみこの形式に置き換える。
//!
//! 互換: 旧形式（base64url 8文字等）と混在してもファイル名順ソートは安定するが、
//! 辞書順が時系列に一致するのは新形式のみ。混在時は時系列順を保証しない。
//!
//! usecase は IdGenerator を注入し、テストでは固定 ID を返す実装を渡せる。
//! 標準実装（StdIdGenerator）は common::adapter に定義。

use std::sync::Arc;

use crate::domain::PartId;
pub use crate::ports::outbound::IdGenerator;

/// 標準実装の re-export（定義は common::adapter）
pub use crate::adapter::StdIdGenerator;

impl PartId {
    /// 新規Part IDを1つ生成する（デフォルトの StdIdGenerator を使用）
    pub fn generate() -> Self {
        crate::adapter::StdIdGenerator::new(Arc::new(crate::adapter::StdClock)).next_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_id_fixed_length_ascii() {
        let id = PartId::generate();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn part_id_lexicographic_monotonic_consecutive() {
        let ids: Vec<PartId> = (0..10).map(|_| PartId::generate()).collect();
        let mut sorted = ids.clone();
        sorted.sort_by(|a, b| (**a).cmp(&**b));
        assert_eq!(ids, sorted, "sort() must preserve generation order");
    }

    #[test]
    fn part_id_same_ms_monotonic() {
        let ids: Vec<PartId> = (0..50).map(|_| PartId::generate()).collect();
        let mut sorted = ids.clone();
        sorted.sort_by(|a, b| (**a).cmp(&**b));
        assert_eq!(ids, sorted, "rapid-fire IDs must be lexicographically monotonic");
    }
}
