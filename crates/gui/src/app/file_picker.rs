//! 文件选择对话框。
//!
//! 单独成模块，避免控制层与视图层直接依赖 [`rfd`] 这类平台相关代码。

/// 弹出系统文件夹选择框，返回用户选中的目录路径。
///
/// `current` 为当前已填写的目录，用于让对话框定位到该位置；
/// 用户取消选择时返回 [`None`]。
pub fn pick_folder(current: &str) -> Option<String> {
    let mut dialog = rfd::FileDialog::new();
    if !current.is_empty() {
        dialog = dialog.set_directory(current);
    }

    dialog.pick_folder().map(|path| path.display().to_string())
}
