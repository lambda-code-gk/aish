//! PartId を生成する IdGenerator の標準実装（Clock + グローバルシーケンス）

use crate::domain::PartId;
use crate::ports::outbound::{Clock, IdGenerator};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static LAST_ID: AtomicU64 = AtomicU64::new(0);

const EPOCH_MS: u64 = 1577836800000; // 2020-01-01 00:00:00 UTC
const SEQ_BITS: u64 = 8;
const SEQ_MASK: u64 = (1 << SEQ_BITS) - 1; // 0..255
const BASE: u64 = 62;
const WIDTH: usize = 8;
const MAX_VAL: u64 = BASE.pow(WIDTH as u32) - 1;

/// 0-9, A-Z, a-z の順で辞書順＝数値順になるbase62
const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Clock + グローバルシーケンスで PartId を生成する標準実装
pub struct StdIdGenerator {
    clock: Arc<dyn Clock>,
}

impl StdIdGenerator {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock }
    }
}

impl IdGenerator for StdIdGenerator {
    fn next_id(&self) -> PartId {
        let ms = self.clock.now_ms();
        let ms_rel = ms.saturating_sub(EPOCH_MS);
        let base = (ms_rel << SEQ_BITS).min(MAX_VAL);

        loop {
            let prev = LAST_ID.load(Ordering::SeqCst);
            let next = if (prev >> SEQ_BITS) < ms_rel {
                base
            } else {
                let seq = (prev & SEQ_MASK) + 1;
                if seq > SEQ_MASK {
                    continue;
                }
                (prev + 1).min(MAX_VAL)
            };
            if LAST_ID.compare_exchange(prev, next, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return PartId::new(to_base62(next));
            }
        }
    }
}

fn to_base62(mut n: u64) -> String {
    let mut buf = [0u8; WIDTH];
    for i in (0..WIDTH).rev() {
        buf[i] = ALPHABET[(n % BASE) as usize];
        n /= BASE;
    }
    std::str::from_utf8(&buf).unwrap().to_string()
}
