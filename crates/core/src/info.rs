pub const PROJECT_NAME: &str = "rust photo sorter";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
pub const LICENSE: &str = env!("CARGO_PKG_LICENSE");
pub const HOMEPAGE: &str = env!("CARGO_PKG_HOMEPAGE");
pub const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
const ASCII_ART: &str = r#"
  ____               _     ____   _             _           ____                _              
 |  _ \  _   _  ___ | |_  |  _ \ | |__    ___  | |_  ___   / ___|   ___   _ __ | |_  ___  _ __ 
 | |_) || | | |/ __|| __| | |_) || '_ \  / _ \ | __|/ _ \  \___ \  / _ \ | '__|| __|/ _ \| '__|
 |  _ < | |_| |\__ \| |_  |  __/ | | | || (_) || |_| (_) |  ___) || (_) || |   | |_|  __/| |   
 |_| \_\ \__,_||___/ \__| |_|    |_| |_| \___/  \__|\___/  |____/  \___/ |_|    \__|\___||_|   
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_core_info() {
        println!("Project Name: {}", PROJECT_NAME);
        println!("Version: {}", VERSION);
        println!("Authors: {}", AUTHORS);
        println!("License: {}", LICENSE);
        println!("Homepage: {}", HOMEPAGE);
        println!("Description: {}", DESCRIPTION);
        println!("{}", ASCII_ART);
    }
}
