# rust-photo-sorter

## 🍳 Display

![project-info](doc/project-info.png)

## 🔗 Description

🎀 Rust + egui 实现的照片分类程序

## 🚀 用法

```powershell
# 先看它打算怎么搬（不动任何文件）
cargo run --release -p photo-sorter-cli -- -i "D:\照片" -o "D:\已分类" --dry-run

# 确认无误后真移动
cargo run --release -p photo-sorter-cli -- -i "D:\照片" -o "D:\已分类"
```

| 参数 | 说明 |
| --- | --- |
| `-i, --input-dir` | 输入目录，必填 |
| `-o, --output-dir` | 输出目录，**不填则与输入目录相同**（原地整理） |
| `--extract` | 提取模式：把整棵目录树的照片平铺到输出目录根，并删除因此变空的目录 |
| `--sort-type` | 确实没有拍摄日期的照片怎么办：`by-modification-date` / `by-single-folder`（默认）/ `do-not-move` |
| `--no-filename-date` | 不从**文件名**里解读拍摄日期（默认会解读，见下） |
| `-t, --photo-count-threshold` | 照片数量阈值，默认 20 |
| `--dry-run` | 只列计划，不动文件（与 `--copy` 互斥）；明细打到 stdout，可 `> 计划.txt` 保存 |
| `--copy` | 复制而非移动，保留源文件 |

日志级别由 `RUST_LOG` 控制（如 `$env:RUST_LOG="debug"`）。
**release 构建默认静默**，只有显式设置 `RUST_LOG` 才输出；结论摘要与试运行明细走 stdout，不受影响。

## 🖥️ 图形界面

```powershell
cargo run -p photo-sorter-gui
```

在命令行那套配置之上，界面里可以直接**执行**：

| 执行方式 | 行为 |
| --- | --- |
| 试运行（只列计划） | 只统计「打算怎么搬」，不碰任何文件 |
| 移动 | 搬到目标位置（默认），源文件不再保留 |
| 复制 | 复制到目标位置，源文件保留 |

- 整理跑在后台线程上，界面不会卡住：执行中会显示进度条、已处理数量与最近处理的文件。
- 结果面板给出一行统计（扫描/跳过/成功/失败/清理空目录/耗时），
  失败的文件逐条列出「从哪来、本来要去哪、为什么没成功」。
- **计划明细**折叠区列出每一条「哪张 → 哪张（进哪类目录）」和「哪张没动、为什么」，
  试运行时这份清单就是主要产出；鼠标悬停某一行能看到两头的完整路径。
- 试运行的结果会明确标注「文件没有被改动」，避免把计划误当成已完成的搬运。
- 界面日志同样受 `RUST_LOG` 控制，默认静默（双击启动时日志无处可去，属预期行为）。

## 📁 分类规则

有拍摄日期的照片按 **ISO 周**归档：

```
输出目录/
├── 2026.07.27 至 08.02 第31周/   ← 该周内未超阈值的日子
│   └── IMG_0001.jpg
└── 2026.07.30/                    ← 这天照片数超过阈值，单独成目录（与周目录平级）
    ├── IMG_0001.jpg
    └── IMG_0001 (1).jpg           ← 不同子目录的同名文件，追加序号
```

- 拍摄日期依次取自 **EXIF**（`DateTimeOriginal` → `DateTimeDigitized` → `DateTime`）
  和**文件名**（可用 `--no-filename-date` 关掉）。
  文件名这一路默认开启，专治截图类工具：Steam 截图没有 EXIF，但文件名形如
  `1629520_20260911110905_1.png`。识别这些形态：
  `YYYYMMDDHHMMSS`、`YYYYMMDD_HHMMSS`、`YYYY-MM-DD_HHMMSS`、以及只有日期的 `YYYYMMDD`。
  EXIF 与文件名都没有时，才轮到 `--sort-type`。
- 确实没有拍摄日期时按 `--sort-type` 处理：
  - `by-single-folder`（默认）→ 放进 `无拍摄日期/`
  - `by-modification-date` → 用文件修改时间（拷来拷去的文件，修改时间往往已经不是拍摄时间了）
  - `do-not-move` → 原地不动
- 输出目录嵌在输入目录里时，扫描会跳过它，避免重复运行把已归档的照片再排一遍。

## 🧱 代码结构

```
crates/
├── core/   # 扫描 → 规划 → 落地，CLI 与 GUI 共用同一套规则
├── cli/    # 命令行前端
└── gui/    # egui 前端（配置 + 执行 + 进度）
```

Core 的 `worker::run` 接受一个可选的 `Observer`：CLI 在试运行时传一个把明细打到 stdout 的实现，
GUI 传一个把进度与明细转发给界面的实现（不传就是闷头干完再返回报告）。
「怎么搬」只有一处实现，两个前端不会走出两套行为。
