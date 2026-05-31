//! autorun の画面描画。配色は gruvbox（`autocc::theme`）に統一する。

use crate::app::{App, BuildState, Entry};
use autocc::detect::Parallelism;
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

/// ビルド状態ごとの色。
fn state_color(s: BuildState) -> Color {
    match s {
        BuildState::Fresh => theme::GREEN,
        BuildState::Stale => theme::YELLOW,
        BuildState::Missing => theme::GRAY,
    }
}

/// 並列方式ごとのアクセント色（MPI を含むものはオレンジで強調）。
fn par_color(p: Parallelism) -> Color {
    if p.mpi {
        theme::ORANGE
    } else if p.openmp {
        theme::BLUE
    } else if p.pthread {
        theme::GREEN
    } else {
        theme::GRAY
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    frame.render_widget(Block::new().style(theme::base()), frame.area());

    let rows = Layout::vertical([
        Constraint::Length(1), // タイトル
        Constraint::Min(5),    // 本体
        Constraint::Length(1), // ステータス
        Constraint::Length(1), // ヘルプ
    ])
    .split(frame.area());

    // タイトル右側に現在の並列度とコア固定状態を表示する。
    let bind = if app.bind { "固定ON" } else { "固定OFF" };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" autorun — 並列実行ランチャ", Style::new().fg(theme::FG).bold()),
            Span::styled(
                format!("   [MPI -np {}  OMP {}  {bind}]", app.procs, app.threads),
                Style::new().fg(theme::AQUA),
            ),
        ]))
        .style(theme::base()),
        rows[0],
    );

    let body = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(rows[1]);
    draw_list(frame, app, body[0]);
    draw_detail(frame, app, body[1]);

    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(theme::base().fg(theme::YELLOW)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(" j/k 選択  Enter/r 実行  +/- スレッド  [/] プロセス  b コア固定  R 再スキャン  q 終了")
            .style(theme::base().fg(theme::GRAY)),
        rows[3],
    );
}

/// 左ペイン: 実行候補の一覧（並列方式・ビルド有無つき）。
fn draw_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .entries
        .iter()
        .map(|e| {
            let name = e.source.file_name().and_then(|s| s.to_str()).unwrap_or("?");
            let line = match e.state {
                // 最新: 名前は通常色、並列方式をアクセント色で。
                BuildState::Fresh => Line::from(vec![
                    Span::styled(format!("{name}  "), Style::new().fg(theme::FG)),
                    Span::styled(format!("[{}]", e.par.label()), Style::new().fg(par_color(e.par))),
                ]),
                // 要再ビルド: 黄で注意喚起。
                BuildState::Stale => Line::from(Span::styled(
                    format!("{name}  [{}] (要再ビルド)", e.par.label()),
                    Style::new().fg(theme::YELLOW),
                )),
                // 未ビルド: 灰色で淡く。
                BuildState::Missing => Line::from(Span::styled(
                    format!("{name}  [{}] (未ビルド)", e.par.label()),
                    Style::new().fg(theme::GRAY),
                )),
            };
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .style(theme::base())
        .block(block("実行候補"))
        .highlight_style(Style::new().bg(theme::YELLOW).fg(theme::BG).bold())
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, area, &mut app.list_state);
}

/// 右ペイン: 実行プレビュー（上）と実行出力（下）。
fn draw_detail(frame: &mut Frame, app: &App, area: Rect) {
    let split = Layout::vertical([Constraint::Length(10), Constraint::Min(3)]).split(area);

    let info: Vec<Line> = match app.selected_entry() {
        Some(e) => entry_lines(app, e),
        None => vec![Line::from("実行候補がない")],
    };
    frame.render_widget(
        Paragraph::new(info)
            .block(block("実行プレビュー"))
            .style(theme::base())
            .wrap(Wrap { trim: false }),
        split[0],
    );

    let log: Vec<Line> = match &app.last_result {
        Some(r) => {
            let mut lines = vec![
                Line::from(Span::styled(format!("$ {}", r.command), Style::new().fg(theme::AQUA))),
                Line::from(""),
            ];
            for l in r.stdout.lines() {
                lines.push(Line::from(l.to_string()));
            }
            let err_color = if r.success { theme::YELLOW } else { theme::RED };
            for l in r.stderr.lines() {
                lines.push(Line::from(l.to_string()).style(Style::new().fg(err_color)));
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "（まだ実行していない）",
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

/// 選択中エントリの実行プレビューを行へ整形する。
fn entry_lines(app: &App, e: &Entry) -> Vec<Line<'static>> {
    // 実行と同一のロジックでコマンドを組み立て、その表示行を流用する。
    let spec = app.build_spec(e);
    vec![
        Line::from(format!("ソース   : {}", e.source.display())),
        Line::from(format!("実行ファイル: {}", e.binary)),
        Line::from(vec![
            Span::styled("並列方式 : ", Style::new().fg(theme::FG)),
            Span::styled(e.par.label().to_string(), Style::new().fg(par_color(e.par))),
        ]),
        Line::from(vec![
            Span::styled("状態     : ", Style::new().fg(theme::FG)),
            Span::styled(e.state.label().to_string(), Style::new().fg(state_color(e.state))),
        ]),
        Line::from(vec![
            Span::styled("CSV 記録 : ", Style::new().fg(theme::FG)),
            if e.timing {
                Span::styled("対象（results.csv）", Style::new().fg(theme::GREEN))
            } else {
                Span::styled("対象外（計測ライブラリなし）", Style::new().fg(theme::GRAY))
            },
        ]),
        Line::from(""),
        Line::from(Span::styled(
            format!("実行コマンド: {}", spec.display),
            Style::new().fg(theme::AQUA),
        )),
    ]
}
