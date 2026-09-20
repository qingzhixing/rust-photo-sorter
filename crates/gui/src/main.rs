// 在 Windows 的 release 模式下隐藏控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use eframe::egui;
use photo_sorter_core::config::NoDateSortType;
use photo_sorter_core::info;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
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

#[derive(Default)]
struct MyApp {
    extract_mode: bool,
    no_date_sort_type: NoDateSortType,
    photo_count_threshold: u32,
}

impl eframe::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        ui.heading("你好，世界！");
    }
}
