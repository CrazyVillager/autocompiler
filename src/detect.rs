//! ソースファイルの種別判定と、`#include` 解析によるコンパイラ／フラグ推定を担うモジュール。
//!
//! ここには「どうコンパイルすべきか」を決める純粋なロジックのみを置く。
//! 実際のプロセス起動は `compile`、画面描画は `ui` へ委譲する（単一責務）。

use std::path::{Path, PathBuf};

/// 最適化レベル。`o` キーで循環的に切り替える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    O0,
    O2,
    O3,
}

impl OptLevel {
    /// コンパイラへ渡す最適化フラグ。
    pub fn flag(&self) -> &'static str {
        match self {
            OptLevel::O0 => "-O0",
            OptLevel::O2 => "-O2",
            OptLevel::O3 => "-O3",
        }
    }

    /// 次のレベル（-O0 → -O2 → -O3 → -O0 と循環）。
    pub fn next(self) -> OptLevel {
        match self {
            OptLevel::O0 => OptLevel::O2,
            OptLevel::O2 => OptLevel::O3,
            OptLevel::O3 => OptLevel::O0,
        }
    }
}

/// C 言語規格。`s` キーで循環切替する（選択中ソースが .c のとき）。
///
/// GNU 方言（gnuXX）を採用する。ISO 厳密モード（cXX）だと `__STRICT_ANSI__` が
/// 定義され、glibc が `clock_gettime` 等の POSIX 拡張宣言を隠してしまうため。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CStd {
    Gnu11,
    Gnu17,
    Gnu23,
}

impl CStd {
    pub fn flag(&self) -> &'static str {
        match self {
            CStd::Gnu11 => "-std=gnu11",
            CStd::Gnu17 => "-std=gnu17",
            CStd::Gnu23 => "-std=gnu23",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            CStd::Gnu11 => "gnu11",
            CStd::Gnu17 => "gnu17",
            CStd::Gnu23 => "gnu23",
        }
    }
    pub fn next(self) -> CStd {
        match self {
            CStd::Gnu11 => CStd::Gnu17,
            CStd::Gnu17 => CStd::Gnu23,
            CStd::Gnu23 => CStd::Gnu11,
        }
    }
}

/// C++ 規格。`s` キーで循環切替する（選択中ソースが C++ のとき）。
///
/// C と同様、POSIX 拡張を見えるよう GNU 方言（gnu++XX）を採用する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CppStd {
    Gnupp17,
    Gnupp20,
    Gnupp23,
}

impl CppStd {
    pub fn flag(&self) -> &'static str {
        match self {
            CppStd::Gnupp17 => "-std=gnu++17",
            CppStd::Gnupp20 => "-std=gnu++20",
            CppStd::Gnupp23 => "-std=gnu++23",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            CppStd::Gnupp17 => "gnu++17",
            CppStd::Gnupp20 => "gnu++20",
            CppStd::Gnupp23 => "gnu++23",
        }
    }
    pub fn next(self) -> CppStd {
        match self {
            CppStd::Gnupp17 => CppStd::Gnupp20,
            CppStd::Gnupp20 => CppStd::Gnupp23,
            CppStd::Gnupp23 => CppStd::Gnupp17,
        }
    }
}

/// ビルド時に一律で適用する設定（最適化・規格・CPU/GPU 向けフラグ）。
#[derive(Debug, Clone)]
pub struct BuildConfig {
    pub opt: OptLevel,             // 最適化レベル
    pub march_native: bool,        // -march=native を付けるか
    pub cuda_arch: Option<String>, // 実機 GPU のアーキ（例 "sm_86"）。None なら指定しない
    pub c_std: CStd,               // C 規格
    pub cpp_std: CppStd,           // C++ 規格
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig {
            opt: OptLevel::O2,
            march_native: false, // 可搬性のため既定 OFF。ベンチ時に m で ON
            cuda_arch: None,
            c_std: CStd::Gnu17,
            cpp_std: CppStd::Gnupp23,
        }
    }
}

/// 対応するソース言語の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    C,
    Cpp,
    Cuda,
}

impl Lang {
    /// 拡張子から言語を判定する。未対応の拡張子なら `None`。
    pub fn from_path(path: &Path) -> Option<Lang> {
        let ext = path.extension()?.to_str()?;
        match ext {
            "c" => Some(Lang::C),
            // .C（大文字）は伝統的に C++ ソースを表す。
            "cpp" | "cc" | "cxx" | "c++" | "C" => Some(Lang::Cpp),
            "cu" => Some(Lang::Cuda),
            _ => None,
        }
    }

