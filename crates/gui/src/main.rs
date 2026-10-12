// 在 Windows 的 release 模式下隐藏控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! 照片分类程序的图形界面。
//!
//! 分层约定见 [`app`]：
//! - [`app::config_form`]：控制层的数据与校验逻辑，不依赖 egui
//! - [`app::view`]：视图层，只负责渲染并上报用户操作
//! - [`app`]：控制层，持有状态并响应操作

mod app;
mod logging;

use eframe::egui;
use photo_sorter_core::info;

fn main() -> eframe::Result<()> {
    // 放在最前面：release 下这里会把日志接回父进程的控制台
    logging::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([520.0, 420.0]),
        ..Default::default()
    };

    eframe::run_native(info::PROJECT_NAME, options, app::build_creator())
}
