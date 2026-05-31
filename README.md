# autocc / autorun

C / C++ / CUDA 向けの TUI ツール群（Rust + ratatui）。責務を分離した2バイナリで構成する。

- **autocc** … コンパイル専用。ソースを `#include` 解析してコンパイラ／フラグを自動推定し、ワンキーでビルドする。
- **autorun** … 実行専用。ソースから並列方式を判定し、`mpirun` / `OMP_NUM_THREADS` / 直接実行を使い分けて起動する。

## インストール

```sh
cargo install --path .     # autocc と autorun の両方が入る
```

開発中は `cargo run --bin autocc -- examples` のように起動する。

---

## autocc（コンパイル）

```sh
autocc            # カレントディレクトリを対象
autocc examples   # 指定ディレクトリを対象
```

### キー操作

| キー | 動作 |
|------|------|
| `↑`/`↓`(`j`/`k`) | ソース選択 |
| `Enter` / `c` | 選択ソースをコンパイル |
| `o` | 最適化レベル切替（-O0→-O2→-O3→…） |
| `s` | C/C++ 規格切替（選択中ソースの言語に応じて循環） |
| `m` | `-march=native` の ON/OFF |
| `R` | ディレクトリ再スキャン |
| `q` / `Esc` | 終了 |

### 推定ルール（`#include` 解析）

| 検出ヘッダ | 反映内容 |
|------------|----------|
| 拡張子 `.c` | `gcc -O2 -Wall -std=c17` |
| 拡張子 `.cpp` 等 | `g++ -O2 -Wall -std=c++23` |
| 拡張子 `.cu` | `nvcc -O2`（+ 起動時検出した `-arch=sm_XX`） |
| `mpi.h` | コンパイラを `mpicc` / `mpic++` に切替 |
| `pthread.h` | `-pthread` |
| `omp.h` | `-fopenmp`（nvcc は `-Xcompiler -fopenmp`） |
| `math.h` | `-lm`（C のみ、リンク順序を考慮し末尾） |
| `cublas.h` / `cublas_v2.h` | `-lcublas` |
| `cusolverDn.h` | `-lcusolver`（GPU版LU分解 等） |
| `cusparse.h` | `-lcusparse` |
| `cblas.h` | `-lopenblas` |
| `lapacke.h` | `-llapacke` |

### ビルド設定（キーで切替・全ソース一律）

- 最適化レベル `-O0`/`-O2`/`-O3`（`o`）
- C 規格 `c11`/`c17`/`c23`、C++ 規格 `c++17`/`c++20`/`c++23`（`s`）
- `-march=native`（`m`、既定OFF・可搬性のため）
- CUDA `-arch=sm_XX`（起動時に `nvidia-smi` で実機 GPU の compute capability を自動検出）

---

## autorun（実行）

```sh
autorun            # カレントディレクトリを対象
autorun examples   # 指定ディレクトリを対象
```

ソースの `#include` から並列方式を判定し、実行方法を自動で選ぶ。

| 並列方式（検出） | 実行方法 |
|------------------|----------|
| `mpi.h` → MPI | `mpirun -np N [--bind-to core] ./bin` |
| `omp.h` → OpenMP | `OMP_NUM_THREADS=T [OMP_PROC_BIND=close OMP_PLACES=cores] ./bin` |
| `mpi.h`+`omp.h` → ハイブリッド | `OMP_NUM_THREADS=T mpirun -np N [--map-by slot:PE=T --bind-to core] ./bin` |
| `pthread.h` → Pthread | `./bin`（並列度はプログラム側） |
| なし → 逐次 | `./bin` |

`[ ]` 内のコア固定は `b` キーで ON/OFF できる（既定 ON。台数効果のベンチを安定させるため）。
ハイブリッドではプロセス数 N（`[`/`]`）とスレッド数 T（`+`/`-`）を独立に指定できる。

### キー操作

| キー | 動作 |
|------|------|
| `↑`/`↓`(`j`/`k`) | 実行候補を選択 |
| `Enter` / `r` | 実行 |
| `+` / `-` | OpenMP スレッド数（T）の増減 |
| `[` / `]` | MPI プロセス数（N）の増減 |
| `b` | コア固定（`--bind-to core` / `OMP_PROC_BIND`）の ON/OFF |
| `R` | ディレクトリ再スキャン |
| `q` / `Esc` | 終了 |

並列度の既定値は論理コア数。

ソースと実行ファイルの更新時刻を比べてビルド状態を表示する。

| 状態 | 表示 | 実行 |
|------|------|------|
| 最新（実行ファイルが新しい） | 緑 | 可 |
| 要再ビルド（ソースが新しい） | 黄 `(要再ビルド)` | 可（実行時に警告） |
| 未ビルド（実行ファイルなし） | 灰 `(未ビルド)` | 拒否 |

### 実行ログの CSV 記録

計測ライブラリ（`time.h` / `sys/time.h` / `<chrono>` / `omp.h` / `mpi.h`）を include するソースを実行すると、対象ディレクトリの `results.csv` に1行追記する（台数効果のデータ取り用）。

```
unix_time,source,parallelism,procs,threads,bind,wall_ms,prog_time
```

- `wall_ms` … autorun が測る壁時計時間（補助）
- `prog_time` … プログラム出力中の `TIME=<値>` を抽出した値（コード内計測）。

プログラム側は計測結果を次のように出力する取り決め:

```c
printf("TIME=%.6f\n", t1 - t0);
```


---

## 構成（責務分割）

```
src/
  lib.rs              # autocc / autorun が共有するコア
  detect.rs           # 言語判定・#include解析・コンパイル計画／並列種別の推定（純粋ロジック）
  compile.rs          # コンパイルと実行（プロセス起動）
  bin/
    autocc/           # コンパイル専用 TUI（main / app / ui）
    autorun/          # 実行専用 TUI（main / app / ui）
```
