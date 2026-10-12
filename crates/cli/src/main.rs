use clap::Parser;
use log::info;
use photo_sorter_core::{config::SortConfig, info::print_project_info};

mod cli_args;
mod logging;

fn main() {
    // 放在最前面：越早装好 logger，越不容易漏掉启动阶段的日志
    logging::init();

    let args = cli_args::CliArgs::parse();

    // 处理 version 选项
    if args.version {
        // print_ascii_art();
        print_project_info();
        return;
    }

    let config: SortConfig = args.into();
    info!("读取到配置: {config}");
}
