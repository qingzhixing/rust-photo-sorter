//! 构建脚本：把 `assets/icon/icon.ico` 嵌进 Windows 的 exe。
//!
//! 只有 Windows 的图标是「编译进可执行文件」的：
//! macOS 的图标属于 `.app` 包、Linux 的属于 `.desktop` 文件，
//! 都不是可执行文件自己能带的，所以这里只在 Windows 上做。
//!
//! `assets/icon/` 里 `icon.svg` 是矢量源图，`icon.ico` 是它的产物
//! （`icon.png` 是同一张图的位图母版，给以后做 macOS 的 `.icns` 或文档用）；
//! 仓库里只放生成好的文件，不放生成脚本。
//!
//! 改了 SVG 就得重新出一份 `.ico`，要点是**7 档尺寸（16/24/32/48/64/128/256）
//! 逐档从矢量渲染**再拼成一个 ico，别渲一张大图再缩——16×16 那档两者平均差 17/255，
//! 直接渲染明显更干净；少一档的话 Explorer 对应那一档视图也会被系统硬放大。
//! 渲染随便挑个能读 SVG 的（cairosvg / Inkscape / resvg），
//! 本机 Windows 上没有 libcairo，一般在 WSL 里做。

fn main() {
    // 这里判的是构建脚本所在的宿主平台。本项目从不交叉编译（CI 也是本机编译），
    // 宿主就等于目标，所以这个判断够用。
    #[cfg(windows)]
    {
        use std::path::Path;

        // 图标改了要重新跑构建脚本，否则 cargo 会一直用上一次编出来的资源
        println!("cargo:rerun-if-changed=../../assets/icon/icon.ico");

        let icon = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon.to_str().expect("图标路径应当是合法 UTF-8"));
        res.compile().expect("嵌入 Windows 图标资源失败");
    }
}
