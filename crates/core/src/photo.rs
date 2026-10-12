//! 照片的发现与拍摄时间提取。
//!
//! 扫描阶段只负责收集**事实**：文件在哪、EXIF 里写了什么拍摄时间、
//! 文件修改时间是多少。至于这些事实怎么归档，由 [`crate::planner`] 决定。

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};
use exif::{In, Tag};
use walkdir::WalkDir;

/// 认定为「照片」的扩展名，比较时不区分大小写。
///
/// 只按扩展名筛选：不读文件内容就没法更准确地判断，
/// 而误收进来的非图片文件会走「没有拍摄日期」的兜底分支，不会造成损坏。
pub const PHOTO_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "png", "gif", "bmp", "webp", "tif", "tiff", "heic", "heif", "avif",
    // 常见 RAW 格式
    "raw", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "dng", "orf", "rw2", "raf", "pef",
];

/// 一张待整理的照片，连同判定归档日期所需的事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Photo {
    /// 文件路径
    pub path: PathBuf,
    /// EXIF 里的拍摄时间
    pub exif_datetime: Option<NaiveDateTime>,
    /// 文件修改时间，EXIF 不可用时的兜底
    pub modified: Option<NaiveDateTime>,
}

/// 扫描结果。
///
/// 扫描过程中的问题（权限不足之类）不会中断流程，而是记在 `problems` 里，
/// 让能处理的照片先处理掉。
#[derive(Debug, Default, Clone)]
pub struct ScanResult {
    /// 找到的照片，按路径排序，保证多次运行的次序一致
    pub photos: Vec<Photo>,
    /// 扫描时遇到的问题，格式为「对象: 原因」
    pub problems: Vec<String>,
}

/// 扩展名是否是照片。
pub fn is_photo(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    PHOTO_EXTENSIONS
        .iter()
        .any(|known| ext.eq_ignore_ascii_case(known))
}

/// 路径的最后一段，供界面和时间轴之类的窄地方展示。
///
/// 找不到文件名（例如以 `..` 结尾）时退回整条路径：
/// 展示层宁可显示得啰嗦，也不该显示空白让人以为哪都去不了。
pub fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// 递归扫描 `input_dir` 下的所有照片。
///
/// `exclude` 用来跳过输出目录：当输出目录嵌在输入目录里时，
/// 不跳过的后果是重复运行会把已经归档好的照片再排一遍。
pub fn scan(input_dir: &Path, exclude: Option<&Path>) -> ScanResult {
    let mut photos = Vec::new();
    let mut problems = Vec::new();

    let walker = WalkDir::new(input_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| Some(entry.path()) != exclude);

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                problems.push(format!("遍历目录出错: {err}"));
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if !is_photo(path) {
            continue;
        }

        photos.push(Photo {
            path: path.to_path_buf(),
            exif_datetime: read_exif_datetime(path),
            modified: modified_time(path),
        });
    }

    photos.sort_by(|a, b| a.path.cmp(&b.path));

    ScanResult { photos, problems }
}

/// 从文件名里推断拍摄时间。
///
/// 截图类工具（Steam、部分手机）不写 EXIF，却会把时间戳塞进文件名，例如：
///
/// - `20260702224557_1.jpg` → 14 位连写
/// - `1629520_20260911110905_1.png` → 前面是应用 ID，后面 14 位才是时间
/// - `20260730_110905.jpg` → 8 位日期 + 分隔符 + 6 位时间
/// - `2026-07-30_11-09-05.jpg` → 用分隔符把年月日时分秒切开
/// - `20260911.png` → 只有日期，按当天 00:00:00 处理
///
/// 从左到右扫描数字段，取**第一个能构成合法日期**的组合；像 13 月这种非法值会被
/// `chrono` 的构造器挡掉，继续往后找。全都认不出来时返回 `None`。
pub fn datetime_from_file_name(path: &Path) -> Option<NaiveDateTime> {
    let name = path.file_name()?.to_str()?;
    let chars: Vec<char> = name.chars().collect();
    let runs = digit_runs(&chars);

    for (index, &(start, end)) in runs.iter().enumerate() {
        // ① 连写的 14 位：YYYYMMDDHHMMSS
        if end - start >= 14
            && let Some(parsed) = parse_compact(&chars[start..start + 14])
        {
            return Some(parsed);
        }

        // ② 被分隔符切开的 14 位，两种常见切法
        for lengths in [&[4usize, 2, 2, 2, 2, 2][..], &[8, 6][..]] {
            if let Some(joined) = match_digit_runs(&chars, &runs, index, lengths)
                && let Some(parsed) = parse_compact(&joined)
            {
                return Some(parsed);
            }
        }

        // ③ 只有日期：按当天零点算
        if end - start == 8
            && let Some(date) = parse_ymd(&chars[start..end])
        {
            return date.and_hms_opt(0, 0, 0);
        }
    }

    None
}

