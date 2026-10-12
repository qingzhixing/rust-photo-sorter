use clap::Parser;
use log::info;
use photo_sorter_core::{
    config::SortConfig,
    info::print_project_info,
    worker::{self, ExecuteMode},
};

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

    // 先取出执行方式，后面 args 会被消耗掉转成配置
    let mode = if args.dry_run {
        ExecuteMode::DryRun
    } else if args.copy {
        ExecuteMode::Copy
    } else {
        ExecuteMode::Move
    };

    let config: SortConfig = args.into();
    info!("读取到配置: {config}");
    info!("执行方式: {mode}");

    match worker::run(&config, mode, None) {
        Ok(report) => {
            // 结论是给用户看的正式输出，走 stdout；细节在日志里
            println!("{}", report.summary());
            for problem in &report.problems {
                eprintln!("⚠ {problem}");
            }
            if report.failed > 0 {
                std::process::exit(1);
            }
        }
        Err(err) => {
            eprintln!("错误：{err}");
            std::process::exit(1);
        }
    }
}