    /// 一覧表示などに使う短いラベル。
    pub fn label(&self) -> &'static str {
        match self {
            Lang::C => "C",
            Lang::Cpp => "C++",
            Lang::Cuda => "CUDA",
        }
    }
}

/// 1 つのソースをどうコンパイルするかを表す計画。
#[derive(Debug, Clone)]
pub struct CompilePlan {
    pub source: PathBuf,    // 入力ソース
    pub lang: Lang,         // 判定された言語
    pub compiler: String,   // 実行するコンパイラ（gcc/g++/nvcc/mpicc/mpic++）
    pub args: Vec<String>,  // コンパイラへ渡す引数（出力指定・リンクフラグを含む）
    pub output: String,     // 生成する実行ファイル名
    pub notes: Vec<String>, // 推定根拠（UI 表示用）
}

/// ソースを解析して `CompilePlan` を構築する。
///
/// `cfg` で最適化レベルや CPU/GPU 向けフラグを指定する。
/// 拡張子が未対応、あるいはファイルが読めない場合は `Err` を返す。
pub fn plan_for(path: &Path, cfg: &BuildConfig) -> Result<CompilePlan, String> {
    let lang =
        Lang::from_path(path).ok_or_else(|| format!("未対応の拡張子: {}", path.display()))?;
    let source_text =
        std::fs::read_to_string(path).map_err(|e| format!("読み込み失敗: {e}"))?;
    let includes = collect_includes(&source_text);

    // 出力ファイル名はソースのステム（拡張子を除いた部分）とする。
    let output = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a.out")
        .to_string();

    let mut notes = Vec::new();

    // --- ベースとなるコンパイラと最適化／警告フラグ ---
    let mut compiler = match lang {
        Lang::C => "gcc".to_string(),
        Lang::Cpp => "g++".to_string(),
        Lang::Cuda => "nvcc".to_string(),
    };
    // ソースより「前」に置くフラグ（最適化・規格・コンパイル時オプション）。
    let opt = cfg.opt.flag().to_string();
    let mut flags: Vec<String> = match lang {
        Lang::C => vec![opt, "-Wall".into(), cfg.c_std.flag().into()],
        Lang::Cpp => vec![opt, "-Wall".into(), cfg.cpp_std.flag().into()],
        // nvcc の -std は CUDA バージョン依存が強いため指定しない。
        Lang::Cuda => vec![opt],
    };
    // ソースより「後ろ」に置くリンクライブラリ（gcc のリンク順序対策）。
    let mut libs: Vec<String> = Vec::new();

    // --- CPU 向け命令セット最適化（-march=native） ---
    if cfg.march_native {
        match lang {
            // nvcc にはホストコンパイラ用フラグとして渡す。
            Lang::Cuda => {
                flags.push("-Xcompiler".into());
                flags.push("-march=native".into());
            }
            _ => flags.push("-march=native".into()),
        }
        notes.push("-march=native を付与（実機 CPU 向け）".into());
    }

    // --- CUDA アーキテクチャ指定（実機 GPU の compute capability） ---
    if lang == Lang::Cuda {
        if let Some(arch) = &cfg.cuda_arch {
            flags.push(format!("-arch={arch}"));
            notes.push(format!("実機 GPU 検出 → -arch={arch} を付与"));
        }
    }

    // 指定ヘッダが include されているか判定するクロージャ。
    let has = |h: &str| includes.iter().any(|i| i == h);

    // --- MPI: ラッパコンパイラへ切り替える ---
    if has("mpi.h") {
        match lang {
            Lang::C => {
                compiler = "mpicc".into();
                notes.push("mpi.h を検出 → mpicc を使用".into());
            }
            Lang::Cpp => {
                compiler = "mpic++".into();
                notes.push("mpi.h を検出 → mpic++ を使用".into());
            }
            // CUDA は nvcc のまま MPI ライブラリをリンクする。
            Lang::Cuda => {
                libs.push("-lmpi".into());
                notes.push("mpi.h を検出 → -lmpi を付与".into());
            }
        }
    }

    // --- Pthread ---
    if has("pthread.h") {
        match lang {
            // nvcc にはホストコンパイラ用フラグとして渡す。
            Lang::Cuda => {
                flags.push("-Xcompiler".into());
                flags.push("-pthread".into());
            }
            _ => flags.push("-pthread".into()),
        }
        notes.push("pthread.h を検出 → -pthread を付与".into());
    }

    // --- OpenMP ---
    if has("omp.h") {
        match lang {
            Lang::Cuda => {
                flags.push("-Xcompiler".into());
                flags.push("-fopenmp".into());
            }
            _ => flags.push("-fopenmp".into()),
        }
        notes.push("omp.h を検出 → -fopenmp を付与".into());
    }

    // --- 数学ライブラリ（C では明示リンクが必要なことが多い） ---
    if has("math.h") && lang == Lang::C {
        libs.push("-lm".into());
        notes.push("math.h を検出 → -lm を付与".into());
    }

    // --- 数値計算ライブラリ（include しても自動リンクされないため明示する） ---
    // (ヘッダ名, 付与する -l, 表示名) の対応表。
    let numeric_libs: &[(&str, &str, &str)] = &[
        ("cublas.h", "-lcublas", "cuBLAS"),
        ("cublas_v2.h", "-lcublas", "cuBLAS"),
        ("cusolverDn.h", "-lcusolver", "cuSOLVER"),
        ("cusparse.h", "-lcusparse", "cuSPARSE"),
        ("cblas.h", "-lopenblas", "OpenBLAS(CBLAS)"),
        ("lapacke.h", "-llapacke", "LAPACKE"),
    ];
    for (header, lib, label) in numeric_libs {
        // 同じ -l を二重に付けない（cublas.h と cublas_v2.h 等）。
        if has(header) && !libs.iter().any(|l| l == lib) {
            libs.push((*lib).into());
            notes.push(format!("{header} を検出 → {lib} を付与（{label}）"));
        }
    }

    // 最終的な引数列を組み立てる: フラグ → -o 出力 ソース → リンクライブラリ。
    let mut args = flags;
    args.push("-o".into());
    args.push(output.clone());
    args.push(path.to_string_lossy().into_owned());
    args.extend(libs);

    if notes.is_empty() {
        notes.push("追加ライブラリの検出なし（標準フラグのみ）".into());
    }

    Ok(CompilePlan {
        source: path.to_path_buf(),
        lang,
        compiler,
        args,
        output,
        notes,
    })
}

