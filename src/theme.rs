//! gruvbox 配色のパレット。autocc / autorun の TUI で共通利用する。
//!
//! 出典: gruvbox (Pavel Pertsev), dark 系の標準色。
//! https://github.com/morhetz/gruvbox

use ratatui::style::Color;

pub const BG: Color = Color::Rgb(0x28, 0x28, 0x28); // bg0  背景
pub const FG: Color = Color::Rgb(0xeb, 0xdb, 0xb2); // fg1  既定文字
pub const GRAY: Color = Color::Rgb(0x92, 0x83, 0x74); // gray ヘルプ・枠線
pub const RED: Color = Color::Rgb(0xfb, 0x49, 0x34); // bright red    エラー
pub const GREEN: Color = Color::Rgb(0xb8, 0xbb, 0x26); // bright green  C / 根拠
pub const YELLOW: Color = Color::Rgb(0xfa, 0xbd, 0x2f); // bright yellow 見出し・ハイライト
pub const BLUE: Color = Color::Rgb(0x83, 0xa5, 0x98); // bright blue   C++
pub const AQUA: Color = Color::Rgb(0x8e, 0xc0, 0x7c); // bright aqua   CUDA・コマンド
pub const ORANGE: Color = Color::Rgb(0xfe, 0x80, 0x19); // bright orange アクセント

/// 全ウィジェット共通の背景・前景スタイル。
pub fn base() -> ratatui::style::Style {
    ratatui::style::Style::new().bg(BG).fg(FG)
}
