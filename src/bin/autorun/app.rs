//! autorun の状態と状態遷移ロジック。
//!
//! ソースを解析して並列方式を判定し、対応する実行ファイルを
//! その方式にふさわしい方法（mpirun / OMP_NUM_THREADS / 直接）で起動する。

use autocc::compile::{self, RunResult};
use autocc::detect::{Lang, Parallelism, analyze_runtime};
use ratatui::widgets::ListState;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 実行ファイルのビルド状態（ソースとの更新時刻の比較で判定する）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildState {
    Missing, // 実行ファイルが存在しない
    Stale,   // 実行ファイルはあるが、ソースより古い（要再ビルド）
    Fresh,   // 実行ファイルがソースより新しい（最新）
}

impl BuildState {
    pub fn label(&self) -> &'static str {
        match self {
            BuildState::Missing => "未ビルド",
            BuildState::Stale => "要再ビルド",
            BuildState::Fresh => "最新",
        }
    }
}

/// 一覧に並ぶ 1 エントリ。ソースと、対応する実行ファイルの情報を持つ。
pub struct Entry {
    pub source: PathBuf,    // 元ソース
    pub binary: String,     // 期待される実行ファイル名（ソースのステム）
    pub state: BuildState,  // 実行ファイルのビルド状態
    pub par: Parallelism,   // ソースから判定した並列方式
    pub timing: bool,       // 計測ライブラリを使っているか（CSV 記録対象か）
}

/// 実行コマンドの仕様。実行（run_selected）とプレビュー（ui）で共用する。
pub struct RunSpec {
    pub program: String,             // 起動するプログラム（mpirun または ./bin）
    pub args: Vec<String>,           // 引数
    pub envs: Vec<(String, String)>, // 環境変数
    pub stdin: Option<PathBuf>,      // 標準入力に接続するファイル（<binary>.in）
    pub display: String,             // 表示用コマンド行
}

/// アプリ全体の状態。
pub struct App {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub list_state: ListState,
    pub procs: usize,   // MPI プロセス数（-np）
    pub threads: usize, // OpenMP スレッド数（OMP_NUM_THREADS）
    pub bind: bool,     // コア固定（--bind-to core / OMP_PROC_BIND）を行うか
    pub last_result: Option<RunResult>,
    pub status: String,
    pub should_quit: bool,
}

impl App {
    /// 指定ディレクトリを走査して初期状態を構築する。
    pub fn new(dir: &Path) -> std::io::Result<App> {
        let entries = scan(dir)?;
        let mut list_state = ListState::default();
        if !entries.is_empty() {
            list_state.select(Some(0));
        }
        // 既定の並列度は論理コア数に合わせる。
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let status = format!("{} 件のソースを検出（{}）", entries.len(), dir.display());
        Ok(App {
            dir: dir.to_path_buf(),
            entries,
            list_state,
            procs: cores,
            threads: cores,
            bind: true, // ベンチで固定し忘れを防ぐため既定 ON
            last_result: None,
            status,
            should_quit: false,
        })
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.list_state.selected().and_then(|i| self.entries.get(i))
    }

    pub fn next(&mut self) {
        self.move_selection(1);
    }

