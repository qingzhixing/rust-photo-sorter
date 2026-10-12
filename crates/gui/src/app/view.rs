//! 配置表单的**视图层**。
//!
//! 这里只负责把 [`ConfigForm`] 与执行状态渲染出来，并把用户操作翻译成
//! [`ViewAction`] 追加进 `actions`；视图本身不修改配置数据、不做校验、
//! 也不执行任何业务逻辑。
//!
//! 执行状态（进度、结果）来自控制层，视图只读不写：
//! 点「执行」按钮也仅仅是上报一个 [`ViewAction::Execute`]。

use std::time::Instant;

use eframe::egui;

use photo_sorter_core::config::{NoDateSortType, WorkMode};
use photo_sorter_core::worker::{ExecuteMode, Report};

use super::config_form::ConfigForm;
use super::job::{JobProgress, JobState, ResultMessage};

/// 「失败清单」最多直接铺开多少条。
///
/// 一次整理可能失败几百张，全铺出来会把结果区淹掉；剩下的折进折叠面板里。
const MAX_VISIBLE_PROBLEMS: usize = 8;

/// 输入/输出目录输入框的宽度。
///
/// 刻意收窄：窗口默认就这么高，「执行」区必须留在首屏里——
/// 主按钮要靠滚动才能找到，用户会以为这程序只会读配置。
const PATH_FIELD_WIDTH: f32 = 260.0;

/// 视图向控制层汇报的用户操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewAction {
    /// 「读取配置」按钮被点击
    ReadConfig,
    /// 「执行」按钮被点击
    Execute,
    /// 「输入目录」的「选择…」按钮被点击
    PickInputDir,
    /// 「输出目录」的「选择…」按钮被点击
    PickOutputDir,
}

/// 「读取配置」之后要在界面上展示的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadMessage {
    /// 读取成功，附带配置摘要
    Summary(String),
    /// 读取失败，附带错误信息
    Error(String),
}

/// 渲染整个配置窗口。
///
/// 用户操作会**追加**进 `actions`，由控制层决定怎么处理；
/// 本帧产生多个操作也不会丢。
pub fn show(
    ui: &mut egui::Ui,
    form: &mut ConfigForm,
    mode: &mut ExecuteMode,
    message: Option<&ReadMessage>,
    state: &JobState,
    actions: &mut Vec<ViewAction>,
) {
    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("照片分类配置");
            ui.add_space(8.0);

            egui::Grid::new("config_grid")
                .num_columns(2)
                .spacing([16.0, 10.0])
                .show(ui, |ui| {
                    config_fields(ui, form, actions);
                });

            ui.add_space(8.0);

            if let Some(message) = message {
                ui.add_space(6.0);
                match message {
                    ReadMessage::Summary(summary) => {
                        ui.label("读取到的配置：");
                        ui.label(summary);
                    }
                    ReadMessage::Error(err) => {
                        ui.colored_label(egui::Color32::RED, format!("错误：{err}"));
                    }
                }
            }

            ui.add_space(12.0);
            ui.separator();
            execute_section(ui, form, mode, state, actions);
        });
    });
}

/// 渲染表单中的各个字段，收集字段级别的操作。
fn config_fields(ui: &mut egui::Ui, form: &mut ConfigForm, actions: &mut Vec<ViewAction>) {
    // 工作模式
    ui.label("工作模式");
    ui.horizontal(|ui| {
        ui.radio_value(&mut form.work_mode, WorkMode::Sort, "分类");
        ui.radio_value(&mut form.work_mode, WorkMode::Extract, "提取");
    });
    ui.end_row();

    // 输入目录
    ui.label("输入目录");
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut form.input_dir)
                .desired_width(PATH_FIELD_WIDTH)
                .hint_text("需要处理的照片目录"),
        );
        if ui.button("选择…").clicked() {
            actions.push(ViewAction::PickInputDir);
        }
    });
    ui.end_row();

    // 输出目录
    ui.label("输出目录");
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut form.output_dir)
                .desired_width(PATH_FIELD_WIDTH)
                .hint_text("留空则与输入目录相同"),
        );
        if ui.button("选择…").clicked() {
            actions.push(ViewAction::PickOutputDir);
        }
    });
    ui.end_row();

    // 没有拍摄日期的照片分类标准
    ui.label("无拍摄日期照片");
    egui::ComboBox::from_id_salt("no_date_sort_type")
        .selected_text(form.no_date_sort_type.to_string())
        .show_ui(ui, |ui| {
            for sort_type in [
                NoDateSortType::ByModificationDate,
                NoDateSortType::BySingleFolder,
                NoDateSortType::FromFileName,
                NoDateSortType::DoNotMove,
            ] {
                ui.selectable_value(
                    &mut form.no_date_sort_type,
                    sort_type.clone(),
                    sort_type.to_string(),
                );
            }
        });
    ui.end_row();

    // 照片数量阈值
    ui.label("照片数量阈值")
        .on_hover_text("某一天的照片数量超过该阈值时，单独放入一个文件夹");
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut form.photo_count_threshold)
                .range(1..=u32::MAX)
                .speed(1),
        );
        if ui.button("读取配置").clicked() {
            actions.push(ViewAction::ReadConfig);
        }
    });
    ui.end_row();
}

