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

/// 测试专用的小工具，不参与正式构建。
#[cfg(test)]
mod test_support;

use eframe::egui;
use photo_sorter_core::info;

/// 随二进制一起带走的图标位图：与 `build.rs` 编进 exe 资源的是同一张图。
const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/icon.png");

/// 交给窗口系统的图标边长。
///
/// 不必带源图那张 1024：eframe 在 Windows 上会按系统当前的图标尺寸
/// （`SM_CXICON` / `SM_CXSMICON`，随 DPI 变）自己重采样，256 够覆盖到很大的缩放。
const ICON_SIZE: u32 = 256;

fn main() -> eframe::Result<()> {
    // 放在最前面：release 下这里会把日志接回父进程的控制台
    logging::init();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(info::PROJECT_NAME)
        // 高度按「配置 + 执行 + 结果」三块留足，
        // 免得进度条和失败清单一出现就要靠滚动才能看到
        .with_inner_size([560.0, 640.0])
        .with_min_inner_size([420.0, 360.0]);

    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(info::PROJECT_NAME, options, app::build_creator())
}

/// 窗口与任务栏用的图标。
///
/// **不设的话 eframe 会自己塞一张默认图标**（黑底白 "e" 的 egui logo，见它
/// `epi_integration.rs` 里的 `load_default_egui_icon()`）——这正是「任务栏图标
/// 跟软件图标对不上」的来源。给了自己的图之后，eframe 在 Windows 上会把任务栏
/// （`ICON_BIG`）和标题栏（`ICON_SMALL`）两个槽位都按 DPI 缩放着填上，
/// macOS 的 Dock 图标与 X11 的窗口图标走的也是这份数据。
fn window_icon() -> Option<egui::IconData> {
    match image::load_from_memory(ICON_PNG) {
        Ok(source) => {
            let rgba = source
                .resize_exact(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3)
                .to_rgba8();

            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: ICON_SIZE,
                height: ICON_SIZE,
            })
        }
        Err(err) => {
            // 解不出来顶多少一枚图标，界面照常：release 下没有控制台，
            // 在这里 panic 会变成「双击了没反应」，那比少一枚图标糟得多
            log::warn!("内嵌的图标解不开，窗口图标退回默认：{err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内嵌的图必须真能解开、且长宽与通道数对得上。
    ///
    /// 这里出问题不会崩：只会静默地退回 eframe 的默认图标（黑底白 e），
    /// 从界面上一眼看不出来，所以值得钉住。
    #[test]
    fn embedded_icon_decodes_to_the_size_egui_expects() {
        let icon = window_icon().expect("内嵌的 icon.png 应当能解码");

        assert_eq!(icon.width, ICON_SIZE);
        assert_eq!(icon.height, ICON_SIZE);
        assert_eq!(icon.rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
    }

    /// 给出去的确实是我们的图：四角透明。
    ///
    /// eframe 那枚默认图标是不透明的黑底白 "e"，两者在这一点上正好相反，
    /// 所以这条断言能挡住「以为设了自己的图、其实还是默认的」这种情况。
    #[test]
    fn our_icon_is_not_the_egui_default() {
        let icon = window_icon().expect("内嵌的 icon.png 应当能解码");

        assert_eq!(icon.rgba[3], 0, "左上角应当是透明的");
    }
}