/// 收集所有「连续数字段」在字符序列里的区间 `[start, end)`。
fn digit_runs(chars: &[char]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        runs.push((start, i));
    }

    runs
}

/// 从第 `start` 个数字段起，按给定的各位长度依次匹配；段与段之间只允许短分隔符。
/// 成功时返回把所有段拼起来的数字序列。
fn match_digit_runs(
    chars: &[char],
    runs: &[(usize, usize)],
    start: usize,
    lengths: &[usize],
) -> Option<Vec<char>> {
    let mut joined = Vec::new();
    let mut previous_end: Option<usize> = None;

    for (offset, &expected) in lengths.iter().enumerate() {
        let &(from, to) = runs.get(start + offset)?;
        if to - from != expected {
            return None;
        }
        if let Some(previous) = previous_end
            && !is_time_separator(&chars[previous..from])
        {
            return None;
        }
        joined.extend_from_slice(&chars[from..to]);
        previous_end = Some(to);
    }

    Some(joined)
}

/// 解析连写的 `YYYYMMDDHHMMSS`。
fn parse_compact(chars: &[char]) -> Option<NaiveDateTime> {
    if chars.len() != 14 {
        return None;
    }
    parse_ymd(&chars[..8])?.and_hms_opt(
        number(&chars[8..10])?,
        number(&chars[10..12])?,
        number(&chars[12..14])?,
    )
}

/// 解析 `YYYYMMDD`；越界（比如 13 月、2 月 30 日）返回 `None`。
fn parse_ymd(chars: &[char]) -> Option<NaiveDate> {
    if chars.len() != 8 {
        return None;
    }
    NaiveDate::from_ymd_opt(
        number(&chars[..4])? as i32,
        number(&chars[4..6])?,
        number(&chars[6..8])?,
    )
}

fn number(chars: &[char]) -> Option<u32> {
    chars.iter().collect::<String>().parse().ok()
}

/// 两段数字之间是不是像日期时间的连接符（长度 1~3，且只由分隔符组成）。
fn is_time_separator(chars: &[char]) -> bool {
    (1..=3).contains(&chars.len())
        && chars
            .iter()
            .all(|c| matches!(c, '_' | '-' | '.' | 'T' | 't' | ' '))
}

/// 读取 EXIF 拍摄时间。
///
/// 依次尝试 `DateTimeOriginal`（拍摄时间）、`DateTimeDigitized`（数字化时间）、
/// `DateTime`（文件更改时间）——前两个才是真正的「拍摄时间」，
/// 但不少手机/软件只写第三个。
///
/// 读不到、格式不对、日期越界（比如 13 月）都返回 `None`，
/// 交给上层按「没有拍摄日期」处理。
pub fn read_exif_datetime(path: &Path) -> Option<NaiveDateTime> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;

    [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime]
        .into_iter()
        .find_map(|tag| exif.get_field(tag, In::PRIMARY).and_then(field_datetime))
}

/// 把 EXIF 的日期字段转成 `NaiveDateTime`。
fn field_datetime(field: &exif::Field) -> Option<NaiveDateTime> {
    let exif::Value::Ascii(values) = &field.value else {
        return None;
    };
    let raw = values.first()?;
    // 交给 exif 自己解析，它认得 `YYYY:MM:DD HH:MM:SS` 这个格式
    let parsed = exif::DateTime::from_ascii(raw).ok()?;
    to_naive(&parsed)
}

