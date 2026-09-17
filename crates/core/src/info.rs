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

pub fn print_project_info() {
    println!("🎀 Project Name: {}", PROJECT_NAME);
    println!("📖 Description: {}", DESCRIPTION);
    println!("🥞 Version: {}", VERSION);
    println!("🐈 Authors: {}", AUTHORS);
    println!("🎮 License: {}", LICENSE);
    println!("🔗 Homepage: {}", HOMEPAGE);
}

pub fn print_ascii_art() {
    println!("{}", ASCII_ART);
}
