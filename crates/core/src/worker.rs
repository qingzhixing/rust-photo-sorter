//! 整理任务的编排与执行。
//!
//! [`run`] 是 Core 的入口：校验目录 → 扫描照片 → 算计划 → 落地搬运 → 汇报结果。
//!
//! 单个文件出错不会中断整次整理，而是记进 [`Report::problems`]，
//! 让剩下的照片先处理掉——半途而废对用户来说是最糟的结果。
//!
//! 想在做的时候就被通知（GUI 要画进度条），传一个 [`Observer`] 给 [`run`] 即可；
//! 「怎么搬运」的规则始终只有 [`apply`] 一处实现。

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use walkdir::WalkDir;

use crate::config::{SortConfig, WorkMode};
use crate::error::SortError;
use crate::photo;
use crate::planner::{self, FolderKind, Plan, PlanEntry, SkipReason, TargetFs};

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

    /// 真正落地时用哪种方式。
    ///
    /// 试运行不落地文件，实际动作按「移动」演练——CLI 里 `--dry-run`
    /// 与 `--copy` 互斥，界面上也只有「试运行 / 移动 / 复制」三选一。
    /// 有了这个映射，调用方不必自己在各处补「试运行算哪一种」的判断。
    pub fn concrete(self) -> Self {
        match self {
            Self::DryRun => Self::Move,
            other => other,
        }
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

/// 一张照片的落地结果。
///
/// 失败时自带一句已经拼好的说明（源、目标、失败在哪一步），
/// 调用方直接展示即可，不必自己再拼一遍——CLI 的告警行和 GUI 的失败清单
/// 就是这样保持一致的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhotoOutcome {
    /// 成功搬运（试运行时表示「打算搬运」）
    Done {
        /// 源文件
        source: PathBuf,
        /// 目标路径（含最终文件名）
        target: PathBuf,
        /// 目标目录属于哪一类
        folder: FolderKind,
    },
    /// 失败
    Failed {
        /// 源文件
        source: PathBuf,
        /// 目标路径（含最终文件名）
        target: PathBuf,
        /// 目标目录属于哪一类
        folder: FolderKind,
        /// 供展示的失败说明，格式为「源 -> 目标: 原因」
        problem: String,
    },
}

impl PhotoOutcome {
    /// 这一张是不是成功落地了。
    pub fn is_done(&self) -> bool {
        matches!(self, PhotoOutcome::Done { .. })
    }

    /// 源文件路径。
    pub fn source(&self) -> &Path {
        match self {
            PhotoOutcome::Done { source, .. } | PhotoOutcome::Failed { source, .. } => source,
        }
    }

    /// 失败说明；成功时为 [`None`]。
    pub fn problem(&self) -> Option<&str> {
        match self {
            PhotoOutcome::Failed { problem, .. } => Some(problem),
            PhotoOutcome::Done { .. } => None,
        }
    }
}

/// 整理过程的旁听者。
///
/// Core 只管搬运，不假设调用方怎么展示进度：CLI 可以什么都不传，
/// GUI 则把这两次通知转成转发给界面的进度消息。
///
/// 回调在**执行整理的那个线程**上被调用；借用形式传入，且 [`run`] 返回前
/// 一定会把借用还回去，所以实现里用 `&self` + 内部可变性（如 `Cell`、通道）
/// 就够了，不需要一个可变的观察者。
pub trait Observer {
    /// 扫描与规划结束，开始落地前的通知；试运行不会触发。
    ///
    /// 参数依次为扫描到的照片数、计划搬运数、明确跳过的数量。
    fn on_started(&self, scanned: usize, planned: usize, skipped: usize) {
        let _ = (scanned, planned, skipped);
    }

