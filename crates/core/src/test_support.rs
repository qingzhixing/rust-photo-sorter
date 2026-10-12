//! 测试用的小工具：临时目录 + 造带 EXIF 的图片。
//!
//! 特意没有引入 `tempfile` 之类的依赖——为了测试多一个依赖不划算，
//! 这点代码就够了。

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// 进程内自增序号，避免同一纳秒里建的两个目录撞名。
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// 测试临时目录的根。
///
/// 刻意**不用** `std::env::temp_dir()`：在受限环境里（例如低完整性沙箱）
/// 系统临时目录是不可写的，用它会让整个测试套件因为一个**环境**问题全红，
/// 而报错信息（一句「拒绝访问」）完全指不到真正的原因。
///
/// 放在构建目录旁边还有两个好处：跟着 `cargo clean` 一起被清掉，
/// 且和被测文件处在同一卷上，跨盘退化之类的分支不会被意外触发。
fn test_root() -> PathBuf {
    // 集成测试/基准测试时 cargo 会设好这个变量；`cargo test --lib` 没有
    if let Some(dir) = std::env::var_os("CARGO_TARGET_TMPDIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("test-tmp")
}

/// 用完自动删掉的临时目录。
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 在构建目录下的测试根目录里建一个唯一的新目录。
    pub fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = test_root().join(format!("{tag}-{}-{nanos}-{seq}", std::process::id()));

        fs::create_dir_all(&path).expect("创建临时目录失败");
        Self { path }
    }

    /// 临时目录的根路径。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// 写一个文本文件，自动创建父目录。
pub fn write(path: &Path, contents: &str) {
    write_bytes(path, contents.as_bytes());
}

/// 写一个二进制文件，自动创建父目录。
pub fn write_bytes(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("创建父目录失败");
    }
    fs::write(path, contents).expect("写文件失败");
}

/// 造一张带 `DateTimeOriginal` 的 TIFF 图片。
///
/// 用 `exif` 自带的 writer 生成，而不是手写字节：它产出的正是 TIFF 容器，
/// `exif::Reader` 能原样读回来，于是用到它的测试覆盖的是**真实的读 EXIF 链路**，
/// 不是打桩。
///
/// `datetime` 要用 EXIF 的格式：`2026:07:30 08:05:09`。
pub fn write_exif_tiff(path: &Path, datetime: &str) {
    use exif::experimental::Writer;
    use exif::{Field, In, Tag, Value};

    let field = Field {
        tag: Tag::DateTimeOriginal,
        ifd_num: In::PRIMARY,
        value: Value::Ascii(vec![datetime.as_bytes().to_vec()]),
    };

    let mut writer = Writer::new();
    writer.push_field(&field);

    let mut buffer = Cursor::new(Vec::new());
    writer.write(&mut buffer, false).expect("生成 EXIF 失败");

    write_bytes(path, &buffer.into_inner());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photo;

    #[test]
    fn generated_tiff_can_be_read_back() {
        let tmp = TempDir::new("test-support");
        let path = tmp.path().join("shot.tif");
        write_exif_tiff(&path, "2026:07:30 08:05:09");

        let read = photo::read_exif_datetime(&path).expect("应当能读回拍摄时间");
        assert_eq!(read.to_string(), "2026-07-30 08:05:09");
    }

    #[test]
    fn files_without_exif_yield_no_datetime() {
        let tmp = TempDir::new("test-support-noexif");
        let path = tmp.path().join("plain.jpg");
        write(&path, "definitely not a jpeg");

        assert_eq!(photo::read_exif_datetime(&path), None);
    }
}
