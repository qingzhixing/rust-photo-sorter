//! 整理任务的编排与执行。
//!
//! [`run`] 是 Core 的入口：校验目录 → 扫描照片 → 算计划 → 落地搬运 → 汇报结果。
//!
//! 单个文件出错不会中断整次整理，而是记进 [`Report::problems`]，
//! 让剩下的照片先处理掉——半途而废对用户来说是最糟的结果。

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use log::{debug, info, warn};
use walkdir::WalkDir;

use crate::config::{SortConfig, WorkMode};
use crate::error::SortError;
use crate::photo;
use crate::planner::{self, Plan, PlanEntry, SkipReason, TargetFs};

/// 跨盘（跨卷）移动时 `fs::rename` 会失败，错误码各平台不同。
#[cfg(windows)]
const CROSS_DEVICE: i32 = 17; // ERROR_NOT_SAME_DEVICE
#[cfg(unix)]
const CROSS_DEVICE: i32 = 18; // EXDEV

/// 是不是「源和目标不在同一个卷」导致的失败。
///
/// 只有这一种失败才值得退化成「复制 + 删源」，
/// 其它错误（权限、目标已存在…）原样上报，免得把真正的问题掩盖成一次复制。
#[cfg(any(windows, unix))]
fn is_cross_device(err: &io::Error) -> bool {
    err.raw_os_error() == Some(CROSS_DEVICE)
}

#[cfg(not(any(windows, unix)))]
fn is_cross_device(_err: &io::Error) -> bool {
    false
}

/// 以什么方式落地这些计划。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ExecuteMode {
    /// 只把计划写进日志，不碰任何文件
    DryRun,
    /// 移动到目标位置（默认）
    #[default]
    Move,
    /// 复制到目标位置，源文件保留
    Copy,
}

impl ExecuteMode {
    /// 是不是「只看不动」。
    pub fn is_dry_run(self) -> bool {
        matches!(self, Self::DryRun)
    }
}

impl fmt::Display for ExecuteMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            ExecuteMode::DryRun => "试运行",
            ExecuteMode::Move => "移动",
            ExecuteMode::Copy => "复制",
        };
        f.write_str(text)
    }
}

/// 一次整理的统计结果。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// 本次用的执行方式，决定 [`Report::summary`] 怎么措辞
    pub mode: ExecuteMode,
    /// 扫描到的照片数
    pub scanned: usize,
    /// 计划里要搬运的数量
    pub planned: usize,
    /// 明确跳过的数量
    pub skipped: usize,
    /// 成功搬运的数量（试运行时是「打算搬运」的数量）
    pub succeeded: usize,
    /// 失败的数量
    pub failed: usize,
    /// 清理掉的空目录数
    pub removed_empty_dirs: usize,
    /// 单个文件或目录的问题，格式为「对象: 原因」
    pub problems: Vec<String>,
}

impl Report {
    /// 给人看的一行结论。
    ///
    /// 试运行必须显式标出来：一份写着「成功 200 张」却不区分真假的结果，
    /// 对会动用户照片的工具来说是危险的。
    pub fn summary(&self) -> String {
        let (prefix, done) = if self.mode.is_dry_run() {
            ("[试运行] ", "待处理")
        } else {
            ("", "成功")
        };
        format!(
            "{prefix}扫描 {} 张，跳过 {} 张，{done} {} 张，失败 {} 张，清理空目录 {} 个",
            self.scanned, self.skipped, self.succeeded, self.failed, self.removed_empty_dirs
        )
    }
}

