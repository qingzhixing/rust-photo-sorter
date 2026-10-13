//! 窗口的**视图层**：配置页与版本页。
//!
//! 这里只负责把 [`ConfigForm`] 与执行状态渲染出来，并把用户操作翻译成
//! [`ViewAction`] 追加进 `actions`；视图本身不修改配置数据、不做校验、
//! 也不执行任何业务逻辑。
//!
//! 版本页的内容取自 [`info::INFO_ROWS`]——CLI 的 `-V` 念的是同一份清单，
//! 两个前端不会各说各话。
//!
//! 执行状态（进度、结果）来自控制层，视图只读不写：
//! 点「执行」按钮也仅仅是上报一个 [`ViewAction::Execute`]。

use std::time::Instant;

use eframe::egui;

use photo_sorter_core::config::{NoDateSortType, WorkMode};
use photo_sorter_core::info;
use photo_sorter_core::photo::file_label;
use photo_sorter_core::worker::{ExecuteMode, Report};

use super::config_form::ConfigForm;
use super::job::{JobProgress, JobState, PlannedMove, ResultMessage};

/// 「失败清单」最多直接铺开多少条。
///
/// 一次整理可能失败几百张，全铺出来会把结果区淹掉；剩下的折进折叠面板里。
const MAX_VISIBLE_PROBLEMS: usize = 8;

/// 计划明细展开后的高度上限。
///
/// 明细可能几百行，全铺开会把窗口顶成一长条；给个高度、里面自己滚动。
const DETAIL_AREA_HEIGHT: f32 = 200.0;

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
    /// 顶栏的「版本」按钮被点击
    OpenVersion,
    /// 版本页的「返回」按钮被点击
    CloseVersion,
}

/// 「读取配置」之后要在界面上展示的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadMessage {
    /// 读取成功，附带配置摘要
    Summary(String),
    /// 读取失败，附带错误信息
    Error(String),
}

/// 渲染整个窗口：顶栏 +（配置页 | 版本页）。
///
/// 用户操作会**追加**进 `actions`，由控制层决定怎么处理；
/// 本帧产生多个操作也不会丢。
pub fn show(
    ui: &mut egui::Ui,
    form: &mut ConfigForm,
    mode: &mut ExecuteMode,
    message: Option<&ReadMessage>,
    state: &JobState,
    show_version: bool,
    actions: &mut Vec<ViewAction>,
) {
    header(ui, show_version, actions);

    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            if show_version {
                version_page(ui);
            } else {
                config_page(ui, form, mode, message, state, actions);
            }
        });
    });
}

/// 顶栏：标题 + 「版本 / 返回」。
///
/// 单独做成面板而不是塞进正文开头：正文是可以滚动的，
/// 入口跟着滚没了的话，这个「偶尔看一眼」的页面就真找不到了。
fn header(ui: &mut egui::Ui, show_version: bool, actions: &mut Vec<ViewAction>) {
    egui::Panel::top("app_header").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading(if show_version {
                "版本信息"
            } else {
                "照片分类配置"
            });

            // 靠右放按钮：右到左布局，先加的在最右边
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if show_version {
                    if ui.button("← 返回").clicked() {
                        actions.push(ViewAction::CloseVersion);
                    }
                } else if ui
                    .button("版本")
                    .on_hover_text("与 `photo-sorter-cli -V` 输出的内容相同")
                    .clicked()
                {
                    actions.push(ViewAction::OpenVersion);
                }
            });
        });
        ui.add_space(4.0);
    });
}

/// 配置页正文：表单 + 执行区。
fn config_page(
    ui: &mut egui::Ui,
    form: &mut ConfigForm,
    mode: &mut ExecuteMode,
    message: Option<&ReadMessage>,
    state: &JobState,
    actions: &mut Vec<ViewAction>,
) {
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
}

