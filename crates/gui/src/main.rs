// 在 Windows 的 release 模式下隐藏控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use eframe::egui;
use photo_sorter_core::config::{NoDateSortType, SortConfig, WorkMode};
use photo_sorter_core::info;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([520.0, 420.0]),
        ..Default::default()
    };

    eframe::run_native(
        info::PROJECT_NAME,
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();

            // 加载 HarmonyOS 字体
            let font_data = egui::FontData::from_static(include_bytes!(
                "../../../assets/font/HarmonyOS_Sans_SC_Regular.ttf"
            ));

            fonts
                .font_data
                .insert("HarmonyOS_Sans".to_owned(), Arc::new(font_data));

            // 将 HarmonyOS 字体添加到字体家族的最前面
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "HarmonyOS_Sans".to_owned());

            // 如需在等宽场景下也使用，可同样处理 Monospace 家族
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "HarmonyOS_Sans".to_owned());

            // 将配置应用到上下文
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(MyApp::default()))
        }),
    )
}

struct MyApp {
    // 正在编辑中的配置，表单控件直接绑定到这些字段
    config: SortConfig,
    // 点击「读取配置」后校验/汇总的结果
    message: Option<Result<String, String>>,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            config: SortConfig {
                photo_count_threshold: 20,
                ..Default::default()
            },
            message: None,
        }
    }
}

impl MyApp {
    /// 将表单中的配置整理成最终的 [`SortConfig`]。
    ///
    /// 输出目录留空时默认与输入目录相同，与 CLI 的行为保持一致。
    fn read_config(&self) -> Result<SortConfig, String> {
        let input_dir = self.config.input_dir.trim();
        if input_dir.is_empty() {
            return Err("输入目录不能为空".to_owned());
        }

        let output_dir = if self.config.output_dir.trim().is_empty() {
            input_dir.to_owned()
        } else {
            self.config.output_dir.trim().to_owned()
        };

        Ok(SortConfig {
            work_mode: self.config.work_mode.clone(),
            no_date_sort_type: self.config.no_date_sort_type.clone(),
            input_dir: input_dir.to_owned(),
            output_dir,
            photo_count_threshold: self.config.photo_count_threshold,
        })
    }

    /// 生成配置的摘要文本，用于在界面上确认读取结果。
    fn summarize(config: &SortConfig) -> String {
        format!(
            "工作模式: {}\n无拍摄日期照片: {}\n输入目录: {}\n输出目录: {}\n照片数量阈值: {}",
            config.work_mode,
            config.no_date_sort_type,
            config.input_dir,
            config.output_dir,
            config.photo_count_threshold,
        )
    }
}

impl eframe::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("照片分类配置");
            ui.add_space(8.0);

            egui::Grid::new("config_grid")
                .num_columns(2)
                .spacing([16.0, 10.0])
                .show(ui, |ui| {
                    // 工作模式
                    ui.label("工作模式");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.config.work_mode, WorkMode::Sort, "分类");
                        ui.radio_value(&mut self.config.work_mode, WorkMode::Extract, "提取");
                    });
                    ui.end_row();

                    // 输入目录
                    ui.label("输入目录");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.config.input_dir)
                                .desired_width(260.0)
                                .hint_text("需要处理的照片目录"),
                        );
                        if ui.button("选择…").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.config.input_dir = path.display().to_string();
                            }
                        }
                    });
                    ui.end_row();

                    // 输出目录
                    ui.label("输出目录");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.config.output_dir)
                                .desired_width(260.0)
                                .hint_text("留空则与输入目录相同"),
                        );
                        if ui.button("选择…").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.config.output_dir = path.display().to_string();
                            }
                        }
                    });
                    ui.end_row();

                    // 没有拍摄日期的照片分类标准
                    ui.label("无拍摄日期照片");
                    egui::ComboBox::from_id_salt("no_date_sort_type")
                        .selected_text(self.config.no_date_sort_type.to_string())
                        .show_ui(ui, |ui| {
                            for sort_type in [
                                NoDateSortType::ByModificationDate,
                                NoDateSortType::BySingleFolder,
                                NoDateSortType::DoNotMove,
                            ] {
                                ui.selectable_value(
                                    &mut self.config.no_date_sort_type,
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
                        egui::DragValue::new(&mut self.config.photo_count_threshold)
                            .range(1..=u32::MAX)
                            .speed(1),
                    );
                    ui.end_row();
                });

            ui.add_space(16.0);

            if ui.button("读取配置").clicked() {
                self.message = Some(match self.read_config() {
                    Ok(config) => Ok(Self::summarize(&config)),
                    Err(err) => Err(err),
                });
            }

            if let Some(message) = &self.message {
                ui.separator();
                match message {
                    Ok(summary) => {
                        ui.label("读取到的配置：");
                        ui.label(summary);
                    }
                    Err(err) => {
                        ui.colored_label(egui::Color32::RED, format!("错误：{err}"));
                    }
                }
            }
        });
    }
}