/// ソーステキストから `#include` されているヘッダ名（山括弧／引用符の中身）を収集する。
///
/// 正規表現には頼らず、行単位の単純な走査で済ませる（依存を増やさない方針）。
fn collect_includes(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        if !line.starts_with('#') {
            continue;
        }
        // '#' と "include" の間に空白が入る書き方にも対応する。
        let rest = line[1..].trim_start();
        let Some(rest) = rest.strip_prefix("include") else {
            continue;
        };
        let rest = rest.trim_start();
        // <...> または "..." の中身を取り出す。
        let header = match rest.chars().next() {
            Some('<') => rest[1..].split('>').next(),
            Some('"') => rest[1..].split('"').next(),
            _ => None,
        };
        if let Some(h) = header {
            out.push(h.trim().to_string());
        }
    }
    out
}

/// ソースが用いる並列方式。複数同時（例: MPI+OpenMP ハイブリッド）もありうるため、
/// 排他的な列挙ではなくフラグの集合として表す。実行ツール（autorun）が使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Parallelism {
    pub mpi: bool,
    pub openmp: bool,
    pub pthread: bool,
}

impl Parallelism {
    /// 表示用のラベル。
    pub fn label(&self) -> &'static str {
        match (self.mpi, self.openmp, self.pthread) {
            (true, true, _) => "MPI+OpenMP",
            (true, false, true) => "MPI+Pthread",
            (true, false, false) => "MPI",
            (false, true, _) => "OpenMP",
            (false, false, true) => "Pthread",
            _ => "逐次",
        }
    }
}