/// 渲染执行区：执行方式、执行按钮、进度与结果。
fn execute_section(
    ui: &mut egui::Ui,
    form: &ConfigForm,
    mode: &mut ExecuteMode,
    state: &JobState,
    actions: &mut Vec<ViewAction>,
) {
    let running = state.is_running();

    ui.add_space(6.0);
    ui.heading("执行");

    ui.horizontal(|ui| {
        // 执行中不允许改：这一趟已经在按旧的方式搬文件了，
        // 让界面显示一个「已经不算数」的选择只会误导人
        ui.add_enabled_ui(!running, |ui| {
            // 「试运行」是「只列计划」，不是「只差最后一步不落地」：
            // 与 CLI 的 `--dry-run`（和 `--copy` 互斥）保持同一套语义
            ui.radio_value(mode, ExecuteMode::DryRun, "试运行")
                .on_hover_text("输出移动方式但是不实际执行");
            ui.radio_value(mode, ExecuteMode::Move, "移动")
                .on_hover_text("搬到目标位置，原文件不再保留");
            ui.radio_value(mode, ExecuteMode::Copy, "复制")
                .on_hover_text("复制到目标位置，原文件保留");
        });
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let label = format!("开始{}", form.work_mode);
        if ui.add_enabled(!running, egui::Button::new(label)).clicked() {
            actions.push(ViewAction::Execute);
        }

        ui.label(match mode {
            ExecuteMode::DryRun => "只列出打算怎么搬，不会改动任何文件",
            ExecuteMode::Move => "把照片搬到目标位置，源文件不再保留",
            ExecuteMode::Copy => "复制到目标位置，源文件保留",
        });
    });

    match state {
        JobState::Idle => {}
        JobState::Running {
            progress,
            started_at,
        } => {
            ui.add_space(10.0);
            progress_section(ui, progress, *started_at, *mode);
        }
        JobState::Finished(result) => {
            ui.add_space(10.0);
            ui.separator();
            result_section(ui, result);
        }
    }
}

/// 渲染执行中的进度。
fn progress_section(
    ui: &mut egui::Ui,
    progress: &JobProgress,
    started_at: Instant,
    mode: ExecuteMode,
) {
    // 试运行只是快速过一遍计划，说「处理中」会让人以为正在搬文件
    let activity = if mode.is_dry_run() {
        "正在列出计划"
    } else {
        "正在处理"
    };

    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(format!(
            "{activity} {}/{}（失败 {}）",
            progress.done(),
            progress.total(),
            progress.failed
        ));
        ui.label(format!("已用 {:.1} 秒", started_at.elapsed().as_secs_f32()));
    });

    ui.add(
        egui::ProgressBar::new(progress.ratio())
            .show_percentage()
            .desired_width(ui.available_width()),
    );

    if let Some(current) = &progress.current {
        // 只显示文件名：整条路径太长会把窗口顶宽，鼠标悬停能看全
        let name = current
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| current.display().to_string());
        ui.label(format!("最近处理：{name}"))
            .on_hover_text(current.display().to_string());
    }
}

/// 渲染执行结束后的统计与失败清单。
fn result_section(ui: &mut egui::Ui, result: &ResultMessage) {
    match result {
        ResultMessage::Error(err) => {
            ui.colored_label(egui::Color32::RED, format!("执行失败：{err}"));
        }
        ResultMessage::Report(report) => {
            statistics(ui, report);
            problems(ui, report);
        }
    }
}

/// 统计数字。
fn statistics(ui: &mut egui::Ui, report: &Report) {
    // 试运行必须显眼地标出来：一份写着「成功 200 张」却不区分真假的结果，
    // 对会动用户照片的工具来说是危险的
    if report.mode.is_dry_run() {
        ui.colored_label(
            egui::Color32::from_rgb(200, 140, 0),
            "[试运行] 以下都是打算执行的内容，文件没有被改动",
        );
    }

    ui.horizontal_wrapped(|ui| {
        ui.label(format!("执行方式: {}", report.mode));
        ui.separator();
        ui.label(format!("扫描: {} 张", report.scanned));
        ui.separator();
        ui.label(format!("跳过: {} 张", report.skipped));
        ui.separator();
        let done = if report.mode.is_dry_run() {
            format!("待处理: {} 张", report.succeeded)
        } else {
            format!("成功: {} 张", report.succeeded)
        };
        ui.label(done);
        ui.separator();
        let failed = format!("失败: {} 张", report.failed);
        if report.failed > 0 {
            ui.colored_label(egui::Color32::RED, failed);
        } else {
            ui.label(failed);
        }
        ui.separator();
        ui.label(format!("清理空目录: {} 个", report.removed_empty_dirs));
        ui.separator();
        ui.label(format!("耗时: {:.2} 秒", report.elapsed.as_secs_f32()));
    });
}

/// 失败清单。
///
/// 只报个「失败 3 张」等于什么也没说：用户既不知道是哪几张，
/// 也不知道该去修什么。所以这里把每条失败的完整说明都摆出来。
fn problems(ui: &mut egui::Ui, report: &Report) {
    if report.problems.is_empty() {
        return;
    }

    ui.add_space(6.0);
    ui.label(format!("失败明细（{} 条）：", report.problems.len()));

    for problem in report.problems.iter().take(MAX_VISIBLE_PROBLEMS) {
        ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {problem}"));
    }

    let rest = &report.problems[MAX_VISIBLE_PROBLEMS.min(report.problems.len())..];
    if !rest.is_empty() {
        egui::CollapsingHeader::new(format!("其余 {} 条", rest.len()))
            .id_salt("rest_problems")
            .show(ui, |ui| {
                for problem in rest {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠ {problem}"));
                }
            });
    }
}
