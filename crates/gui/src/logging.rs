//! 图形界面入口的日志初始化。
//!
//! 这里要解决的核心问题是：GUI 在 release 下标记了
//! `windows_subsystem = "windows"`（见 `crates/gui/src/main.rs`），
//! 进程默认**不附带控制台**，`env_logger` 写往 stderr 的内容会无处可去。
//!
//! 处理策略（Windows）：
//!
//! 1. **stderr 可用** → 原样交给 stderr，尊重用户的 `2> log.txt`。
//!    实测从终端启动的 release 版 GUI 自己虽然没有控制台
//!    （`GetConsoleWindow()` 返回 null），但标准句柄是从父进程继承来的、
//!    依然有效，所以这一条已经覆盖了日常「在终端里跑」的场景。
//! 2. **stderr 不可用** → 附加到父进程的控制台，把日志直接写进那个终端。
//! 3. **连可借的控制台也没有**（典型情况：在资源管理器里双击启动）→
//!    默认放弃，日志静默丢弃，不会平白弹出黑窗；
//!    设置环境变量 `PHOTO_SORTER_ALLOC_CONSOLE=1` 可改为新建一个控制台窗口。
//!
//! 想让双击启动**总是**弹出控制台，把 `console_writer` 里那个环境变量判断去掉即可。
//!
//! 另外，release 构建默认**不输出日志**（见 [`RELEASE_DEFAULT`]）：
//! 正式发布的 GUI 不该因为用户碰巧从终端启动就往人家终端里刷日志，
//! 需要排查时显式设置 `RUST_LOG` 打开即可。

use std::sync::Once;

static INIT: Once = Once::new();

/// 读取日志级别的环境变量。
pub const LOG_ENV: &str = "RUST_LOG";

/// 双击启动时用来要求新建控制台窗口的环境变量。
///
/// 只有 Windows 会去借/新建控制台，所以这个常量跟着 `platform` 模块一起按平台裁剪，
/// 免得非 Windows 构建里它「没人用」而被 `clippy -D warnings` 拦下。
#[cfg(windows)]
pub const ALLOC_CONSOLE_ENV: &str = "PHOTO_SORTER_ALLOC_CONSOLE";

/// debug 构建未设置 [`LOG_ENV`] 时的默认级别。
const DEBUG_DEFAULT: &str = "info";

/// release 构建未设置 [`LOG_ENV`] 时的默认级别。
///
/// 默认 `off`：发布的 GUI 静默运行，不给用户添噪音。
/// 调试 release 版时用 `RUST_LOG=debug` 打开，或把这里改成 `"info"`。
const RELEASE_DEFAULT: &str = "off";

/// 初始化全局 logger；重复调用是安全的空操作。
///
/// - 级别由 `RUST_LOG` 控制；**未设置时，debug 构建用 `info`，release 构建静默**。
/// - 有控制台时写到控制台，否则退回 stderr。
pub fn init() {
    INIT.call_once(|| {
        let env = env_logger::Env::default().filter_or(LOG_ENV, default_level());
        let mut builder = env_logger::Builder::from_env(env);
        builder.format_timestamp_millis();

        if let Some(console) = platform::console_writer() {
            builder.target(env_logger::Target::Pipe(console));
        }

        // 已经被别处装过 logger 时静默跳过，不 panic
        let _ = builder.try_init();
    });
}

/// 未设置 `RUST_LOG` 时使用的默认级别。
///
/// 用 `debug_assertions` 判断构建类型：`cargo build --release` 默认关掉它，
/// 于是 release 走 [`RELEASE_DEFAULT`]。
/// 注意自定义 profile 若显式打开 `debug-assertions`，这里会被当成 debug 构建。
fn default_level() -> &'static str {
    if cfg!(debug_assertions) {
        DEBUG_DEFAULT
    } else {
        RELEASE_DEFAULT
    }
}

