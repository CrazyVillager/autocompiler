//! autocc / autorun が共有するコアロジック。
//!
//! - `detect` … 言語判定・`#include` 解析・コンパイル計画／並列種別の推定（純粋ロジック）
//! - `compile` … コンパイルと実行（外部プロセス起動）

pub mod compile;
pub mod detect;
pub mod theme;