/// 按配置整理照片。
///
/// `mode` 决定是「只看不动」「移动」还是「复制」。
pub fn run(config: &SortConfig, mode: ExecuteMode) -> Result<Report, SortError> {
    let input = Path::new(&config.input_dir);
    let output = Path::new(&config.output_dir);

    let meta = fs::metadata(input).map_err(|_| SortError::InputDirMissing(input.to_path_buf()))?;
    if !meta.is_dir() {
        return Err(SortError::InputDirNotADirectory(input.to_path_buf()));
    }
    if output.exists() && !output.is_dir() {
        return Err(SortError::OutputDirNotADirectory(output.to_path_buf()));
    }

    // 输出目录嵌在输入目录里时，扫描要跳过它，
    // 否则重复运行会把已经归档好的照片当成待整理的照片再排一遍
    let exclude = excluded_output(input, output);

    let scanned = photo::scan(input, exclude);
    let mut report = Report {
        mode,
        scanned: scanned.photos.len(),
        problems: scanned.problems,
        ..Report::default()
    };

    let listing = CachedListing::default();
    let plan = planner::plan(config, &scanned.photos, &listing);
    report.planned = plan.entries.len();
    report.skipped = plan.skipped.len();

    for item in &plan.skipped {
        debug!("跳过 {}：{}", item.source.display(), item.reason);
    }
    info!(
        "{}模式：待处理 {} 张，跳过 {} 张",
        mode, report.planned, report.skipped
    );
    // 「跳过了 74 张」不写清楚原因，用户根本没法判断这是不是自己想要的
    if report.skipped > 0 {
        info!("跳过明细：{}", skip_breakdown(&plan));
    }

    for entry in &plan.entries {
        match apply(entry, mode) {
            Ok(()) => report.succeeded += 1,
            Err(err) => {
                report.failed += 1;
                // 连目标一起报出来，否则用户只知道哪个文件失败、不知道它要去哪
                let problem = format!(
                    "{} -> {}: {}",
                    entry.source.display(),
                    entry.target.display(),
                    describe(&err)
                );
                warn!("处理失败 {problem}");
                report.problems.push(problem);
            }
        }
    }

    // 「提取」的语义本身就包含删除空目录；试运行不能动文件系统
    if config.work_mode == WorkMode::Extract && !mode.is_dry_run() {
        report.removed_empty_dirs = remove_empty_dirs(input, exclude);
    }

    // 结论由调用方决定怎么呈现（CLI 打到 stdout，GUI 画到界面上），
    // 这里只留一条 debug 级别的记录，避免和调用方的输出重复
    debug!("{}", report.summary());
    Ok(report)
}

/// 落地一条计划。
fn apply(entry: &PlanEntry, mode: ExecuteMode) -> io::Result<()> {
    match mode {
        ExecuteMode::DryRun => {
            info!(
                "[试运行] {} -> {}（{}）",
                entry.source.display(),
                entry.target.display(),
                entry.folder
            );
            Ok(())
        }
        ExecuteMode::Copy => {
            ensure_parent(&entry.target)?;
            info!(
                "复制 {} -> {}（{}）",
                entry.source.display(),
                entry.target.display(),
                entry.folder
            );
            fs::copy(&entry.source, &entry.target)
                .map(|_| ())
                .map_err(|err| context(err, "复制到目标失败"))
        }
        ExecuteMode::Move => {
            ensure_parent(&entry.target)?;
            match fs::rename(&entry.source, &entry.target) {
                Ok(()) => {
                    info!(
                        "移动 {} -> {}（{}）",
                        entry.source.display(),
                        entry.target.display(),
                        entry.folder
                    );
                    Ok(())
                }
                Err(err) if is_cross_device(&err) => {
                    // 跨盘时 rename 不可用，退化成复制 + 删源
                    fs::copy(&entry.source, &entry.target)
                        .map_err(|err| context(err, "跨盘移动时复制失败"))?;
                    fs::remove_file(&entry.source)
                        .map_err(|err| context(err, "跨盘移动时删除源文件失败"))?;
                    info!(
                        "移动（跨盘）{} -> {}（{}）",
                        entry.source.display(),
                        entry.target.display(),
                        entry.folder
                    );
                    Ok(())
                }
                Err(err) => Err(context(err, "重命名失败")),
            }
        }
    }
}

/// 给 I/O 错误补上「是哪一步失败的」。
///
/// 刻意保留原来的 `ErrorKind`——[`describe`] 靠它决定给什么排查提示；
/// 只把步骤名拼进消息里。没有这一步的话，一句「拒绝访问」根本分不清
/// 是建目录失败了、还是改名失败了。
fn context(err: io::Error, step: &str) -> io::Error {
    io::Error::new(err.kind(), format!("{step}: {err}"))
}

