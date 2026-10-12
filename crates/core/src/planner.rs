//! 归档目录与文件名的计算。
//!
//! 这一层刻意不碰文件系统——除了通过 [`TargetFs`] 查目标目录里已有什么。
//! 把文件系统抽成 trait 的好处是：目录结构、阈值、重名这些真正容易出错的规则
//! 可以脱离真实磁盘完整单测。
//!
//! # 目录结构
//!
//! 有拍摄日期的照片默认按 **ISO 周**归档，周目录形如
//! `2026.07.27 至 08.02 第31周`；某一天的照片数**超过**阈值时，
//! 该天单独建一个 `2026.07.30` 这样的目录，且**与周目录平级**。
//!
//! 没有拍摄日期的照片按 [`crate::config::NoDateSortType`] 处理。

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Days, NaiveDate, Weekday};

use crate::config::{NoDateSortType, SortConfig, WorkMode};
use crate::photo::{self, Photo};

/// 没有拍摄日期、且配置为「放入单独文件夹」时用的目录名。
pub const NO_DATE_DIR: &str = "无拍摄日期";

/// 目标目录的来源，主要用于日志和断言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    /// 按 ISO 周归档，如 `2026.07.27 至 08.02 第31周`
    Week,
    /// 照片数超过阈值的一天，如 `2026.07.30`
    Day,
    /// 没有拍摄日期的照片，如 `无拍摄日期`
    NoDate,
    /// 提取模式：平铺到输出目录根
    ExtractRoot,
}

impl fmt::Display for FolderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            FolderKind::Week => "周目录",
            FolderKind::Day => "单日目录",
            FolderKind::NoDate => "无日期目录",
            FolderKind::ExtractRoot => "输出目录根",
        };
        f.write_str(text)
    }
}

/// 没有进计划的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkipReason {
    /// 配置成「不移动」
    Configured,
    /// 已经在目标位置了，再动就是白折腾
    AlreadyInPlace,
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            SkipReason::Configured => "配置为不移动",
            SkipReason::AlreadyInPlace => "已在目标位置",
        };
        f.write_str(text)
    }
}

/// 一条待执行的搬运计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    /// 源文件
    pub source: PathBuf,
    /// 目标路径（含最终文件名）
    pub target: PathBuf,
    /// 目标目录属于哪一类
    pub folder: FolderKind,
}

/// 被跳过的照片。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// 源文件
    pub source: PathBuf,
    /// 跳过原因
    pub reason: SkipReason,
}

/// 一次规划的结果。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    /// 要搬运的文件
    pub entries: Vec<PlanEntry>,
    /// 明确不动的文件
    pub skipped: Vec<Skipped>,
}

/// 规划时需要的文件系统视图。
///
/// 抽成 trait 是为了让规划逻辑能脱离真实文件系统单测；
/// 真实实现见 `crate::worker` 里的带缓存版本。
pub trait TargetFs {
    /// 列出目录下已有的文件名（含子目录名）；目录不存在时返回空。
    fn list_dir(&self, dir: &Path) -> Vec<String>;
}

/// 一周的目录名，形如 `2026.07.27 至 08.02 第31周`。
///
/// 用 ISO-8601 周（周一到周日，第 1 周是含首个周四的那周），
/// 跨年时以周一所在年份为准，例如 2025-12-29 属于 2026 年第 1 周。
pub fn week_folder_name(date: NaiveDate) -> String {
    let iso = date.iso_week();
    // 用 ISO 周反推周一，跨年时也能拿到正确的起止日期
    let monday = NaiveDate::from_isoywd_opt(iso.year(), iso.week(), Weekday::Mon).unwrap_or(date);
    let sunday = monday + Days::new(6);
    format!(
        "{} 至 {} 第{}周",
        monday.format("%Y.%m.%d"),
        sunday.format("%m.%d"),
        iso.week()
    )
}

/// 单独一天的目录名，形如 `2026.07.30`。
pub fn day_folder_name(date: NaiveDate) -> String {
    date.format("%Y.%m.%d").to_string()
}

