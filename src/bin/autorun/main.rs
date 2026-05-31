//! autorun — 並列実行ランチャ（TUI）。
//!
//! autocc が出力した実行ファイルを、ソースから判定した並列方式に応じて
//! mpirun / OMP_NUM_THREADS / 直接実行のいずれかで起動する。
//!
//! 使い方:
//!   autorun          … カレントディレクトリを対象にする
//!   autorun <dir>    … 指定ディレクトリを対象にする

mod app;
mod ui;

use app::App;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut app = App::new(&dir)?;

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.prev(),
                KeyCode::Enter | KeyCode::Char('r') => app.run_selected(),
                // スレッド数の増減（'=' は Shift なしの '+' キー）。
                KeyCode::Char('+') | KeyCode::Char('=') => app.adjust_threads(1),
                KeyCode::Char('-') => app.adjust_threads(-1),
                // プロセス数の増減。
                KeyCode::Char(']') => app.adjust_procs(1),
                KeyCode::Char('[') => app.adjust_procs(-1),
                KeyCode::Char('b') => app.toggle_bind(),
                KeyCode::Char('R') => app.refresh(),
                _ => {}
            }
        }
    }
    Ok(())
}
