//! 构建脚本：把 `assets/icon/icon.ico` 嵌进 Windows 的 exe。
//!
//! 与 `crates/gui/build.rs` 是同一份逻辑，有意重复而不是抽公共 crate：
//! 一共十几行，为它多一个 workspace 成员不划算。
//!
//! 只有 Windows 的图标是「编译进可执行文件」的，macOS / Linux 的图标
//! 分别属于 `.app` 包与 `.desktop` 文件，所以这里只在 Windows 上做。
//!
//! 换了图标要重新出一份 `assets/icon/icon.ico`，
//! 出图的办法（逐档从矢量渲染）写在 gui 那份的注释里。

fn main() {
    #[cfg(windows)]
    {
        use std::path::Path;

        println!("cargo:rerun-if-changed=../../assets/icon/icon.ico");

        let icon = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon.to_str().expect("图标路径应当是合法 UTF-8"));
        res.compile().expect("嵌入 Windows 图标资源失败");
    }
}