/// 版本页正文。
///
/// 内容与 CLI 的 `-V` 同源（[`info::INFO_ROWS`]），顺序也一致：
/// 用户在两边看到的项目名、版本、作者、许可、主页必须是同一套。
fn version_page(ui: &mut egui::Ui) {
    egui::Grid::new("version_grid")
        .num_columns(2)
        .spacing([16.0, 10.0])
        .show(ui, |ui| {
            for row in info::INFO_ROWS {
                ui.label(format!("{} {}", row.icon, row.label));

                match row.url {
                    // 主页这类地址点一下交给系统浏览器
                    // （eframe 默认开着 links feature，见它的 default features）
                    Some(url) => {
                        ui.hyperlink_to(row.value, url)
                            .on_hover_text(format!("在浏览器中打开 {url}"));
                    }
                    // 其余的值（作者邮箱等）常要复制走，允许选中
                    None => {
                        ui.add(egui::Label::new(row.value).selectable(true))
                            .on_hover_text(row.value);
                    }
                }
                ui.end_row();
            }
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

    // 从文件名里解读日期：它是「拍摄日期的来源之一」，不是没有日期时的兜底，
    // 所以紧挨着「无拍摄日期怎么办」放，让人看得出先后顺序
    ui.label("从文件名读日期").on_hover_text(
        "截图类工具不写 EXIF，却把时间戳放进文件名（如 1629520_20260911110905_1.png）",
    );
    ui.horizontal(|ui| {
        ui.checkbox(&mut form.parse_date_from_filename, "开启");
        ui.label("没读到拍摄日期时，接着看文件名");
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
        JobState::Finished { progress, result } => {
            ui.add_space(10.0);
            ui.separator();
            result_section(ui, result, progress);
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
    // 试运行只是快速过一遍计划：说「处理中」会让人以为正在搬文件，
    // 画进度条也没意义——它压根不落地文件，也就没有「搬了多少」这回事
    let dry_run = mode.is_dry_run();

    ui.horizontal(|ui| {
        ui.spinner();
        if dry_run {
            ui.label(format!(
                "正在列出计划：已列出 {} 条（跳过 {} 张）",
                progress.moves.len(),
                progress.skipped
            ));
        } else {
            ui.label(format!(
                "正在处理 {}/{}（失败 {}）",
                progress.done(),
                progress.total(),
                progress.failed
            ));
        }
        ui.label(format!("已用 {:.1} 秒", started_at.elapsed().as_secs_f32()));
    });

    if !dry_run {
        ui.add(
            egui::ProgressBar::new(progress.ratio())
                .show_percentage()
                .desired_width(ui.available_width()),
        );
    }

    if let Some(current) = &progress.current {
        // 只显示文件名：整条路径太长会把窗口顶宽，鼠标悬停能看全
        ui.label(format!("最近处理：{}", file_label(current)))
            .on_hover_text(current.display().to_string());
    }
}

/// 渲染执行结束后的统计、明细与失败清单。
fn result_section(ui: &mut egui::Ui, result: &ResultMessage, progress: &JobProgress) {
    match result {
        ResultMessage::Error(err) => {
            ui.colored_label(egui::Color32::RED, format!("执行失败：{err}"));
            // 没能跑起来也可能已经排好了计划（例如半路改坏了目录），
            // 排出来的东西照样让人看
            plan_details(ui, progress);
        }
        ResultMessage::Report(report) => {
            statistics(ui, report);
            problems(ui, report);
            plan_details(ui, progress);
        }
    }
}

/// 计划明细：哪张搬到哪、哪张没动。
///
/// 默认折叠：几百行铺开会把结果区顶到屏幕外面，想看的人点一下就能展开。
/// 试运行时这份清单就是主要产出——「打算怎么搬」全在这里。
fn plan_details(ui: &mut egui::Ui, progress: &JobProgress) {
    if progress.moves.is_empty() && progress.skipped == 0 {
        return;
    }

    ui.add_space(6.0);
    let title = if progress.skipped > 0 {
        format!(
            "计划明细：{} 条搬运，{} 张不动",
            progress.moves.len(),
            progress.skipped
        )
    } else {
        format!("计划明细：{} 条搬运", progress.moves.len())
    };

    egui::CollapsingHeader::new(title)
        .id_salt("plan_details")
        .default_open(false)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(DETAIL_AREA_HEIGHT)
                .id_salt("plan_details_scroll")
                .show(ui, |ui| {
                    for item in &progress.moves {
                        move_row(ui, item);
                    }
                    if progress.omitted_moves > 0 {
                        ui.label(format!(
                            "… 其余 {} 条未列出（一次最多列 {} 条）",
                            progress.omitted_moves,
                            progress.moves.len() + progress.omitted_moves
                        ));
                    }

                    if progress.skipped > 0 {
                        ui.add_space(4.0);
                        ui.label(format!("不动（{} 张）：", progress.skipped));
                        if !progress.skipped_reasons.is_empty() {
                            ui.label(format!("　{}", progress.skipped_summary()));
                        }
                        for item in &progress.skipped_photos {
                            ui.label(format!("{}　←　{}", file_label(&item.source), item.reason))
                                .on_hover_text(item.source.display().to_string());
                        }
                    }
                });
        });
}

/// 一条「源 → 目标」。
///
/// 窄窗口里只放文件名和目录类别，整条路径挂在悬停提示上——
/// 两头的完整路径加起来往往比窗口还宽，硬铺出来会换行成一团。
fn move_row(ui: &mut egui::Ui, item: &PlannedMove) {
    let line = format!(
        "{}　→　{}　（{}）",
        file_label(&item.source),
        file_label(&item.target),
        item.folder
    );
    ui.label(egui::RichText::new(line).monospace())
        .on_hover_text(format!(
            "{}\n↓\n{}",
            item.source.display(),
            item.target.display()
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 一帧的输入：给足尺寸（不给的话面板会按 0 大小布局，什么都画不出来）。
    fn frame_input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 800.0),
            )),
            events,
            ..Default::default()
        }
    }

    /// 跑一帧，返回这一帧的输出。
    fn run_frame(
        ctx: &egui::Context,
        show_version: bool,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut form = ConfigForm::default();
        let mut mode = ExecuteMode::Move;
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(frame_input(events), |ui| {
            show(
                ui,
                &mut form,
                &mut mode,
                None,
                &JobState::default(),
                show_version,
                &mut actions,
            );
        });

        // 真后端会把字体图集这类纹理增量传给 GPU；测试里没人接，
        // 得自己清掉——否则 epaint 在 drop 时会认为漏了处理而 panic
        output.textures_delta.clear();
        output
    }

    /// 遍历一帧里画出的所有图元（`Shape::Vec` 要递归下去）。
    fn walk_shapes(output: &egui::FullOutput, visit: &mut impl FnMut(&egui::Shape)) {
        fn walk(shape: &egui::Shape, visit: &mut impl FnMut(&egui::Shape)) {
            visit(shape);
            if let egui::Shape::Vec(shapes) = shape {
                for shape in shapes {
                    walk(shape, visit);
                }
            }
        }
        for clipped in &output.shapes {
            walk(&clipped.shape, visit);
        }
    }

    /// 跑一帧，把画出来的文字全抠出来。
    ///
    /// 界面没法在测试里「看」，但 egui 每帧都会把文字布局交给后端，
    /// 于是可以据此确认真的是**画了**这些字，而不只是状态变了。
    fn rendered_texts(show_version: bool) -> Vec<String> {
        let ctx = egui::Context::default();
        let output = run_frame(&ctx, show_version, Vec::new());

        let mut texts = Vec::new();
        walk_shapes(&output, &mut |shape| {
            if let egui::Shape::Text(text) = shape {
                texts.push(text.galley.text().to_owned());
            }
        });
        texts
    }

    /// 某段文字画在了哪里。
    fn text_rect(output: &egui::FullOutput, needle: &str) -> Option<egui::Rect> {
        let mut found = None;
        walk_shapes(output, &mut |shape| {
            if found.is_none()
                && let egui::Shape::Text(text) = shape
                && text.galley.text() == needle
            {
                found = Some(egui::Rect::from_min_size(text.pos, text.galley.size()));
            }
        });
        found
    }

    /// 这一帧有没有请求后端打开某个地址。
    ///
    /// 链接点下去要经过后端才会真的拉起浏览器，测试里就看这条命令——
    /// 不用真弹一个浏览器出来，也能证明链接是活的。
    fn has_open_url(output: &egui::FullOutput, url: &str) -> bool {
        output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::OpenUrl(open) if open.url == url))
    }

    /// 把指针移到某段文字上并按一下、松手，返回松手那一帧的输出。
    fn click_on_text(ctx: &egui::Context, show_version: bool, needle: &str) -> egui::FullOutput {
        // 第一帧先把控件摆出来，才知道那段文字落在哪
        let first = run_frame(ctx, show_version, Vec::new());
        let pos = text_rect(&first, needle)
            .unwrap_or_else(|| panic!("页面上找不到 {needle:?}"))
            .center();

        // 按下与松开分在两帧：真实的点击就是跨帧的
        run_frame(
            ctx,
            show_version,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        run_frame(
            ctx,
            show_version,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        )
    }

    /// 版本页要真的把那份共享清单一行行画出来。
    #[test]
    fn version_page_renders_every_row_of_the_shared_list() {
        let texts = rendered_texts(true);

        for row in info::INFO_ROWS {
            let label = format!("{} {}", row.icon, row.label);
            assert!(
                texts.contains(&label),
                "版本页没画出标签 {label:?}；实际画出：{texts:?}"
            );
            assert!(
                texts.contains(&row.value.to_owned()),
                "版本页没画出 {} 的值 {:?}；实际画出：{texts:?}",
                row.label,
                row.value
            );
        }
    }

    /// 两页不能串场：配置页不该出现版本信息，版本页也不该出现配置项。
    #[test]
    fn the_two_pages_do_not_bleed_into_each_other() {
        let config = rendered_texts(false);
        assert!(config.contains(&"照片分类配置".to_owned()), "{config:?}");
        assert!(config.contains(&"执行".to_owned()), "{config:?}");
        assert!(!config.contains(&"版本信息".to_owned()), "{config:?}");

        let version = rendered_texts(true);
        assert!(version.contains(&"版本信息".to_owned()), "{version:?}");
        assert!(!version.contains(&"执行".to_owned()), "{version:?}");
    }

    /// 主页那一行是真链接：点一下要请求后端打开它。
    #[test]
    fn homepage_row_opens_the_url_when_clicked() {
        let ctx = egui::Context::default();

        // 没点的时候不能自己跳
        let idle = run_frame(&ctx, true, Vec::new());
        assert!(!has_open_url(&idle, info::HOMEPAGE), "没人点它就不该跳");

        let clicked = click_on_text(&ctx, true, info::HOMEPAGE);
        assert!(
            has_open_url(&clicked, info::HOMEPAGE),
            "点在主页那一行，应当请求打开 {}",
            info::HOMEPAGE
        );
    }

    /// 别的值不能被顺手做成链接：点版本号不该弹浏览器。
    #[test]
    fn plain_values_are_not_links() {
        let clicked = click_on_text(&egui::Context::default(), true, info::VERSION);

        let opened: Vec<_> = clicked
            .platform_output
            .commands
            .iter()
            .filter(|command| matches!(command, egui::OutputCommand::OpenUrl(_)))
            .collect();
        assert!(opened.is_empty(), "点版本号不该打开任何东西：{opened:?}");
    }
}