/// EXIF 的 `DateTime` 故意不校验取值范围（可能解析出 13 月），
/// 这里借 `chrono` 的构造器把非法日期挡掉。
fn to_naive(value: &exif::DateTime) -> Option<NaiveDateTime> {
    chrono::NaiveDate::from_ymd_opt(value.year.into(), value.month.into(), value.day.into())
        .and_then(|date| {
            date.and_hms_opt(value.hour.into(), value.minute.into(), value.second.into())
        })
}

/// 文件修改时间。
fn modified_time(path: &Path) -> Option<NaiveDateTime> {
    let system_time: SystemTime = fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::<Local>::from(system_time).naive_local())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_photo_extensions_case_insensitively() {
        for name in ["a.jpg", "a.JPG", "b.Jpeg", "c.HEIC", "d.cr2", "e.TIF"] {
            assert!(is_photo(Path::new(name)), "{name} 应当被认成照片");
        }
    }

    #[test]
    fn rejects_non_photo_files() {
        for name in ["a.txt", "b.mp4", "c", "d.", ".gitignore"] {
            assert!(!is_photo(Path::new(name)), "{name} 不该被认成照片");
        }
    }

    #[test]
    fn rejects_out_of_range_exif_datetime() {
        // exif 只做字符串解析，13 月这种越界值要靠 chrono 的构造器挡下来
        let parsed = exif::DateTime::from_ascii(b"2026:13:01 10:00:00").unwrap();
        assert_eq!(to_naive(&parsed), None);
    }

    #[test]
    fn accepts_valid_exif_datetime() {
        let parsed = exif::DateTime::from_ascii(b"2026:07:30 08:05:09").unwrap();
        assert_eq!(
            to_naive(&parsed).unwrap().to_string(),
            "2026-07-30 08:05:09"
        );
    }

    #[test]
    fn reads_datetime_from_common_screenshot_names() {
        let cases = [
            // 真实的 Steam 截图命名
            ("20260702224557_1.jpg", "2026-07-02 22:45:57"),
            ("1629520_20260911110905_1.png", "2026-09-11 11:09:05"),
            ("250900_20260813193543_1.png", "2026-08-13 19:35:43"),
            // 各种分隔写法
            ("IMG_20260730_110905.jpg", "2026-07-30 11:09:05"),
            ("2026-07-30_11-09-05.jpg", "2026-07-30 11:09:05"),
            ("2026.07.30 11.09.05.jpg", "2026-07-30 11:09:05"),
            ("20260730-110905.jpg", "2026-07-30 11:09:05"),
            // 只有日期
            ("20260911.png", "2026-09-11 00:00:00"),
        ];

        for (name, expected) in cases {
            let got = datetime_from_file_name(Path::new(name));
            assert_eq!(
                got.map(|d| d.to_string()).as_deref(),
                Some(expected),
                "文件名 {name}"
            );
        }
    }

    #[test]
    fn rejects_names_without_a_usable_date() {
        for name in [
            "IMG_1234.jpg",       // 数字段太短
            "99999999999999.jpg", // 14 位但不是合法日期
            "20261345.jpg",       // 13 月
            "20260230.jpg",       // 2 月 30 日
            "screenshot.png",     // 没有数字
            "1234_5678.png",      // 有 8 位数字但不是日期
            "a-1-2-3-4-5-6.jpg",  // 分段长度不对
        ] {
            assert_eq!(
                datetime_from_file_name(Path::new(name)),
                None,
                "文件名 {name} 不该被认出日期"
            );
        }
    }

    #[test]
    fn skips_a_leading_non_date_number_run() {
        // 前面的应用 ID 不是日期，应当继续往后找到真正的 14 位时间戳
        let got = datetime_from_file_name(Path::new("4122860_20260702224557_9.jpg"));
        assert_eq!(got.unwrap().to_string(), "2026-07-02 22:45:57");
    }
}