/// 依据配置与扫描结果算出搬运计划。
pub fn plan(config: &SortConfig, photos: &[Photo], fs: &dyn TargetFs) -> Plan {
    let root = PathBuf::from(&config.output_dir);

    // 第一步：每张照片先定「进哪一类目录」
    let pending: Vec<Pending> = photos.iter().map(|photo| classify(config, photo)).collect();

    // 第二步：统计每个自然日的照片数，决定哪些天要单独成目录
    let mut per_day: HashMap<NaiveDate, u32> = HashMap::new();
    for item in &pending {
        if let Pending::Dated(day) = item {
            *per_day.entry(*day).or_default() += 1;
        }
    }

    // 第三步：分配最终文件名，顺便解决重名
    let mut plan = Plan::default();
    let mut taken: HashMap<PathBuf, TakenNames> = HashMap::new();

    for (photo, item) in photos.iter().zip(&pending) {
        let Some((dir, folder)) = target_dir(config, item, &per_day, &root) else {
            let Pending::Skip(reason) = item else {
                unreachable!("只有 Skip 才会没有目标目录")
            };
            plan.skipped.push(Skipped { source: photo.path.clone(), reason: *reason });
            continue;
        };

        let names = taken
            .entry(dir.clone())
            .or_insert_with(|| TakenNames::from_listing(fs.list_dir(&dir)));

        let Some(file_name) = photo.path.file_name().and_then(|n| n.to_str()) else {
            // 文件名不是合法 UTF-8，没法参与重名计算，原样搬过去
            plan.entries.push(PlanEntry {
                source: photo.path.clone(),
                target: dir.join(photo.path.file_name().unwrap_or_default()),
                folder,
            });
            continue;
        };

        // 已经在目标目录里 → 不用动。这一步必须在重名检查之前，
        // 否则文件会把自己当成冲突，每跑一次就改一次名。
        if photo.path.parent() == Some(dir.as_path()) {
            names.insert(file_name);
            plan.skipped.push(Skipped {
                source: photo.path.clone(),
                reason: SkipReason::AlreadyInPlace,
            });
            continue;
        }

        let (stem, ext) = split_name(file_name);
        let unique = names.claim(stem, ext);
        plan.entries.push(PlanEntry {
            source: photo.path.clone(),
            target: dir.join(unique),
            folder,
        });
    }

    plan
}

/// 照片在归档前的中间状态。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    /// 有日期，但还没决定进周目录还是单日目录
    Dated(NaiveDate),
    /// 目录已确定
    Fixed { dir: PathBuf, folder: FolderKind },
    /// 不处理
    Skip(SkipReason),
}

/// 决定一张照片的去向（还没定文件名）。
fn classify(config: &SortConfig, photo: &Photo) -> Pending {
    let root = PathBuf::from(&config.output_dir);

    if config.work_mode == WorkMode::Extract {
        // 提取模式不分类，全部平铺到输出目录根
        return Pending::Fixed { dir: root, folder: FolderKind::ExtractRoot };
    }

    if let Some(datetime) = photo.exif_datetime {
        return Pending::Dated(datetime.date());
    }

    match config.no_date_sort_type {
        // 退而求其次用文件修改时间
        NoDateSortType::ByModificationDate => match photo.modified {
            Some(datetime) => Pending::Dated(datetime.date()),
            // 连修改时间都读不到，只能塞进无日期目录，总不能把文件丢了
            None => Pending::Fixed { dir: root.join(NO_DATE_DIR), folder: FolderKind::NoDate },
        },
        NoDateSortType::BySingleFolder => {
            Pending::Fixed { dir: root.join(NO_DATE_DIR), folder: FolderKind::NoDate }
        }
        // 截图类工具不写 EXIF，但常把时间戳放进文件名
        NoDateSortType::FromFileName => match photo::datetime_from_file_name(&photo.path) {
            Some(datetime) => Pending::Dated(datetime.date()),
            None => Pending::Fixed { dir: root.join(NO_DATE_DIR), folder: FolderKind::NoDate },
        },
        NoDateSortType::DoNotMove => Pending::Skip(SkipReason::Configured),
    }
}

/// 定下最终目录。
fn target_dir(
    config: &SortConfig,
    pending: &Pending,
    per_day: &HashMap<NaiveDate, u32>,
    root: &Path,
) -> Option<(PathBuf, FolderKind)> {
    match pending {
        Pending::Fixed { dir, folder } => Some((dir.clone(), *folder)),
        Pending::Skip(_) => None,
        Pending::Dated(day) => {
            let count = per_day.get(day).copied().unwrap_or(0);
            if count > config.photo_count_threshold {
                Some((root.join(day_folder_name(*day)), FolderKind::Day))
            } else {
                Some((root.join(week_folder_name(*day)), FolderKind::Week))
            }
        }
    }
}

/// 拆出主名与扩展名，扩展名保留原来的大小写。
fn split_name(file_name: &str) -> (&str, Option<&str>) {
    match file_name.rsplit_once('.') {
        // 开头的点属于隐藏文件名，不算扩展名分隔符
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (file_name, None),
    }
}

