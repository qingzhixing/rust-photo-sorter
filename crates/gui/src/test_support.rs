//! 测试用的小工具：用完自动删掉的临时目录。
//!
//! 和 core 的 `test_support` 一样，刻意不引 `tempfile`——为了测试多一个依赖不划算。
//! 另外也刻意**不用** `std::env::temp_dir()`：受限环境里系统临时目录可能不可写，
//! 那会让整套测试因为一个环境问题全红，而报错信息完全指不到真正的原因。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// 进程内自增序号，避免同一纳秒里建的两个目录撞名。
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// 测试临时目录的根：构建目录下的 `test-tmp`。
fn test_root() -> PathBuf {
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
    /// 在测试根目录里建一个唯一的新目录。
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
#[cfg(test)]
pub fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("创建父目录失败");
    }
    fs::write(path, contents).expect("写文件失败");
}
