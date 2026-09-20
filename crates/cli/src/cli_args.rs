use clap::Parser;
use photo_sorter_core::config::{NoDateSortType, SortConfig};
use photo_sorter_core::info::{
    AUTHORS, DESCRIPTION, PROJECT_NAME, VERSION
};

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
#[command(
	name = PROJECT_NAME, 
	version = VERSION, 
	about = DESCRIPTION,
    author = AUTHORS,
    disable_version_flag = true,     // 关掉内置 --version 选项
)]
pub(crate) struct CliArgs {
	/// 显示版本信息
    #[arg(short = 'V', long = "version")]
    pub version: bool,

    /// 没有拍摄日期的照片分类标准
    #[arg(
        long,
        default_value = "by-single-folder",
        value_parser = parse_no_date_sort_type,
    )]
    no_date_sort_type: NoDateSortType,

    /// 输入目录
    #[arg(short, long, required_unless_present = "version")]
    input_dir: Option<String>,

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
            .unwrap_or_else(|| cli_args.input_dir.clone().unwrap());
        SortConfig {
            no_date_sort_type: cli_args.no_date_sort_type,
            input_dir: cli_args.input_dir.unwrap(),
            output_dir,
            photo_count_threshold: cli_args.photo_count_threshold,
        }
    }
}
