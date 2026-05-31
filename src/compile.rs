//! `CompilePlan` に基づくコンパイルの実行と、生成された実行ファイルの起動を担う。
//!
//! 副作用（外部プロセス起動）はこのモジュールに閉じ込める。

use crate::detect::CompilePlan;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

/// コンパイルまたは実行の結果。UI でそのまま表示できる形にまとめる。
pub struct RunResult {
    pub command: String, // 実際に走らせたコマンド行（表示用）
    pub success: bool,   // 終了コードが 0 か
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u128,
}

/// 計画に従ってコンパイラを起動する。
pub fn compile(plan: &CompilePlan) -> RunResult {
    let start = Instant::now();
    let output = Command::new(&plan.compiler).args(&plan.args).output();
    let command = format!("{} {}", plan.compiler, plan.args.join(" "));
    finish(command, output, start)
}

/// 任意のコマンドを環境変数付きで起動する汎用関数（実行ツール autorun 用）。
///
/// `stdin_path` が指定されていればそのファイルを標準入力に接続する
/// （無指定なら stdin は閉じる）。`display` は表示用のコマンド行。
pub fn run(
    program: &str,
    args: &[String],
    envs: &[(String, String)],
    stdin_path: Option<&Path>,
    display: &str,
) -> RunResult {
    let start = Instant::now();
    let mut cmd = Command::new(program);
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    // stdout / stderr は捕捉。stdin はファイル指定があれば接続、無ければ閉じる。
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    match stdin_path {
        Some(p) => match std::fs::File::open(p) {
            Ok(f) => {
                cmd.stdin(Stdio::from(f));
            }
            Err(e) => {
                return RunResult {
                    command: display.to_string(),
                    success: false,
                    stdout: String::new(),
                    stderr: format!("入力ファイルを開けない: {e}"),
                    elapsed_ms: start.elapsed().as_millis(),
                };
            }
        },
        None => {
            cmd.stdin(Stdio::null());
        }
    }
    let output = cmd.spawn().and_then(|child| child.wait_with_output());
    finish(display.to_string(), output, start)
}

/// `nvidia-smi` で実機 GPU の compute capability を取得し、`sm_XX` 形式で返す。
///
/// 例: compute_cap が "8.6" なら `Some("sm_86")`。GPU が無い・取得できない場合は `None`。
pub fn detect_cuda_arch() -> Option<String> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // 複数 GPU の場合は先頭行を使う。"8.6" → "sm_86"。
    let text = String::from_utf8_lossy(&out.stdout);
    let cap = text.lines().next()?.trim();
    let digits: String = cap.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        Some(format!("sm_{digits}"))
    }
}

/// プロセスの出力を `RunResult` に正規化する共通処理。
fn finish(
    command: String,
    output: std::io::Result<std::process::Output>,
    start: Instant,
) -> RunResult {
    let elapsed_ms = start.elapsed().as_millis();
    match output {
        Ok(o) => RunResult {
            command,
            success: o.status.success(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            elapsed_ms,
        },
        Err(e) => RunResult {
            command,
            success: false,
            stdout: String::new(),
            stderr: format!("起動失敗: {e}"),
            elapsed_ms,
        },
    }
}
