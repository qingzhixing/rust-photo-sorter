// 在 Windows 的 release 模式下隐藏控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
        Box::new(|_cc| Ok(Box::new(MyApp::default()))),
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
        ui.heading(info::HOMEPAGE);
    }
}
