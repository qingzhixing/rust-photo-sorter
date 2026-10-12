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
| `--sort-type` | 无拍摄日期的照片怎么办：`by-modification-date` / `by-single-folder`（默认）/ `do-not-move` / `from-filename` |
| `-t, --photo-count-threshold` | 照片数量阈值，默认 20 |
| `--dry-run` | 只打印计划，不动文件 |
| `--copy` | 复制而非移动，保留源文件 |

日志级别由 `RUST_LOG` 控制（如 `$env:RUST_LOG="debug"`）。
**release 构建默认静默**，只有显式设置 `RUST_LOG` 才输出；结论摘要走 stdout，不受影响。

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

- 拍摄日期依次取 EXIF 的 `DateTimeOriginal` → `DateTimeDigitized` → `DateTime`。
- 没有拍摄日期时按 `--sort-type` 处理：
  - `by-single-folder`（默认）→ 放进 `无拍摄日期/`
  - `by-modification-date` → 用文件修改时间
  - `from-filename` → 从**文件名**里挖时间戳，适合截图目录（Steam 截图没有 EXIF，
    但文件名形如 `1629520_20260911110905_1.png`）。识别这些形态：
    `YYYYMMDDHHMMSS`、`YYYYMMDD_HHMMSS`、`YYYY-MM-DD_HHMMSS`、以及只有日期的 `YYYYMMDD`。
    都认不出来时仍然进 `无拍摄日期/`。
  - `do-not-move` → 原地不动
- 输出目录嵌在输入目录里时，扫描会跳过它，避免重复运行把已归档的照片再排一遍。
