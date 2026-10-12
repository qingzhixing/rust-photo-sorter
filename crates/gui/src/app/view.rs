//! 配置表单的**视图层**。
//!
//! 这里只负责把 [`ConfigForm`] 渲染出来，并把用户操作翻译成 [`ViewAction`]
//! 返回给控制层；视图本身不修改数据、不做校验、也不执行任何业务逻辑。

use eframe::egui;

use photo_sorter_core::config::{NoDateSortType, WorkMode};

use super::config_form::ConfigForm;

/// 视图向控制层汇报的用户操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewAction {
    /// 「读取配置」按钮被点击
    ReadConfig,
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
/// 返回本轮界面产生的操作；需要交换多个操作时，后续可以改成容量足够的小集合。
pub fn show(
    ui: &mut egui::Ui,
    form: &mut ConfigForm,
    message: Option<&ReadMessage>,
) -> Option<ViewAction> {
    let mut action = None;

    egui::CentralPanel::default().show(ui, |ui| {
        ui.heading("照片分类配置");
        ui.add_space(8.0);

        egui::Grid::new("config_grid")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                action = config_fields(ui, form).or(action);
            });

        ui.add_space(16.0);

        if ui.button("读取配置").clicked() {
            action = Some(ViewAction::ReadConfig);
        }

        if let Some(message) = message {
            ui.separator();
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
    });

    action
}

/// 渲染表单中的各个字段，收集字段级别的操作。
fn config_fields(ui: &mut egui::Ui, form: &mut ConfigForm) -> Option<ViewAction> {
    let mut action = None;

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
                .desired_width(260.0)
                .hint_text("需要处理的照片目录"),
        );
        if ui.button("选择…").clicked() {
            action = Some(ViewAction::PickInputDir);
        }
    });
    ui.end_row();

    // 输出目录
    ui.label("输出目录");
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut form.output_dir)
                .desired_width(260.0)
                .hint_text("留空则与输入目录相同"),
        );
        if ui.button("选择…").clicked() {
            action = Some(ViewAction::PickOutputDir);
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
    ui.add(
        egui::DragValue::new(&mut form.photo_count_threshold)
            .range(1..=u32::MAX)
            .speed(1),
    );
    ui.end_row();

    action
}