/// 某个目标目录里已被占用的文件名。
///
/// 统一按小写比较：Windows 的文件系统不区分大小写，
/// 在区分大小写的平台上这样稍保守，但绝不会误覆盖已有文件。
#[derive(Debug, Default, Clone)]
struct TakenNames(HashSet<String>);

impl TakenNames {
    fn from_listing(names: Vec<String>) -> Self {
        Self(names.into_iter().map(|name| name.to_lowercase()).collect())
    }

    fn contains(&self, name: &str) -> bool {
        self.0.contains(&name.to_lowercase())
    }

    fn insert(&mut self, name: &str) {
        self.0.insert(name.to_lowercase());
    }

    /// 占用一个不冲突的名字：`IMG_1234.jpg` → `IMG_1234 (1).jpg` → `IMG_1234 (2).jpg`…
    fn claim(&mut self, stem: &str, ext: Option<&str>) -> String {
        let compose = |n: u32| -> String {
            let base = if n == 0 { stem.to_owned() } else { format!("{stem} ({n})") };
            match ext {
                Some(ext) => format!("{base}.{ext}"),
                None => base,
            }
        };

        let mut n = 0;
        loop {
            let candidate = compose(n);
            if !self.contains(&candidate) {
                self.insert(&candidate);
                return candidate;
            }
            n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorkMode;
    use chrono::NaiveDateTime;
    use std::collections::HashMap as Map;
    use std::path::PathBuf;

    /// 用内存里的目录清单当文件系统，测试不碰真实磁盘。
    #[derive(Default)]
    struct FakeFs(Map<PathBuf, Vec<String>>);

    impl FakeFs {
        fn with(mut self, dir: &str, names: &[&str]) -> Self {
            self.0.insert(PathBuf::from(dir), names.iter().map(|n| (*n).to_owned()).collect());
            self
        }
    }

    impl TargetFs for FakeFs {
        fn list_dir(&self, dir: &Path) -> Vec<String> {
            self.0.get(dir).cloned().unwrap_or_default()
        }
    }

    fn config() -> SortConfig {
        SortConfig {
            work_mode: WorkMode::Sort,
            no_date_sort_type: NoDateSortType::BySingleFolder,
            input_dir: "C:\\in".to_owned(),
            output_dir: "C:\\out".to_owned(),
            photo_count_threshold: 20,
        }
    }

    fn photo(path: &str, taken: Option<&str>) -> Photo {
        Photo {
            path: PathBuf::from(path),
            exif_datetime: taken.map(|t| NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap()),
            modified: None,
        }
    }

    fn no_fs() -> FakeFs {
        FakeFs::default()
    }

    #[test]
    fn week_folder_matches_the_agreed_format() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 27).unwrap();
        assert_eq!(week_folder_name(date), "2026.07.27 至 08.02 第31周");
    }

    #[test]
    fn week_folder_handles_a_week_spanning_new_year() {
        // 2025-12-29 是 2026 年第 1 周的周一
        let date = NaiveDate::from_ymd_opt(2025, 12, 29).unwrap();
        assert_eq!(week_folder_name(date), "2025.12.29 至 01.04 第1周");
    }

    #[test]
    fn week_folder_is_stable_for_any_day_in_the_week() {
        let expected = "2026.07.27 至 08.02 第31周";
        for day in 27..=31 {
            let date = NaiveDate::from_ymd_opt(2026, 7, day).unwrap();
            assert_eq!(week_folder_name(date), expected, "7 月 {day} 日");
        }
        for day in 1..=2 {
            let date = NaiveDate::from_ymd_opt(2026, 8, day).unwrap();
            assert_eq!(week_folder_name(date), expected, "8 月 {day} 日");
        }
    }

