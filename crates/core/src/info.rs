pub const CORE_NAME: &str = env!("CARGO_PKG_NAME");
pub const PROJECT_NAME: &str = "Rust Photo Sorter";
pub const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
pub const LICENSE: &str = env!("CARGO_PKG_LICENSE");
pub const HOMEPAGE: &str = env!("CARGO_PKG_HOMEPAGE");
pub const ASCII_ART: &str = r#"
  ____               _     ____   _             _           ____                _              
 |  _ \  _   _  ___ | |_  |  _ \ | |__    ___  | |_  ___   / ___|   ___   _ __ | |_  ___  _ __ 
 | |_) || | | |/ __|| __| | |_) || '_ \  / _ \ | __|/ _ \  \___ \  / _ \ | '__|| __|/ _ \| '__|
 |  _ < | |_| |\__ \| |_  |  __/ | | | || (_) || |_| (_) |  ___) || (_) || |   | |_|  __/| |   
 |_| \_\ \__,_||___/ \__| |_|    |_| |_| \___/  \__|\___/  |____/  \___/ |_|    \__|\___||_|   
"#;

/// 版本信息的一行：图标 + 标签 + 值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InfoRow {
    pub icon: &'static str,
    pub label: &'static str,
    pub value: &'static str,
    /// 值本身是个可以点开的地址（GUI 渲染成链接；CLI 只把它当普通文本打出来）
    pub url: Option<&'static str>,
}

impl InfoRow {
    /// 一行普通文本。
    const fn text(icon: &'static str, label: &'static str, value: &'static str) -> Self {
        Self {
            icon,
            label,
            value,
            url: None,
        }
    }

    /// 一行地址：显示的就是它，点开也是它。
    const fn link(icon: &'static str, label: &'static str, url: &'static str) -> Self {
        Self {
            icon,
            label,
            value: url,
            url: Some(url),
        }
    }
}

/// 版本信息的全部内容。
///
/// CLI 的 `-V`（[`print_project_info`]）与 GUI 的版本页都念这一份清单：
/// 两个前端显示的东西必须一致，所以只在这一处定义，谁都别自己拼字符串。
///
/// 挑 emoji 时注意**两个前端都要能显示**：终端会自己找字体，GUI 只能用 egui
/// 装进去的那几个（项目的 HarmonyOS + egui 自带的 Ubuntu-Light、NotoEmoji、
/// emoji-icon-font）。不在这些字体里的话，命令行看着正常，界面上却是个方框——
/// 原来的 🥞 就是这么被换成 🔖 的。
pub const INFO_ROWS: &[InfoRow] = &[
    InfoRow::text("🎀", "Project Name", PROJECT_NAME),
    InfoRow::text("📖", "Description", DESCRIPTION),
    InfoRow::text("🔖", "Version", VERSION),
    InfoRow::text("🐈", "Authors", AUTHORS),
    InfoRow::text("🎮", "License", LICENSE),
    InfoRow::link("🔗", "Homepage", HOMEPAGE),
];

pub fn print_project_info() {
    for row in INFO_ROWS {
        println!("{} {}: {}", row.icon, row.label, row.value);
    }
}

pub fn print_ascii_art() {
    println!("{}", ASCII_ART);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 行与顺序都不能悄悄变：CLI 的输出和 GUI 的版本页都照着它走。
    #[test]
    fn info_rows_keep_the_agreed_labels_in_order() {
        let labels: Vec<&str> = INFO_ROWS.iter().map(|row| row.label).collect();
        assert_eq!(
            labels,
            [
                "Project Name",
                "Description",
                "Version",
                "Authors",
                "License",
                "Homepage"
            ]
        );
    }

    /// 版本号跟着 Cargo.toml 走，不能在清单里写死一份。
    #[test]
    fn version_row_follows_the_manifest() {
        let row = INFO_ROWS
            .iter()
            .find(|row| row.label == "Version")
            .expect("清单里必须有 Version 这一行");
        assert_eq!(row.value, env!("CARGO_PKG_VERSION"));
        assert_eq!(row.value, VERSION);
    }

    /// 每行三样都得有：空着在界面上就是缺一块，或者打印出「:  」这种半截货。
    #[test]
    fn no_row_is_empty() {
        for row in INFO_ROWS {
            assert!(!row.icon.is_empty(), "{row:?}");
            assert!(!row.label.is_empty(), "{row:?}");
            assert!(!row.value.is_empty(), "{row:?}");
        }
    }

    /// 可点的只有主页。别的行要是混进一个地址，界面上就会冒出一个
    /// 意料之外的链接；而主页漏了这个标记，就点不开了。
    #[test]
    fn only_the_homepage_row_is_a_link() {
        let links: Vec<&str> = INFO_ROWS
            .iter()
            .filter(|row| row.url.is_some())
            .map(|row| row.label)
            .collect();
        assert_eq!(links, ["Homepage"]);

        let homepage = INFO_ROWS
            .iter()
            .find(|row| row.label == "Homepage")
            .expect("清单里必须有 Homepage 这一行");
        assert_eq!(homepage.url, Some(HOMEPAGE));
        assert_eq!(homepage.value, HOMEPAGE, "显示出来的也应当是那个地址");
    }
}