    /// 一张照片处理完（成功或失败）之后的通知。
    fn on_photo(&self, outcome: &PhotoOutcome) {
        let _ = outcome;
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
    /// 本次整理实际花掉的时间
    pub elapsed: Duration,
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
///
/// `observer` 是可选的旁听者：传 [`None`] 就是「闷头干完再返回报告」，
/// 传 [`Some`] 则每张照片处理完都会收到通知（见 [`Observer`]）。
/// 试运行不落地任何文件，也就没有「进度」可言，因此**不会**发出开始通知，
/// 逐张的通知照发（试运行也要能逐张列出「打算怎么搬」）。
pub fn run(
    config: &SortConfig,
    mode: ExecuteMode,
    observer: Option<&dyn Observer>,
) -> Result<Report, SortError> {
    let started = Instant::now();
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

    // 试运行没有进度可言：这里一张文件都不落地
    if !mode.is_dry_run()
        && let Some(observer) = observer
    {
        observer.on_started(report.scanned, report.planned, report.skipped);
    }

    for entry in &plan.entries {
        let outcome = apply(entry, mode);
        match &outcome {
            PhotoOutcome::Done { .. } => report.succeeded += 1,
            PhotoOutcome::Failed { problem, .. } => {
                report.failed += 1;
                // 问题说明在 `apply` 里就拼好了（源、目标、失败在哪一步），
                // 这里只负责记进报告，免得两处各拼一次、结果还不一致
                report.problems.push(problem.clone());
            }
        }
        report_outcome(mode, &outcome);

        if let Some(observer) = observer {
            observer.on_photo(&outcome);
        }
    }

    // 「提取」的语义本身就包含删除空目录；试运行不能动文件系统
    if config.work_mode == WorkMode::Extract && !mode.is_dry_run() {
        report.removed_empty_dirs = remove_empty_dirs(input, exclude);
    }

    report.elapsed = started.elapsed();

    // 结论由调用方决定怎么呈现（CLI 打到 stdout，GUI 画到界面上），
    // 这里只留一条 debug 级别的记录，避免和调用方的输出重复
    debug!("{}", report.summary());
    Ok(report)
}

/// 把一张照片的落地结果写进日志。
///
/// 日志要说清「这一张是怎么处理的」，所以除结果本身外还要知道这次用的执行方式。
/// 试运行只是加前缀、不改动词：`--dry-run --copy` 说的是「打算复制」，
/// 一律写成「将移动」会把用户对这次执行的理解带偏。
fn report_outcome(mode: ExecuteMode, outcome: &PhotoOutcome) {
    match outcome {
        PhotoOutcome::Done {
            source,
            target,
            folder,
        } => {
            let prefix = outcome_verb(mode);
            info!(
                "{prefix} {} -> {}（{}）",
                source.display(),
                target.display(),
                folder
            );
        }
        PhotoOutcome::Failed { problem, .. } => warn!("处理失败 {problem}"),
    }
}

/// 日志里描述「这一张是怎么处理的」那一段。
///
/// 试运行只有一种：按「移动」演练（CLI 里 `--dry-run` 与 `--copy` 互斥，
/// GUI 的「试运行」选项也写明是「不动文件」）。所以这里不需要考虑
/// 「试运行 + 复制」这种组合。
fn outcome_verb(mode: ExecuteMode) -> &'static str {
    match mode.concrete() {
        ExecuteMode::Copy => "复制",
        // 试运行与移动共用一个说法：两种情况说的都是「搬到目标位置」
        _ if mode.is_dry_run() => "[试运行] 将移动",
        _ => "移动",
    }
}

/// 落地一条计划。
///
/// 「计划」到「文件真的动了」之间所有会失败的地方都在这里，
/// 且失败信息在这里就地拼好——调用方（报告、日志、GUI）直接拿去用。
fn apply(entry: &PlanEntry, mode: ExecuteMode) -> PhotoOutcome {
    let (source, target, folder) = (&entry.source, &entry.target, entry.folder);

    let failed = |problem: String| PhotoOutcome::Failed {
        source: source.clone(),
        target: target.clone(),
        folder,
        problem,
    };
    let done = || PhotoOutcome::Done {
        source: source.clone(),
        target: target.clone(),
        folder,
    };

    match mode {
        // 试运行不碰文件，日志统一由 `report_outcome` 打，这里只给出结果
        ExecuteMode::DryRun => done(),
        ExecuteMode::Copy => {
            if let Err(err) = ensure_parent(target) {
                return failed(problem(source, target, &err));
            }
            match fs::copy(source, target) {
                Ok(_) => done(),
                Err(err) => failed(problem(source, target, &context(err, "复制到目标失败"))),
            }
        }
        ExecuteMode::Move => {
            if let Err(err) = ensure_parent(target) {
                return failed(problem(source, target, &err));
            }
            match fs::rename(source, target) {
                Ok(()) => done(),
                Err(err) if is_cross_device(&err) => {
                    // 跨盘时 rename 不可用，退化成复制 + 删源
                    if let Err(err) = fs::copy(source, target) {
                        return failed(problem(
                            source,
                            target,
                            &context(err, "跨盘移动时复制失败"),
                        ));
                    }
                    match fs::remove_file(source) {
                        Ok(()) => done(),
                        Err(err) => failed(problem(
                            source,
                            target,
                            &context(err, "跨盘移动时删除源文件失败"),
                        )),
                    }
                }
                Err(err) => failed(problem(source, target, &context(err, "重命名失败"))),
            }
        }
    }
}

/// 把「哪张照片、本来要去哪、为什么没成功」拼成一句话。
fn problem(source: &Path, target: &Path, err: &io::Error) -> String {
    format!(
        "{} -> {}: {}",
        source.display(),
        target.display(),
        describe(err)
    )
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

        self.cache
            .borrow_mut()
            .insert(dir.to_path_buf(), names.clone());
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
        let err = run(
            &config(&tmp.path().join("nope"), tmp.path()),
            ExecuteMode::DryRun,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, SortError::InputDirMissing(_)), "{err}");
    }

    #[test]
    fn dry_run_does_not_touch_the_filesystem() {
        let tmp = TempDir::new("dry-run");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("sub").join("a.jpg"), "not really a jpeg");

        let report = run(&config(&input, &output), ExecuteMode::DryRun, None).unwrap();

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

        let report = run(&config(&input, &output), ExecuteMode::Move, None).unwrap();

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

        let report = run(&config(&input, &output), ExecuteMode::Copy, None).unwrap();

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

        let report = run(&config(&input, &output), ExecuteMode::Move, None).unwrap();

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
        let first = run(&cfg, ExecuteMode::Move, None).unwrap();
        assert_eq!(first.succeeded, 1);

        // 第二遍扫描输入目录已经空了，改成直接扫输出目录，验证「已就位」的判定
        let resort = SortConfig {
            input_dir: cfg.output_dir.clone(),
            ..cfg.clone()
        };
        let second = run(&resort, ExecuteMode::Move, None).unwrap();

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

        let report = run(&config(&input, &output), ExecuteMode::Move, None).unwrap();

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

        let report = run(&cfg, ExecuteMode::Move, None).unwrap();

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

        let report = run(&config(&input, &output), ExecuteMode::Move, None).unwrap();

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

        let report = run(&config(&input, &output), ExecuteMode::Move, None).unwrap();

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

        let report = run(&config(&input, &input), ExecuteMode::Move, None).unwrap();

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

        let report = run(&config(&input, &output), ExecuteMode::Move, None).unwrap();

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
                Skipped {
                    source: PathBuf::from("c.jpg"),
                    reason: SkipReason::Configured,
                },
            ],
        };

        let text = skip_breakdown(&plan);
        assert!(text.contains("已在目标位置 2 张"), "{text}");
        assert!(text.contains("配置为不移动 1 张"), "{text}");
    }

    /// 把观察者收到的通知原样记下来，供测试断言。
    #[derive(Default)]
    struct RecordingObserver {
        started: RefCell<Option<(usize, usize, usize)>>,
        outcomes: RefCell<Vec<PhotoOutcome>>,
    }

    impl Observer for RecordingObserver {
        fn on_started(&self, scanned: usize, planned: usize, skipped: usize) {
            *self.started.borrow_mut() = Some((scanned, planned, skipped));
        }

        fn on_photo(&self, outcome: &PhotoOutcome) {
            self.outcomes.borrow_mut().push(outcome.clone());
        }
    }

    #[test]
    fn observer_sees_the_start_and_every_photo() {
        let tmp = TempDir::new("observer");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("a.jpg"), "x");
        test_support::write(&input.join("b.jpg"), "y");

        let observer = RecordingObserver::default();
        let report = run(&config(&input, &output), ExecuteMode::Move, Some(&observer)).unwrap();

        assert_eq!(
            *observer.started.borrow(),
            Some((2, 2, 0)),
            "开始时应当报出扫描/计划/跳过的数量"
        );

        let outcomes = observer.outcomes.borrow();
        assert_eq!(outcomes.len(), report.planned, "每张照片都要通知一次");
        assert!(outcomes.iter().all(PhotoOutcome::is_done), "{outcomes:?}");
        // 通知里带上真实路径，界面才能显示「正在处理哪一个」
        assert!(outcomes.iter().any(|o| o.source().ends_with("a.jpg")));
        assert!(outcomes.iter().all(|o| o.problem().is_none()));
    }

    #[test]
    fn observer_hears_about_failures_too() {
        let tmp = TempDir::new("observer-failure");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("a.jpg"), "x");
        // 目标目录的位置被一个同名文件占住，建目录必然失败
        test_support::write(&output.join(planner::NO_DATE_DIR), "占位");

        let observer = RecordingObserver::default();
        let report = run(&config(&input, &output), ExecuteMode::Move, Some(&observer)).unwrap();

        assert_eq!(report.failed, 1);
        let outcomes = observer.outcomes.borrow();
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].is_done());
        // 观察者拿到的说明要和报告里的完全一致，界面才不会和 stdout 打架
        assert_eq!(outcomes[0].problem(), Some(report.problems[0].as_str()));
    }

    #[test]
    fn dry_run_announces_no_start_because_nothing_is_landed() {
        let tmp = TempDir::new("observer-dry-run");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("a.jpg"), "x");

        let observer = RecordingObserver::default();
        run(
            &config(&input, &output),
            ExecuteMode::DryRun,
            Some(&observer),
        )
        .unwrap();

        // 试运行不落地任何文件，没有「进度」可言，开始时的那次通知要省掉，
        // 否则界面会画出一个永远停在 0% 的进度条
        assert!(observer.started.borrow().is_none());
        // 逐张的通知仍然照发：界面上「试运行会怎么搬」的清单就是靠它拼的
        assert_eq!(observer.outcomes.borrow().len(), 1);
        assert!(observer.outcomes.borrow()[0].is_done());
    }

    #[test]
    fn report_carries_the_elapsed_time() {
        let tmp = TempDir::new("elapsed");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("a.jpg"), "x");

        let report = run(&config(&input, &output), ExecuteMode::Move, None).unwrap();

        // 只是「记了时间」，不假设具体多久——这种断言在快的机器上必然脆
        assert!(
            report.elapsed < Duration::from_secs(60),
            "{:?}",
            report.elapsed
        );
    }

    #[test]
    fn dry_run_rehearses_a_move_and_is_spelled_out() {
        // 试运行只有「按移动演练」一种，其它执行方式保持原样
        assert_eq!(ExecuteMode::DryRun.concrete(), ExecuteMode::Move);
        assert_eq!(ExecuteMode::Move.concrete(), ExecuteMode::Move);
        assert_eq!(ExecuteMode::Copy.concrete(), ExecuteMode::Copy);

        // 日志措辞：试运行必须一眼看得出来，复制要说「复制」而不是「移动」
        assert_eq!(outcome_verb(ExecuteMode::DryRun), "[试运行] 将移动");
        assert_eq!(outcome_verb(ExecuteMode::Move), "移动");
        assert_eq!(outcome_verb(ExecuteMode::Copy), "复制");
    }
}