    #[test]
    fn day_folder_uses_dotted_date() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 29).unwrap();
        assert_eq!(day_folder_name(date), "2026.08.29");
    }

    #[test]
    fn quiet_day_goes_into_the_week_folder() {
        let photos = vec![photo("C:\\in\\a.jpg", Some("2026-07-30 10:00:00"))];
        let plan = plan(&config(), &photos, &no_fs());

        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].folder, FolderKind::Week);
        assert_eq!(
            plan.entries[0].target,
            PathBuf::from("C:\\out\\2026.07.27 至 08.02 第31周\\a.jpg")
        );
    }

    #[test]
    fn busy_day_gets_its_own_folder_alongside_the_week_folder() {
        // 阈值 2，这天有 3 张 → 单独成目录
        let photos: Vec<Photo> = (0..3)
            .map(|i| photo(&format!("C:\\in\\{i}.jpg"), Some("2026-07-30 10:00:00")))
            .collect();
        let mut cfg = config();
        cfg.photo_count_threshold = 2;

        let plan = plan(&cfg, &photos, &no_fs());

        assert_eq!(plan.entries.len(), 3);
        for entry in &plan.entries {
            assert_eq!(entry.folder, FolderKind::Day);
            assert_eq!(entry.target.parent().unwrap(), Path::new("C:\\out\\2026.07.30"));
        }
    }

    #[test]
    fn threshold_is_strictly_greater_than() {
        // 阈值 2，正好 2 张 → 不超阈值，还留在周目录
        let photos: Vec<Photo> = (0..2)
            .map(|i| photo(&format!("C:\\in\\{i}.jpg"), Some("2026-07-30 10:00:00")))
            .collect();
        let mut cfg = config();
        cfg.photo_count_threshold = 2;

        let plan = plan(&cfg, &photos, &no_fs());
        assert!(plan.entries.iter().all(|e| e.folder == FolderKind::Week));
    }

    #[test]
    fn only_the_busy_day_leaves_the_week_folder() {
        // 同一个 ISO 周里的两天，只有 7-30 超阈值
        let mut photos = Vec::new();
        for i in 0..3 {
            photos.push(photo(&format!("C:\\in\\busy{i}.jpg"), Some("2026-07-30 10:00:00")));
        }
        photos.push(photo("C:\\in\\quiet.jpg", Some("2026-07-28 10:00:00")));

        let mut cfg = config();
        cfg.photo_count_threshold = 2;
        let plan = plan(&cfg, &photos, &no_fs());

        let quiet = plan.entries.iter().find(|e| e.source.ends_with("quiet.jpg")).unwrap();
        assert_eq!(quiet.folder, FolderKind::Week);

        let busy = plan.entries.iter().find(|e| e.source.ends_with("busy0.jpg")).unwrap();
        assert_eq!(busy.folder, FolderKind::Day);
    }

    #[test]
    fn photos_without_date_follow_the_configured_policy() {
        let photos = vec![photo("C:\\in\\a.jpg", None)];

        let mut cfg = config();
        cfg.no_date_sort_type = NoDateSortType::BySingleFolder;
        let placed = plan(&cfg, &photos, &no_fs());
        assert_eq!(placed.entries[0].folder, FolderKind::NoDate);
        assert_eq!(placed.entries[0].target, PathBuf::from("C:\\out\\无拍摄日期\\a.jpg"));

        cfg.no_date_sort_type = NoDateSortType::DoNotMove;
        let left_alone = plan(&cfg, &photos, &no_fs());
        assert!(left_alone.entries.is_empty());
        assert_eq!(left_alone.skipped[0].reason, SkipReason::Configured);
    }

    #[test]
    fn filename_timestamp_is_used_in_from_filename_mode() {
        // 真实的 Steam 截图命名：前面是应用 ID，后面才是时间戳
        let photos = vec![photo("C:\\in\\1629520_20260911110905_1.png", None)];
        let mut cfg = config();
        cfg.no_date_sort_type = NoDateSortType::FromFileName;

        let plan = plan(&cfg, &photos, &no_fs());

        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].folder, FolderKind::Week);
        // 2026-09-11 是第 37 周的周五，周一为 09-07
        assert_eq!(
            plan.entries[0].target,
            PathBuf::from("C:\\out\\2026.09.07 至 09.13 第37周\\1629520_20260911110905_1.png")
        );
    }

    #[test]
    fn from_filename_falls_back_to_the_no_date_folder() {
        let photos = vec![photo("C:\\in\\steam_screenshot.png", None)];
        let mut cfg = config();
        cfg.no_date_sort_type = NoDateSortType::FromFileName;

        let plan = plan(&cfg, &photos, &no_fs());

        // 文件名里没有可用日期 → 仍然进无日期目录，不能把文件弄丢
        assert_eq!(plan.entries[0].folder, FolderKind::NoDate);
        assert_eq!(
            plan.entries[0].target,
            PathBuf::from("C:\\out\\无拍摄日期\\steam_screenshot.png")
        );
    }

    #[test]
    fn modification_time_is_used_when_exif_is_missing() {
        let mut one = photo("C:\\in\\a.jpg", None);
        one.modified = Some(
            NaiveDateTime::parse_from_str("2026-07-30 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        );

        let mut cfg = config();
        cfg.no_date_sort_type = NoDateSortType::ByModificationDate;
        let plan = plan(&cfg, &[one], &no_fs());

        assert_eq!(plan.entries[0].folder, FolderKind::Week);
        assert_eq!(
            plan.entries[0].target,
            PathBuf::from("C:\\out\\2026.07.27 至 08.02 第31周\\a.jpg")
        );
    }

    #[test]
    fn photos_with_date_are_sorted_even_if_no_date_policy_is_do_not_move() {
        let photos = vec![
            photo("C:\\in\\dated.jpg", Some("2026-07-30 10:00:00")),
            photo("C:\\in\\undated.jpg", None),
        ];
        let mut cfg = config();
        cfg.no_date_sort_type = NoDateSortType::DoNotMove;

        let plan = plan(&cfg, &photos, &no_fs());

        assert_eq!(plan.entries.len(), 1);
        assert!(plan.entries[0].source.ends_with("dated.jpg"));
        assert_eq!(plan.skipped.len(), 1);
        assert!(plan.skipped[0].source.ends_with("undated.jpg"));
    }

    #[test]
    fn conflicting_names_get_a_numbered_suffix() {
        let photos = vec![
            photo("C:\\in\\trip\\IMG_1234.jpg", Some("2026-07-30 10:00:00")),
            photo("C:\\in\\other\\IMG_1234.jpg", Some("2026-07-30 11:00:00")),
        ];
        // 目标周目录里已经有一张同名照片
        let fs = FakeFs::default().with(
            "C:\\out\\2026.07.27 至 08.02 第31周",
            &["IMG_1234.jpg"],
        );

        let plan = plan(&config(), &photos, &fs);

        let names: Vec<String> = plan
            .entries
            .iter()
            .map(|e| e.target.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["IMG_1234 (1).jpg", "IMG_1234 (2).jpg"]);
    }

    #[test]
    fn conflict_detection_ignores_case() {
        let photos = vec![photo("C:\\in\\trip\\img_1234.jpg", Some("2026-07-30 10:00:00"))];
        let fs = FakeFs::default().with(
            "C:\\out\\2026.07.27 至 08.02 第31周",
            &["IMG_1234.JPG"],
        );

        let plan = plan(&config(), &photos, &fs);
        assert_eq!(
            plan.entries[0].target.file_name().unwrap().to_string_lossy(),
            "img_1234 (1).jpg"
        );
    }

    #[test]
    fn files_already_in_place_are_skipped_instead_of_renamed() {
        // 文件已经在正确的周目录里 → 再跑一次不该改名
        let photos = vec![photo(
            "C:\\out\\2026.07.27 至 08.02 第31周\\a.jpg",
            Some("2026-07-30 10:00:00"),
        )];
        let fs = FakeFs::default().with(
            "C:\\out\\2026.07.27 至 08.02 第31周",
            &["a.jpg"],
        );

        let plan = plan(&config(), &photos, &fs);

        assert!(plan.entries.is_empty());
        assert_eq!(plan.skipped[0].reason, SkipReason::AlreadyInPlace);
    }

    #[test]
    fn extract_mode_flattens_everything_into_the_output_root() {
        let photos = vec![
            photo("C:\\in\\2024\\deep\\a.jpg", Some("2026-07-30 10:00:00")),
            photo("C:\\in\\b.jpg", None),
        ];
        let mut cfg = config();
        cfg.work_mode = WorkMode::Extract;

        let plan = plan(&cfg, &photos, &no_fs());

        assert_eq!(plan.entries.len(), 2);
        for entry in &plan.entries {
            assert_eq!(entry.folder, FolderKind::ExtractRoot);
            assert_eq!(entry.target.parent().unwrap(), Path::new("C:\\out"));
        }
    }

    #[test]
    fn dotted_names_keep_their_extension_intact() {
        assert_eq!(split_name("a.tar.gz"), ("a.tar", Some("gz")));
        assert_eq!(split_name("noext"), ("noext", None));
        // 隐藏文件不把开头的点当扩展名分隔符
        assert_eq!(split_name(".gitignore"), (".gitignore", None));
    }

    #[test]
    fn claim_walks_forward_until_a_free_name_is_found() {
        let mut taken = TakenNames::from_listing(vec!["a.jpg".to_owned(), "a (1).jpg".to_owned()]);
        assert_eq!(taken.claim("a", Some("jpg")), "a (2).jpg");
    }
}
