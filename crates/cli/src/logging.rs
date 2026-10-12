//! 命令行入口的日志初始化。
//!
//! `photo-sorter-core` 只依赖 `log` 门面并负责写日志，
//! 具体装哪个 logger、写到哪、默认什么级别，由二进制入口决定。

use std::sync::Once;

static INIT: Once = Once::new();

/// 读取日志级别的环境变量。
pub const LOG_ENV: &str = "RUST_LOG";

/// debug 构建未设置 [`LOG_ENV`] 时的默认级别。
const DEBUG_DEFAULT: &str = "info";

/// release 构建未设置 [`LOG_ENV`] 时的默认级别。
///
/// 默认 `off`：正式发布的程序不主动往用户终端里塞日志，
/// 需要排查时显式设置 `RUST_LOG` 即可打开。
/// 如果希望 release 版也默认打印读取到的配置，把它改成 `"info"`。
const RELEASE_DEFAULT: &str = "off";

/// 初始化全局 logger；重复调用是安全的空操作。
///
/// - 级别由 `RUST_LOG` 控制；**未设置时，debug 构建用 `info`，release 构建静默**。
/// - 日志写到 stderr，不污染 stdout 上真正要交付的数据。
pub fn init() {
    INIT.call_once(|| {
        let env = env_logger::Env::default().filter_or(LOG_ENV, default_level());

        // 已经被别处装过 logger 时静默跳过，不 panic
        let _ = env_logger::Builder::from_env(env)
            .format_timestamp_millis()
            .try_init();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 这个测试跑在 debug 下，所以默认级别应当是 info。
    #[test]
    fn debug_build_defaults_to_info() {
        assert_eq!(default_level(), "info");
    }

    #[test]
    fn release_build_defaults_to_silence() {
        assert_eq!(RELEASE_DEFAULT, "off");
    }
}
