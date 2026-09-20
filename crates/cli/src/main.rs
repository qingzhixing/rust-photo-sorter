use clap::Parser;
use photo_sorter_core::{
    config::SortConfig,
    info::{print_ascii_art, print_project_info},
};

mod cli_args;

fn main() {
    let args = cli_args::CliArgs::parse();

    // 处理 version 选项
    if args.version {
        print_ascii_art();
        print_project_info();
        return;
    }

    let config: SortConfig = args.into();
    println!("{:?}", config);
}