/// 把常见的 I/O 失败翻译成能指导下一步动作的话。
///
/// 「拒绝访问」最容易让人一头雾水：可能是文件被占用、只读，
/// 也可能是当前进程压根没拿到写目标目录的权限（受限的沙箱环境就是这样），
/// 光看 `os error 5` 是猜不出来的。
fn describe(err: &io::Error) -> String {
    match err.kind() {
        io::ErrorKind::PermissionDenied => format!(
            "{err}（写入被拒绝：文件可能被其它程序占用或处于只读状态，\
             也可能是当前进程无权写入目标目录）"
        ),
        io::ErrorKind::NotFound => format!("{err}（源文件或目标目录已不存在）"),
        io::ErrorKind::AlreadyExists => format!("{err}（目标位置已存在同名文件）"),
        _ => err.to_string(),
    }
}

/// 把跳过原因汇总成一行，例如「已在目标位置 74 张，配置为不移动 3 张」。
fn skip_breakdown(plan: &Plan) -> String {
    let mut counts: BTreeMap<SkipReason, usize> = BTreeMap::new();
    for item in &plan.skipped {
        *counts.entry(item.reason).or_default() += 1;
    }

    counts
        .into_iter()
        .map(|(reason, count)| format!("{reason} {count} 张"))
        .collect::<Vec<_>>()
        .join("，")
}

/// 目标目录嵌在输入目录里时返回它，用于在扫描时跳过。
fn excluded_output<'a>(input: &Path, output: &'a Path) -> Option<&'a Path> {
    if output != input && output.starts_with(input) {
        Some(output)
    } else {
        None
    }
}

/// 建好目标文件所在的目录。
fn ensure_parent(target: &Path) -> io::Result<()> {
    match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            fs::create_dir_all(parent).map_err(|err| context(err, "创建目标目录失败"))
        }
        _ => Ok(()),
    }
}

/// 自底向上删掉空目录，返回删除的个数。
///
/// 输入目录本身不会删；`exclude`（输出目录）整棵子树也跳过。
fn remove_empty_dirs(root: &Path, exclude: Option<&Path>) -> usize {
    let mut removed = 0;

    let walker = WalkDir::new(root)
        .contents_first(true)
        .into_iter()
        .filter_entry(|entry| Some(entry.path()) != exclude);

    for entry in walker {
        let Ok(entry) = entry else { continue };
        if entry.path() == root || !entry.file_type().is_dir() {
            continue;
        }
        if is_dir_empty(entry.path()) && fs::remove_dir(entry.path()).is_ok() {
            debug!("删除空目录 {}", entry.path().display());
            removed += 1;
        }
    }

    removed
}