/// ソースの実行時情報をまとめて解析する。
///
/// 戻り値は (並列方式, 計測ライブラリを使っているか)。
/// 計測ライブラリは time.h / sys/time.h / `<chrono>` / omp.h(omp_get_wtime) /
/// mpi.h(MPI_Wtime) のいずれかの include で判定する。
pub fn analyze_runtime(path: &Path) -> Result<(Parallelism, bool), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("読み込み失敗: {e}"))?;
    let inc = collect_includes(&text);
    let has = |h: &str| inc.iter().any(|i| i == h);
    let par = Parallelism {
        mpi: has("mpi.h"),
        openmp: has("omp.h"),
        pthread: has("pthread.h"),
    };
    let timing = ["time.h", "sys/time.h", "chrono", "omp.h", "mpi.h"]
        .iter()
        .any(|h| has(h));
    Ok((par, timing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_includes_ignoring_spacing() {
        let src = "#  include <pthread.h>\n#include \"local.h\"\nint main(){}";
        let inc = collect_includes(src);
        assert!(inc.iter().any(|h| h == "pthread.h"));
        assert!(inc.iter().any(|h| h == "local.h"));
    }

    #[test]
    fn extension_maps_to_language() {
        assert_eq!(Lang::from_path(Path::new("a.c")), Some(Lang::C));
        assert_eq!(Lang::from_path(Path::new("a.cpp")), Some(Lang::Cpp));
        assert_eq!(Lang::from_path(Path::new("a.cu")), Some(Lang::Cuda));
        assert_eq!(Lang::from_path(Path::new("a.txt")), None);
    }

    /// pthread.h を含む C ソースには gcc + -pthread が選ばれ、リンク用 -lm はソースより後ろに来る。
    #[test]
    fn pthread_and_math_plan() {
        let dir = std::env::temp_dir().join("autocc_test_pthread");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("worker.c");
        std::fs::write(&file, "#include <pthread.h>\n#include <math.h>\nint main(){return 0;}").unwrap();

        let plan = plan_for(&file, &BuildConfig::default()).unwrap();
        assert_eq!(plan.compiler, "gcc");
        assert_eq!(plan.output, "worker");
        assert!(plan.args.contains(&"-O2".to_string()));
        assert!(plan.args.contains(&"-pthread".to_string()));
        // -lm は -o worker <source> より後ろ（リンク順序対策）。
        let lm = plan.args.iter().position(|a| a == "-lm").unwrap();
        let src = plan.args.iter().position(|a| a.ends_with("worker.c")).unwrap();
        assert!(lm > src);
    }

    /// mpi.h を含む C++ ソースは mpic++ へ切り替わる。
    #[test]
    fn mpi_switches_compiler() {
        let dir = std::env::temp_dir().join("autocc_test_mpi");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("solver.cpp");
        std::fs::write(&file, "#include <mpi.h>\nint main(){return 0;}").unwrap();

        let plan = plan_for(&file, &BuildConfig::default()).unwrap();
        assert_eq!(plan.compiler, "mpic++");
    }

    /// 最適化レベルがフラグへ反映され、循環順序も正しい。
    #[test]
    fn opt_level_flag_and_cycle() {
        let dir = std::env::temp_dir().join("autocc_test_opt");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("plain.c");
        std::fs::write(&file, "int main(){return 0;}").unwrap();

        let cfg = BuildConfig {
            opt: OptLevel::O3,
            ..BuildConfig::default()
        };
        let plan = plan_for(&file, &cfg).unwrap();
        assert!(plan.args.contains(&"-O3".to_string()));

        assert_eq!(OptLevel::O0.next(), OptLevel::O2);
        assert_eq!(OptLevel::O2.next(), OptLevel::O3);
        assert_eq!(OptLevel::O3.next(), OptLevel::O0);
    }

    /// CUDA ソースに march/arch と数値ライブラリが正しく載る。
    #[test]
    fn cuda_arch_march_and_numeric_libs() {
        let dir = std::env::temp_dir().join("autocc_test_cuda");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("lu.cu");
        std::fs::write(&file, "#include <cusolverDn.h>\nint main(){return 0;}").unwrap();

        let cfg = BuildConfig {
            opt: OptLevel::O2,
            march_native: true,
            cuda_arch: Some("sm_86".into()),
            ..BuildConfig::default()
        };
        let plan = plan_for(&file, &cfg).unwrap();
        assert_eq!(plan.compiler, "nvcc");
        assert!(plan.args.contains(&"-arch=sm_86".to_string()));
        // nvcc にはホスト向けフラグとして -Xcompiler 経由で渡る。
        assert!(plan.args.contains(&"-Xcompiler".to_string()));
        assert!(plan.args.contains(&"-march=native".to_string()));
        // 数値ライブラリはソースより後ろ。
        let lib = plan.args.iter().position(|a| a == "-lcusolver").unwrap();
        let src = plan.args.iter().position(|a| a.ends_with("lu.cu")).unwrap();
        assert!(lib > src);
    }
}
