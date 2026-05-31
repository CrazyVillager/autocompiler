//! ratatui を用いた画面描画。`App` の状態を読み取って各ウィジェットへ反映する。
//! 配色は gruvbox（`autocc::theme`）に統一する。

use crate::app::App;
use autocc::detect::{CompilePlan, Lang};
use autocc::theme;
use ratatui::prelude::*;
use ratatui::widgets::{Block, List, ListItem, Paragraph, Wrap};

/// 枠付きブロックを gruvbox 配色で作る。
fn block(title: &str) -> Block<'static> {
    Block::bordered()
        .title(Span::styled(
            title.to_string(),
            Style::new().fg(theme::YELLOW).bold(),
        ))
        .border_style(Style::new().fg(theme::GRAY))
        .style(theme::base())
}

/// 言語ごとのアクセント色。
fn lang_color(lang: Lang) -> Color {
    match lang {
        Lang::C => theme::GREEN,
        Lang::Cpp => theme::BLUE,
        Lang::Cuda => theme::AQUA,
    }
}

/// 画面全体を描画する。
pub fn draw(frame: &mut Frame, app: &mut App) {
    // まず画面全体を背景色で塗る。
    frame.render_widget(Block::new().style(theme::base()), frame.area());

    let rows = Layout::vertical([
        Constraint::Length(1), // タイトル
        Constraint::Min(5),    // 本体
        Constraint::Length(1), // ステータス
        Constraint::Length(1), // キーヘルプ
    ])
    .split(frame.area());

    // タイトル右側に現在のビルド設定（最適化／規格／march／GPU）を表示する。
    let march = if app.config.march_native { " march" } else { "" };
    let gpu = match &app.config.cuda_arch {
        Some(a) => format!(" {a}"),
        None => String::new(),
    };
    let std = match app.selected_entry().and_then(|e| e.plan.as_ref().ok()).map(|p| p.lang) {
        Some(Lang::C) => format!(" {}", app.config.c_std.label()),
        Some(Lang::Cpp) => format!(" {}", app.config.cpp_std.label()),
        _ => String::new(),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " autocc — C/C++/CUDA オートコンパイラ",
                Style::new().fg(theme::FG).bold(),
            ),
            Span::styled(
                format!("   [{}{}{}{}]", app.config.opt.flag(), std, march, gpu),
                Style::new().fg(theme::AQUA),
            ),
        ]))
        .style(theme::base()),
        rows[0],
    );

    // 本体を左右に分割: ファイル一覧 / 詳細＋ログ。
    let body = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(rows[1]);
    draw_file_list(frame, app, body[0]);
    draw_detail(frame, app, body[1]);

    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(theme::base().fg(theme::YELLOW)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(" j/k 選択  Enter/c コンパイル  o 最適化  s 規格  m march  R 再スキャン  q 終了")
            .style(theme::base().fg(theme::GRAY)),
        rows[3],
    );
}

/// 左ペイン: 検出されたソースの一覧。
fn draw_file_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .entries
        .iter()
        .map(|e| {
            let name = e.path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
            let (label, color) = match &e.plan {
                Ok(p) => (p.lang.label(), lang_color(p.lang)),
                Err(_) => ("ERR", theme::RED),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{name}  "), Style::new().fg(theme::FG)),
                Span::styled(format!("[{label}]"), Style::new().fg(color)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .style(theme::base())
        .block(block("ソース"))
        .highlight_style(Style::new().bg(theme::YELLOW).fg(theme::BG).bold())
        .highlight_symbol("▶ ");
    // ListState を渡すことで選択ハイライトとスクロール追従が機能する。
    frame.render_stateful_widget(list, area, &mut app.list_state);
}

/// 右ペイン: 推定コンパイル設定（上）と、コンパイルの出力（下）。
fn draw_detail(frame: &mut Frame, app: &App, area: Rect) {
    let split = Layout::vertical([Constraint::Length(11), Constraint::Min(3)]).split(area);

    // --- 推定コンパイル設定 ---
    let plan_text: Vec<Line> = match app.selected_entry() {
        Some(entry) => match &entry.plan {
            Ok(p) => plan_lines(p),
            Err(e) => vec![Line::from(Span::styled(
                format!("解析エラー: {e}"),
                Style::new().fg(theme::RED),
            ))],
        },
        None => vec![Line::from("ソースが見つからない")],
    };
    frame.render_widget(
        Paragraph::new(plan_text)
            .block(block("推定コンパイル設定"))
            .style(theme::base())
            .wrap(Wrap { trim: false }),
        split[0],
    );

    // --- コンパイルログ ---
    let log: Vec<Line> = match &app.last_result {
        Some(r) => {
            let mut lines = vec![
                Line::from(Span::styled(
                    format!("$ {}", r.command),
                    Style::new().fg(theme::AQUA),
                )),
                Line::from(""),
            ];
            for l in r.stdout.lines() {
                lines.push(Line::from(l.to_string()));
            }
            // 失敗時はエラー出力を赤、成功時の警告は黄で示す。
            let err_color = if r.success { theme::YELLOW } else { theme::RED };
            for l in r.stderr.lines() {
                lines.push(Line::from(l.to_string()).style(Style::new().fg(err_color)));
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "（まだコンパイルしていない）",
            Style::new().fg(theme::GRAY),
        ))],
    };
    frame.render_widget(
        Paragraph::new(log)
            .block(block("出力"))
            .style(theme::base())
            .wrap(Wrap { trim: false }),
        split[1],
    );
}

/// 推定コンパイル設定を表示用の行に整形する。
fn plan_lines(p: &CompilePlan) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("言語       : {}", p.lang.label())),
        Line::from(format!("コンパイラ : {}", p.compiler)),
        Line::from(format!("出力       : {}", p.output)),
        Line::from(format!("コマンド   : {} {}", p.compiler, p.args.join(" "))),
        Line::from(""),
        Line::from(Span::styled("推定根拠:", Style::new().fg(theme::YELLOW))),
    ];
    for n in &p.notes {
        lines.push(Line::from(Span::styled(
            format!("  • {n}"),
            Style::new().fg(theme::GREEN),
        )));
    }
    lines
}
