use clap::Parser;
use photo_sorter_core::config::{NoDateSortType, SortConfig};

fn parse_no_date_sort_type(s: &str) -> Result<NoDateSortType, String> {
    match s {
        "by-modification-date" => Ok(NoDateSortType::ByModificationDate),
        "by-single-folder" => Ok(NoDateSortType::BySingleFolder),
        "do-not-move" => Ok(NoDateSortType::DoNotMove),
        other => Err(format!(
            "未知取值 `{other}`，可选: by-modification-date, by-single-folder, do-not-move"
        )),
    }
}

#[derive(Debug, Parser)]
#[command(name = "photo-sorter", version, about = "按日期整理照片")]
pub(crate) struct CliArgs {
    /// 没有拍摄日期的照片分类标准
    #[arg(
        long,
        default_value = "by-single-folder",
        value_parser = parse_no_date_sort_type,
    )]
    no_date_sort_type: NoDateSortType,

    /// 输入目录
    #[arg(short, long)]
    input_dir: String,

    /// 输出目录（不填则与输入目录相同）
    #[arg(short, long)]
    output_dir: Option<String>,

    /// 照片数量阈值
    #[arg(short = 't', long, default_value_t = 20)]
    photo_count_threshold: u32,
}

impl From<CliArgs> for SortConfig {
    fn from(cli_args: CliArgs) -> Self {
        let output_dir = cli_args
            .output_dir
            .unwrap_or_else(|| cli_args.input_dir.clone());
        SortConfig {
            no_date_sort_type: cli_args.no_date_sort_type,
            input_dir: cli_args.input_dir,
            output_dir,
            photo_count_threshold: cli_args.photo_count_threshold,
        }
    }
}
