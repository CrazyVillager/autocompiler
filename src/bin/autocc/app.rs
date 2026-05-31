//! TUI アプリケーションの状態と状態遷移ロジック。
//!
//! 描画（`ui`）や入力読み取り（`main`）からは独立させ、ここでは
//! 「何が選ばれていて、操作の結果どう変わるか」だけを扱う。

use autocc::compile::{self, RunResult};
use autocc::detect::{BuildConfig, CompilePlan, Lang, plan_for};
use ratatui::widgets::ListState;
use std::path::{Path, PathBuf};

/// 一覧に並ぶ 1 エントリ。ソースと、その推定結果（または解析エラー）を持つ。
pub struct Entry {
    pub path: PathBuf,
    pub plan: Result<CompilePlan, String>,
}

/// アプリ全体の状態。
pub struct App {
    pub entries: Vec<Entry>,
    pub list_state: ListState,    // 選択位置とスクロール位置の保持に利用
    pub config: BuildConfig,      // 全エントリへ一律適用するビルド設定
    pub last_result: Option<RunResult>,
    pub status: String,
    pub should_quit: bool,
}

impl App {
    /// 指定ディレクトリを走査して初期状態を構築する。
    pub fn new(dir: &Path) -> std::io::Result<App> {
        // 起動時に実機 GPU のアーキを検出して既定設定に組み込む。
        let cuda_arch = compile::detect_cuda_arch();
        let config = BuildConfig {
            cuda_arch: cuda_arch.clone(),
            ..BuildConfig::default()
        };
        let entries = scan(dir, &config)?;
        let mut list_state = ListState::default();
        if !entries.is_empty() {
            list_state.select(Some(0));
        }
        let gpu = match &cuda_arch {
            Some(a) => format!("GPU -arch={a}"),
            None => "GPU 未検出".into(),
        };
        let status = format!("{} 件のソースを検出（{}） / {gpu}", entries.len(), dir.display());
        Ok(App {
            entries,
            list_state,
            config,
            last_result: None,
            status,
            should_quit: false,
        })
    }

    /// 現在選択中のエントリ。
    pub fn selected_entry(&self) -> Option<&Entry> {
        self.list_state.selected().and_then(|i| self.entries.get(i))
    }

    /// 次のエントリへ（末尾で先頭に戻る）。
    pub fn next(&mut self) {
        self.move_selection(1);
    }

    /// 前のエントリへ（先頭で末尾に回る）。
    pub fn prev(&mut self) {
        self.move_selection(-1);
    }

    /// 選択位置を相対移動する内部ヘルパ。
    fn move_selection(&mut self, delta: isize) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.list_state.select(Some(next));
    }

    /// 選択中ソースをコンパイルする。
    pub fn compile_selected(&mut self) {
        // ライフタイム衝突を避けるため、計画を複製してから可変借用に入る。
        let plan = match self.current_plan() {
            Ok(p) => p,
            Err(msg) => {
                self.status = msg;
                return;
            }
        };
        let result = compile::compile(&plan);
        self.status = if result.success {
            format!("コンパイル成功 → {} ({} ms)", plan.output, result.elapsed_ms)
        } else {
            format!("コンパイル失敗: {}", plan.source.display())
        };
        self.last_result = Some(result);
    }

    /// 選択中ソースの言語に応じて C / C++ 規格を循環切替する。
    pub fn cycle_std(&mut self, dir: &Path) {
        let lang = self
            .selected_entry()
            .and_then(|e| e.plan.as_ref().ok())
            .map(|p| p.lang);
        match lang {
            Some(Lang::C) => self.config.c_std = self.config.c_std.next(),
            Some(Lang::Cpp) => self.config.cpp_std = self.config.cpp_std.next(),
            _ => {
                self.status = "規格切替は C / C++ ソースのみ対象".into();
                return;
            }
        }
        self.refresh(dir);
        self.status = format!(
            "規格: C={} / C++={}",
            self.config.c_std.label(),
            self.config.cpp_std.label()
        );
    }

    /// 最適化レベルを循環切替し、推定設定へ即座に反映する。
    pub fn cycle_opt(&mut self, dir: &Path) {
        self.config.opt = self.config.opt.next();
        self.refresh(dir); // plan を現在の設定で作り直す
        self.status = format!("最適化レベル: {}", self.config.opt.flag());
    }

    /// -march=native の ON/OFF を切り替える。
    pub fn toggle_march(&mut self, dir: &Path) {
        self.config.march_native = !self.config.march_native;
        self.refresh(dir);
        self.status = format!(
            "-march=native: {}",
            if self.config.march_native { "ON" } else { "OFF" }
        );
    }

    /// ディレクトリを再走査して一覧を更新する。
    pub fn refresh(&mut self, dir: &Path) {
        match scan(dir, &self.config) {
            Ok(entries) => {
                self.entries = entries;
                // 選択位置が範囲外になったら補正する。
                let sel = match self.list_state.selected() {
                    Some(i) if i < self.entries.len() => Some(i),
                    _ if self.entries.is_empty() => None,
                    _ => Some(0),
                };
                self.list_state.select(sel);
                self.status = format!("再スキャン完了: {} 件", self.entries.len());
            }
            Err(e) => self.status = format!("再スキャン失敗: {e}"),
        }
    }

    /// 選択中エントリの計画を複製して取り出す（無選択／解析エラーは `Err`）。
    fn current_plan(&self) -> Result<CompilePlan, String> {
        match self.selected_entry() {
            Some(entry) => match &entry.plan {
                Ok(p) => Ok(p.clone()),
                Err(e) => Err(format!("解析エラー: {e}")),
            },
            None => Err("ソースが選択されていない".into()),
        }
    }
}

/// ディレクトリ直下の対応ソースを集め、パス順に並べて返す。
fn scan(dir: &Path, cfg: &BuildConfig) -> std::io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for ent in std::fs::read_dir(dir)? {
        let path = ent?.path();
        if !path.is_file() {
            continue;
        }
        if Lang::from_path(&path).is_none() {
            continue;
        }
        let plan = plan_for(&path, cfg);
        entries.push(Entry { path, plan });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