#[cfg(windows)]
mod platform {
    use std::fs::OpenOptions;
    use std::io;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, GetConsoleWindow, GetStdHandle,
        STD_ERROR_HANDLE, WriteConsoleW,
    };

    /// 把日志写到 Windows 控制台。
    ///
    /// 走 `WriteConsoleW`（宽字符）而不是直接写字节：日志内容是中文，
    /// 直接写 UTF-8 字节会被控制台按当前代码页（国内默认 GBK）解码成乱码。
    pub(super) struct ConsoleWriter {
        /// 存成 `isize` 而不是 `HANDLE`（`*mut c_void` 不是 `Send`），
        /// 这样 [`ConsoleWriter`] 天然满足 `env_logger` 要求的 `Send`。
        handle: isize,
        /// 持有打开 `CONOUT$` 得到的文件，保证上面的句柄一直有效。
        _file: std::fs::File,
    }

    impl io::Write for ConsoleWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }

            // 日志内容始终是 UTF-8；解码成 UTF-16 后交给控制台。
            // 用 lossy 是为了万一有半截多字节序列也不至于 panic 或丢日志。
            let text = String::from_utf8_lossy(buf);
            let wide: Vec<u16> = text.encode_utf16().collect();

            let mut written = 0u32;
            // SAFETY: handle 来自下方仍然存活的 File；wide 在整个调用期间有效。
            let ok = unsafe {
                WriteConsoleW(
                    self.handle as HANDLE,
                    wide.as_ptr(),
                    wide.len() as u32,
                    &mut written,
                    std::ptr::null(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }

            // 控制台缓冲区满时可能只写进去一部分，对日志来说可接受，
            // 这里按「全部写完」上报，避免 env_logger 反复重试。
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            // 控制台没有用户态缓冲，无需 flush
            Ok(())
        }
    }

    /// stderr 是否已经接到某个真实的目标上。
    ///
    /// 从终端启动时标准句柄会被继承下来（GUI 子系统进程也一样），
    /// 这种情况下就不该去抢控制台，免得破坏 `2> file` 重定向。
    fn stderr_is_usable() -> bool {
        // SAFETY: 不接收指针参数。
        let handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        !handle.is_null() && handle != INVALID_HANDLE_VALUE
    }

    /// 用户是否通过环境变量要求「没有控制台就新建一个」。
    ///
    /// 双击启动的 release 版 GUI 本来是完全静默的，这个开关让排查问题时
    /// 不必重新编译就能拿到一个显示日志的终端窗口。
    fn want_console_window() -> bool {
        std::env::var_os(super::ALLOC_CONSOLE_ENV).is_some_and(|v| !v.is_empty() && v != "0")
    }

    /// 返回一个指向控制台的输出流；没有可用控制台时返回 `None`（退回 stderr）。
    pub(super) fn console_writer() -> Option<Box<dyn io::Write + Send>> {
        // 已经能写 stderr 就别抢：这样 debug 构建和输出重定向都保持原样
        if stderr_is_usable() {
            return None;
        }

        // SAFETY: 这几个函数都不接受指针参数，失败时只返回 null / 0。
        let has_console = unsafe {
            if !GetConsoleWindow().is_null() {
                true
            } else if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
                // 借用启动它的那个终端
                true
            } else {
                // 双击启动：没有父控制台可借。只有用户明确要求时才新开一个窗口，
                // 否则会平白弹出一个黑窗。
                want_console_window() && AllocConsole() != 0
            }
        };
        if !has_console {
            return None;
        }

        // `CONOUT$` 是 Win32 的控制台输出设备名，打开它就等于拿到控制台句柄。
        let file = OpenOptions::new().write(true).open("CONOUT$").ok()?;

        Some(Box::new(ConsoleWriter {
            handle: file.as_raw_handle() as isize,
            _file: file,
        }))
    }
}

#[cfg(not(windows))]
mod platform {
    use std::io;

    /// 非 Windows 平台 release 版照样带着 stderr，直接用 stderr 即可。
    pub(super) fn console_writer() -> Option<Box<dyn io::Write + Send>> {
        None
    }
}
