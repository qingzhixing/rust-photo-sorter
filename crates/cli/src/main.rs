use clap::Parser;
use photo_sorter_core::config::SortConfig;

mod cli_args;

fn main() {
    let config: SortConfig = cli_args::CliArgs::parse().into();
    println!("{:?}", config);
}
