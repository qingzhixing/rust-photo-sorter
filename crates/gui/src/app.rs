//! 配置窗口的**控制层**。
//!
//! [`PhotoSorterApp`] 持有配置表单与结果状态，负责响应视图上报的
//! [`ViewAction`]，并把要展示的内容整理好交给视图渲染。
//! 除实现 [`eframe::App`] 所需的入口外，这里不做任何界面布局。

pub(crate) mod config_form;
mod file_picker;
pub(crate) mod view;

use eframe::egui;

use self::config_form::ConfigForm;
use self::view::{ReadMessage, ViewAction};

/// 照片分类的配置界面。
#[derive(Default)]
pub struct PhotoSorterApp {
    /// 正在编辑中的配置，表单控件直接绑定到这些字段
    form: ConfigForm,
    /// 点击「读取配置」后校验/汇总的结果
    message: Option<ReadMessage>,
}

impl PhotoSorterApp {
    /// 读取当前表单，并把结果（摘要或错误）记录到待展示状态。
    ///
    /// 校验、补全之后的最终配置会写进日志，方便排查「界面上填的」和
    /// 「实际拿去分类的」不一致的问题。
    fn read_config(&mut self) {
        self.message = Some(match self.form.read_config() {
            Ok(config) => {
                log::info!("读取到配置: {config}");
                ReadMessage::Summary(ConfigForm::summarize(&config))
            }
            Err(err) => {
                log::warn!("读取配置失败: {err}");
                ReadMessage::Error(err)
            }
        });
    }

    /// 处理视图上报的操作。
    fn handle(&mut self, action: ViewAction) {
        match action {
            ViewAction::ReadConfig => self.read_config(),
            ViewAction::PickInputDir => {
                if let Some(dir) = file_picker::pick_folder(&self.form.input_dir) {
                    self.form.input_dir = dir;
                }
            }
            ViewAction::PickOutputDir => {
                if let Some(dir) = file_picker::pick_folder(&self.form.output_dir) {
                    self.form.output_dir = dir;
                }
            }
        }
    }
}

impl eframe::App for PhotoSorterApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(action) = view::show(ui, &mut self.form, self.message.as_ref()) {
            self.handle(action);
        }
    }
}

/// 注册窗口与字体，返回 eframe 需要的应用构造函数。
pub fn build_creator() -> eframe::AppCreator<'static> {
    Box::new(|cc| {
        install_fonts(&cc.egui_ctx);
        Ok(Box::new(PhotoSorterApp::default()))
    })
}

/// 加载 HarmonyOS 字体并设为默认字体。
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 加载 HarmonyOS 字体
    let font_data = egui::FontData::from_static(include_bytes!(
        "../../../assets/font/HarmonyOS_Sans_SC_Regular.ttf"
    ));

    fonts
        .font_data
        .insert("HarmonyOS_Sans".to_owned(), std::sync::Arc::new(font_data));

    // 将 HarmonyOS 字体添加到字体家族的最前面
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "HarmonyOS_Sans".to_owned());
    }

    // 将配置应用到上下文
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_config_reports_error_for_empty_input_dir() {
        let mut app = PhotoSorterApp::default();
        app.handle(ViewAction::ReadConfig);

        assert_eq!(
            app.message,
            Some(ReadMessage::Error("输入目录不能为空".to_owned()))
        );
    }

    #[test]
    fn read_config_reports_summary_for_valid_form() {
        let mut app = PhotoSorterApp {
            form: ConfigForm {
                input_dir: "D:\\photos".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        };
        app.handle(ViewAction::ReadConfig);

        match app.message {
            Some(ReadMessage::Summary(summary)) => {
                assert!(summary.contains("输入目录: D:\\photos"), "{summary}");
                assert!(summary.contains("输出目录: D:\\photos"), "{summary}");
            }
            other => panic!("期望得到成功摘要，实际为 {other:?}"),
        }
    }
}