fn is_dir_empty(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

/// 带缓存的目标目录清单：同一次规划里，一个目录只读一次磁盘。
#[derive(Default)]
struct CachedListing {
    cache: RefCell<HashMap<PathBuf, Vec<String>>>,
}

impl TargetFs for CachedListing {
    fn list_dir(&self, dir: &Path) -> Vec<String> {
        if let Some(cached) = self.cache.borrow().get(dir) {
            return cached.clone();
        }

        let names: Vec<String> = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();

        self.cache.borrow_mut().insert(dir.to_path_buf(), names.clone());
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NoDateSortType;
    use crate::test_support::{self, TempDir};

    fn config(input: &Path, output: &Path) -> SortConfig {
        SortConfig {
            work_mode: WorkMode::Sort,
            no_date_sort_type: NoDateSortType::BySingleFolder,
            input_dir: input.to_string_lossy().into_owned(),
            output_dir: output.to_string_lossy().into_owned(),
            photo_count_threshold: 20,
        }
    }

    #[test]
    fn missing_input_dir_is_a_clean_error() {
        let tmp = TempDir::new("missing-input");
        let err = run(&config(&tmp.path().join("nope"), tmp.path()), ExecuteMode::DryRun).unwrap_err();
        assert!(matches!(err, SortError::InputDirMissing(_)), "{err}");
    }

    #[test]
    fn dry_run_does_not_touch_the_filesystem() {
        let tmp = TempDir::new("dry-run");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("sub").join("a.jpg"), "not really a jpeg");

        let report = run(&config(&input, &output), ExecuteMode::DryRun).unwrap();

        assert_eq!(report.scanned, 1);
        assert_eq!(report.succeeded, 1);
        // 源文件原地不动，输出目录根本不该被创建
        assert!(input.join("sub").join("a.jpg").exists());
        assert!(!output.exists(), "试运行不该创建输出目录");
    }

    #[test]
    fn undated_photos_land_in_the_no_date_folder() {
        let tmp = TempDir::new("no-date");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("a.jpg"), "x");

        let report = run(&config(&input, &output), ExecuteMode::Move).unwrap();

        assert_eq!(report.succeeded, 1);
        assert_eq!(report.failed, 0);
        assert!(output.join(planner::NO_DATE_DIR).join("a.jpg").exists());
        assert!(!input.join("a.jpg").exists(), "移动模式应当删掉源文件");
    }

    #[test]
    fn copy_mode_keeps_the_source() {
        let tmp = TempDir::new("copy");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("a.jpg"), "x");

        let report = run(&config(&input, &output), ExecuteMode::Copy).unwrap();

        assert_eq!(report.succeeded, 1);
        assert!(output.join(planner::NO_DATE_DIR).join("a.jpg").exists());
        assert!(input.join("a.jpg").exists(), "复制模式必须保留源文件");
    }

    #[test]
    fn exif_datetime_decides_the_week_folder_end_to_end() {
        let tmp = TempDir::new("exif");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        // 造一张真的带 EXIF 的图片，走完整的「读 EXIF → 分周」链路
        test_support::write_exif_tiff(&input.join("shot.tif"), "2026:07:30 08:05:09");

        let report = run(&config(&input, &output), ExecuteMode::Move).unwrap();

        assert_eq!(report.succeeded, 1, "{:?}", report.problems);
        let target = output.join("2026.07.27 至 08.02 第31周").join("shot.tif");
        assert!(target.exists(), "没找到 {target:?}");
    }

    #[test]
    fn second_run_is_a_no_op_because_files_are_already_in_place() {
        let tmp = TempDir::new("idempotent");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("a.jpg"), "x");

        let cfg = config(&input, &output);
        let first = run(&cfg, ExecuteMode::Move).unwrap();
        assert_eq!(first.succeeded, 1);

        // 第二遍扫描输入目录已经空了，改成直接扫输出目录，验证「已就位」的判定
        let resort = SortConfig { input_dir: cfg.output_dir.clone(), ..cfg.clone() };
        let second = run(&resort, ExecuteMode::Move).unwrap();

        assert_eq!(second.scanned, 1);
        assert_eq!(second.succeeded, 0, "已经在对的位置就不该再动");
        assert_eq!(second.skipped, 1);
        assert!(output.join(planner::NO_DATE_DIR).join("a.jpg").exists());
    }

    #[test]
    fn output_dir_nested_in_input_is_excluded_from_scanning() {
        let tmp = TempDir::new("nested");
        let input = tmp.path().join("in");
        let output = input.join("sorted");
        test_support::write(&input.join("a.jpg"), "x");
        test_support::write(&output.join("已归档.jpg"), "y");

        let report = run(&config(&input, &output), ExecuteMode::Move).unwrap();

        // 只该看到 in/a.jpg，输出目录里的照片不参与扫描
        assert_eq!(report.scanned, 1);
        assert!(output.join("已归档.jpg").exists(), "已归档的照片不该被搬走");
    }

    #[test]
    fn extract_mode_flattens_and_removes_empty_dirs() {
        let tmp = TempDir::new("extract");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("2024").join("deep").join("a.jpg"), "x");
        test_support::write(&input.join("2024").join("b.jpg"), "y");

        let mut cfg = config(&input, &output);
        cfg.work_mode = WorkMode::Extract;

        let report = run(&cfg, ExecuteMode::Move).unwrap();

        assert_eq!(report.succeeded, 2);
        assert!(output.join("a.jpg").exists());
        assert!(output.join("b.jpg").exists());
        assert!(!input.join("2024").exists(), "空目录应当被清掉");
        assert!(input.exists(), "输入目录本身要保留");
        assert_eq!(report.removed_empty_dirs, 2);
    }

    #[test]
    fn sort_mode_leaves_source_directories_alone() {
        let tmp = TempDir::new("keep-dirs");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("sub").join("a.jpg"), "x");

        let report = run(&config(&input, &output), ExecuteMode::Move).unwrap();

        assert_eq!(report.succeeded, 1);
        // 分类模式不负责清理源目录结构，只有提取模式才删空目录
        assert!(input.join("sub").exists());
        assert_eq!(report.removed_empty_dirs, 0);
    }

    #[test]
    fn same_name_from_different_folders_both_survive() {
        let tmp = TempDir::new("collision");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("trip").join("IMG_1.jpg"), "one");
        test_support::write(&input.join("party").join("IMG_1.jpg"), "two");

        let report = run(&config(&input, &output), ExecuteMode::Move).unwrap();

        assert_eq!(report.succeeded, 2);
        assert_eq!(report.failed, 0);
        let dir = output.join(planner::NO_DATE_DIR);
        assert!(dir.join("IMG_1.jpg").exists());
        assert!(dir.join("IMG_1 (1).jpg").exists());
    }

    #[test]
    fn sorting_in_place_works_when_output_equals_input() {
        let tmp = TempDir::new("in-place");
        let input = tmp.path().join("in");
        test_support::write(&input.join("trip").join("a.jpg"), "x");

        let report = run(&config(&input, &input), ExecuteMode::Move).unwrap();

        assert_eq!(report.succeeded, 1);
        assert!(input.join(planner::NO_DATE_DIR).join("a.jpg").exists());
    }

    #[test]
    fn is_cross_device_only_matches_the_platform_error_code() {
        let other = io::Error::from_raw_os_error(2); // ENOENT / ERROR_FILE_NOT_FOUND
        assert!(!is_cross_device(&other));
    }

    #[test]
    fn permission_denied_errors_explain_themselves() {
        let err = io::Error::from(io::ErrorKind::PermissionDenied);
        let message = describe(&err);

        assert!(message.contains("写入被拒绝"), "{message}");
        // 提示里要提到「无权写入目标目录」这种可能，否则受限环境下的人排查不到
        assert!(message.contains("无权写入目标目录"), "{message}");
    }

    #[test]
    fn failures_report_both_ends_of_the_move() {
        let tmp = TempDir::new("report-both-ends");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("a.jpg"), "x");
        // 目标目录的位置被一个同名文件占住，创建目录必然失败
        test_support::write(&output.join(planner::NO_DATE_DIR), "占位");

        let report = run(&config(&input, &output), ExecuteMode::Move).unwrap();

        assert_eq!(report.failed, 1, "{:?}", report.problems);
        assert_eq!(report.succeeded, 0);

        let problem = &report.problems[0];
        // 源和目标都要出现，否则用户不知道这个文件本来要搬去哪
        assert!(problem.contains("a.jpg"), "{problem}");
        assert!(problem.contains(planner::NO_DATE_DIR), "{problem}");
        assert!(problem.contains(" -> "), "{problem}");
        // 失败原因要能看懂，不能只有一句 os error
        assert!(problem.contains("目标位置已存在同名文件"), "{problem}");
        // 还要说清是哪一步失败的：建目录 / 改名 / 复制
        assert!(problem.contains("创建目标目录失败"), "{problem}");
    }

    #[test]
    fn skip_breakdown_counts_each_reason() {
        use crate::planner::{Plan, Skipped};

        let plan = Plan {
            entries: Vec::new(),
            skipped: vec![
                Skipped {
                    source: PathBuf::from("a.jpg"),
                    reason: SkipReason::AlreadyInPlace,
                },
                Skipped {
                    source: PathBuf::from("b.jpg"),
                    reason: SkipReason::AlreadyInPlace,
                },
                Skipped { source: PathBuf::from("c.jpg"), reason: SkipReason::Configured },
            ],
        };

        let text = skip_breakdown(&plan);
        assert!(text.contains("已在目标位置 2 张"), "{text}");
        assert!(text.contains("配置为不移动 1 张"), "{text}");
    }
}