    pub fn prev(&mut self) {
        self.move_selection(-1);
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len as isize) as usize;
        self.list_state.select(Some(next));
    }

    /// MPI プロセス数（-np）を増減する。
    pub fn adjust_procs(&mut self, delta: isize) {
        self.procs = (self.procs as isize + delta).max(1) as usize;
        self.status = format!("MPI プロセス数 -np {}", self.procs);
    }

    /// OpenMP スレッド数（OMP_NUM_THREADS）を増減する。
    pub fn adjust_threads(&mut self, delta: isize) {
        self.threads = (self.threads as isize + delta).max(1) as usize;
        self.status = format!("OMP_NUM_THREADS = {}", self.threads);
    }

    /// コア固定（--bind-to core / OMP_PROC_BIND）の ON/OFF を切り替える。
    pub fn toggle_bind(&mut self) {
        self.bind = !self.bind;
        self.status = format!("コア固定: {}", if self.bind { "ON" } else { "OFF" });
    }

    /// エントリの実行コマンドを組み立てる（実行とプレビューで共用）。
    ///
    /// MPI / OpenMP / ハイブリッドを判定し、コア固定が ON ならベンチ向けに
    /// プロセス・スレッドを物理コアへ割り付ける。
    pub fn build_spec(&self, e: &Entry) -> RunSpec {
        // 実行パスは対象ディレクトリ基準。"./" を付けて PATH 探索を避ける。
        let bin_path = format!("./{}", self.dir.join(&e.binary).to_string_lossy());
        let p = e.par;

        let mut program = bin_path.clone();
        let mut args: Vec<String> = Vec::new();
        let mut envs: Vec<(String, String)> = Vec::new();

        if p.mpi {
            // MPI（単体、または OpenMP ハイブリッド）。ローカル起動なら
            // mpirun の環境変数は各ランクへ継承される。
            program = "mpirun".to_string();
            args.push("-np".into());
            args.push(self.procs.to_string());

            if p.openmp {
                envs.push(("OMP_NUM_THREADS".into(), self.threads.to_string()));
                if self.bind {
                    // 各プロセスへ threads 個のコアを割り当て、その中でスレッドを固定。
                    args.push("--map-by".into());
                    args.push(format!("slot:PE={}", self.threads));
                    args.push("--bind-to".into());
                    args.push("core".into());
                    envs.push(("OMP_PROC_BIND".into(), "close".into()));
                    envs.push(("OMP_PLACES".into(), "cores".into()));
                }
            } else if self.bind {
                args.push("--bind-to".into());
                args.push("core".into());
            }
            args.push(bin_path.clone());
        } else if p.openmp {
            // 純 OpenMP。
            envs.push(("OMP_NUM_THREADS".into(), self.threads.to_string()));
            if self.bind {
                envs.push(("OMP_PROC_BIND".into(), "close".into()));
                envs.push(("OMP_PLACES".into(), "cores".into()));
            }
        }
        // Pthread / 逐次は素のまま（program=bin_path、引数・環境変数なし）。

        // <実行ファイル名>.in があれば標準入力に接続する。
        let stdin_name = format!("{}.in", e.binary);
        let stdin_full = self.dir.join(&stdin_name);
        let stdin = if stdin_full.is_file() {
            Some(stdin_full)
        } else {
            None
        };

        // 表示用コマンド行（env... program args [< name.in] の順、シェル風）。
        let env_disp: String = envs.iter().map(|(k, v)| format!("{k}={v} ")).collect();
        let arg_disp = if args.is_empty() {
            String::new()
        } else {
            format!(" {}", args.join(" "))
        };
        let in_disp = if stdin.is_some() {
            format!(" < {stdin_name}")
        } else {
            String::new()
        };
        let display = format!("$ {env_disp}{program}{arg_disp}{in_disp}");

        RunSpec {
            program,
            args,
            envs,
            stdin,
            display,
        }
    }

    /// 選択中エントリを並列方式にふさわしい方法で実行する。
    pub fn run_selected(&mut self) {
        // エントリから spec を作りつつ借用を切ってから、実行・状態更新する。
        let idx = self.list_state.selected().unwrap_or(0);
        let (spec, binary, state, timing, source) = match self.entries.get(idx) {
            Some(e) => (
                self.build_spec(e),
                e.binary.clone(),
                e.state,
                e.timing,
                e.source
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_string(),
            ),
            None => return,
        };
        // 未ビルドは実行できない。古い（Stale）場合は実行を許しつつ警告する。
        if state == BuildState::Missing {
            self.status = format!("未ビルド: {binary}（autocc でコンパイルすること）");
            return;
        }
        let warn = if state == BuildState::Stale {
            " ⚠ ソースが新しい（autocc で再ビルド推奨）"
        } else {
            ""
        };

        let result = compile::run(
            &spec.program,
            &spec.args,
            &spec.envs,
            spec.stdin.as_deref(),
            &spec.display,
        );

        // 計測ライブラリを使うソースなら、実行結果を results.csv へ追記する。
        let mut csv_note = String::new();
        if timing && result.success {
            match self.append_csv(&source, &result) {
                Ok(()) => csv_note = " / results.csv に追記".into(),
                Err(e) => csv_note = format!(" / CSV 追記失敗: {e}"),
            }
        }

        self.status = if result.success {
            format!("実行終了: {binary} ({} ms){warn}{csv_note}", result.elapsed_ms)
        } else {
            format!("実行失敗: {binary}{warn}")
        };
        self.last_result = Some(result);
    }

    /// 実行結果を対象ディレクトリの results.csv へ1行追記する。
    ///
    /// ファイルが無ければヘッダ行を先に書く。時間は stdout/stderr 中の
    /// `TIME=<値>` を探して prog_time 列へ入れる（見つからなければ空）。
    fn append_csv(&self, source: &str, result: &RunResult) -> std::io::Result<()> {
        let path = self.dir.join("results.csv");
        let new_file = !path.exists();

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        if new_file {
            writeln!(
                file,
                "unix_time,source,parallelism,procs,threads,bind,wall_ms,prog_time"
            )?;
        }

        let unix_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // 計測値は stdout を優先し、無ければ stderr から探す。
        let prog_time = extract_time(&result.stdout)
            .or_else(|| extract_time(&result.stderr))
            .unwrap_or_default();
        // 並列方式ラベルにカンマは含まれない（CSV エスケープ不要）。
        let par = self
            .selected_entry()
            .map(|e| e.par.label())
            .unwrap_or("?");

        writeln!(
            file,
            "{unix_time},{source},{par},{},{},{},{},{prog_time}",
            self.procs,
            self.threads,
            if self.bind { 1 } else { 0 },
            result.elapsed_ms,
        )?;
        Ok(())
    }

    /// ディレクトリを再走査して一覧を更新する。
    pub fn refresh(&mut self) {
        match scan(&self.dir) {
            Ok(entries) => {
                self.entries = entries;
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
}

/// ディレクトリ直下のソースを集め、並列方式と実行ファイルの有無を判定して返す。
fn scan(dir: &Path) -> std::io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for ent in std::fs::read_dir(dir)? {
        let path = ent?.path();
        if !path.is_file() {
            continue;
        }
        if Lang::from_path(&path).is_none() {
            continue;
        }
        let binary = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("a.out")
            .to_string();
        // 実行ファイルはソースと同じディレクトリにある前提（autocc の出力）。
        // ソースと実行ファイルの更新時刻を比べてビルド状態を判定する。
        let state = build_state(&path, &dir.join(&binary));
        let (par, timing) = analyze_runtime(&path).unwrap_or_default();
        entries.push(Entry {
            source: path,
            binary,
            state,
            par,
            timing,
        });
    }
    entries.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(entries)
}

/// 出力テキストから `TIME=<値>` を探し、値の文字列を返す。
///
/// プログラム側は `printf("TIME=%f\n", t)` のように出力する取り決め。
fn extract_time(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            line.find("TIME=").map(|pos| {
                line[pos + 5..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(',')
                    .to_string()
            })
        })
        .filter(|s| !s.is_empty())
}

/// ソースと実行ファイルの更新時刻を比べてビルド状態を判定する。
fn build_state(source: &Path, binary: &Path) -> BuildState {
    let bin_mtime = match std::fs::metadata(binary).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return BuildState::Missing,
    };
    let src_mtime = std::fs::metadata(source).and_then(|m| m.modified()).ok();
    match src_mtime {
        // ソースの方が新しければ「要再ビルド」。
        Some(s) if s > bin_mtime => BuildState::Stale,
        _ => BuildState::Fresh,
    }
}
