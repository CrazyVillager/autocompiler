//! autocc — C / C++ / CUDA のオートコンパイラ（TUI）。
//!
//! 対象ディレクトリ内のソースを一覧表示し、`#include` 解析で推定した
//! コンパイラとフラグを使ってワンキーでコンパイルする（実行は autorun が担う）。
//!
//! 使い方:
//!   autocc          … カレントディレクトリを対象にする
//!   autocc <dir>    … 指定ディレクトリを対象にする

mod app;
mod ui;

use app::App;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::path::{Path, PathBuf};

fn main() -> std::io::Result<()> {
    // 第 1 引数があれば対象ディレクトリ。なければカレントディレクトリ。
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut app = App::new(&dir)?;

    // 端末を raw モード＆代替画面へ。run の戻り値に関わらず必ず復帰させる。
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &dir);
    ratatui::restore();
    result
}

/// 入力イベントを処理しつつ再描画を続けるメインループ。
fn run(terminal: &mut DefaultTerminal, app: &mut App, dir: &Path) -> std::io::Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;

        // イベントが来るまでブロックする（アニメーションは無いためポーリング不要）。
        if let Event::Key(key) = event::read()? {
            // キーの「押下」のみ処理する（Windows 等での離鍵イベントを無視）。
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.prev(),
                KeyCode::Enter | KeyCode::Char('c') => app.compile_selected(),
                KeyCode::Char('o') => app.cycle_opt(dir),
                KeyCode::Char('m') => app.toggle_march(dir),
                KeyCode::Char('s') => app.cycle_std(dir),
                KeyCode::Char('R') => app.refresh(dir),
                _ => {}
            }
        }
    }
    Ok(())
}
