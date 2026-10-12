use clap::{Parser, ValueEnum};
use photo_sorter_core::config::{NoDateSortType, SortConfig, WorkMode};
use photo_sorter_core::info::{AUTHORS, DESCRIPTION, PROJECT_NAME, VERSION};

/// `--sort-type` 的全部取值：都是「照片确实没有日期」时的处置方式。
///
/// 之所以单独定义一个枚举而不是直接复用 [`NoDateSortType`]，是为了让 clap 自动
/// 列出候选值（`-h` 里的 `[possible values: ...]`、`--help` 里每个取值一行说明）——
/// core 不该为了 CLI 的展示需求去依赖 clap。
///
/// 「从文件名里读时间」不在这个清单里：它是**拍摄日期的来源之一**
/// （见 `--parse-date-from-filename`），不是没有日期时的兜底策略。
#[derive(Debug, Clone, Copy, ValueEnum)]
enum SortTypeArg {
    /// 用文件的修改时间代替拍摄时间
    ByModificationDate,
    /// 统一放进「无拍摄日期」目录
    BySingleFolder,
    /// 留在原地不动
    DoNotMove,
}

impl From<SortTypeArg> for NoDateSortType {
    fn from(value: SortTypeArg) -> Self {
        match value {
            SortTypeArg::ByModificationDate => NoDateSortType::ByModificationDate,
            SortTypeArg::BySingleFolder => NoDateSortType::BySingleFolder,
            SortTypeArg::DoNotMove => NoDateSortType::DoNotMove,
        }
    }
}

/// `--sort-type` 各取值的说明表，附在帮助末尾。
///
/// 直接从枚举的 `ValueEnum` 元数据生成，所以**取值清单和说明都只有一处定义**
/// （就是上面那些变体和它们的文档注释），不会出现"加了变体却忘了改帮助"。
///
/// 之所以不用 clap 自带的 `Possible values:` 列表：那个只在 `--help`（长帮助）里
/// 带说明，`-h`（短帮助）里只渲染取值名。挪到 `after_help` 则两边都能看到。
fn sort_type_help() -> String {
    let mut table = String::from("--sort-type 的取值:\n");

    for variant in SortTypeArg::value_variants() {
        if let Some(value) = variant.to_possible_value() {
            let help = value
                .get_help()
                .map(|text| text.to_string())
                .unwrap_or_default();
            table.push_str(&format!("  {:<22}{help}\n", value.get_name()));
        }
    }

    // 直接报错会把老命令的人卡在「换个值就行」的猜测里，索性说清替代写法
    table.push_str("\n拍摄日期依次取自 EXIF、文件名（见 --no-filename-date）；\n两者都没有时，才按上面的取值处理。");
    table
}

/// `--no-filename-date` 的说明。
///
/// 单独拎出来是因为它要在**两处**出现：参数自己的帮助，以及 `--sort-type`
/// 里 `from-filename` 被拒时给出的替代提示。两处措辞不一致的话，
/// 用户会以为它们是两件不同的事。
const FILENAME_DATE_HELP: &str = "不从文件名里解读拍摄日期（默认会解读：截图类工具不写 EXIF，\
     却把时间戳放进文件名，关掉后这类照片只能按 --sort-type 处理）";

#[derive(Debug, Parser)]
#[command(
	name = PROJECT_NAME,
	version = VERSION,
	about = DESCRIPTION,
    author = AUTHORS,
    disable_version_flag = true,     // 关掉内置 --version 选项
    after_help = sort_type_help(),   // 取值说明表，-h 和 --help 都会显示
)]
pub(crate) struct CliArgs {
    /// 显示版本信息
    #[arg(short = 'V', long = "version")]
    pub version: bool,

    /// 有此标记时,提取目标目录及其子目录所有照片并删除空目录
    #[arg(long = "extract")]
    pub extract_mode: bool,

    /// 没有拍摄日期的照片 分类标准（取值见帮助末尾）
    #[arg(
        long = "sort-type",
        value_enum,
        value_name = "TYPE",
        default_value = "by-single-folder",
        // 说明表统一放在 after_help 里，避免在 --help 里显示两遍
        hide_possible_values = true,
    )]
    no_date_sort_type: SortTypeArg,

    /// 输入目录
    #[arg(short, long, required_unless_present = "version")]
    input_dir: Option<String>,

    /// 输出目录（不填则与输入目录相同）
    #[arg(short, long)]
    output_dir: Option<String>,

    /// 照片数量阈值
    #[arg(short = 't', long, default_value_t = 20)]
    photo_count_threshold: u32,

    /// 不从文件名里解读拍摄日期
    #[arg(long = "no-filename-date", help = FILENAME_DATE_HELP)]
    no_filename_date: bool,

    /// 试运行
    #[arg(long = "dry-run", conflicts_with = "copy")]
    pub dry_run: bool,

    /// 复制而不是移动
    #[arg(long = "copy")]
    pub copy: bool,
}

impl From<CliArgs> for SortConfig {
    fn from(cli_args: CliArgs) -> Self {
        let input_dir = cli_args.input_dir.unwrap();
        let work_mode = if cli_args.extract_mode {
            WorkMode::Extract
        } else {
            WorkMode::Sort
        };
        let output_dir = cli_args.output_dir.unwrap_or_else(|| input_dir.clone());
        SortConfig {
            work_mode,
            no_date_sort_type: cli_args.no_date_sort_type.into(),
            parse_date_from_filename: !cli_args.no_filename_date,
            input_dir,
            output_dir,
            photo_count_threshold: cli_args.photo_count_threshold,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// 命令行上暴露的取值清单。
    ///
    /// 这个断言是**故意写死**的：以后给 `NoDateSortType` 加了变体却忘了在
    /// `SortTypeArg` 里补上，或者改了取值拼写，这里会立刻失败。
    ///
    /// 只有 3 个：文件名解析已经独立成 `--no-filename-date`，
    /// 留在 `--sort-type` 里会让「照片到底有没有日期」有两个说法。
    const EXPECTED: [&str; 3] = ["by-modification-date", "by-single-folder", "do-not-move"];

    /// 渲染短帮助（也就是 `-h` 实际打印的内容）。
    fn short_help() -> String {
        let mut command = CliArgs::command();
        let mut buffer = Vec::new();
        command.write_help(&mut buffer).expect("渲染 -h 失败");
        String::from_utf8(buffer).expect("-h 输出应当是 UTF-8")
    }

    #[test]
    fn short_help_introduces_every_sort_type() {
        // 直接检查 `-h` 的真实输出，而不是查结构：
        // 取值名和它的说明都必须真的出现在用户看得到的地方
        let help = short_help();

        for name in EXPECTED {
            assert!(help.contains(name), "-h 里缺少取值 {name}:\n{help}");
        }
        for variant in SortTypeArg::value_variants() {
            let value = variant.to_possible_value().expect("每个变体都有取值");
            let description = value
                .get_help()
                .map(|text| text.to_string())
                .unwrap_or_default();
            assert!(!description.is_empty(), "{} 缺少说明", value.get_name());
            assert!(
                help.contains(&description),
                "-h 里缺少 {} 的说明 `{description}`:\n{help}",
                value.get_name()
            );
        }
    }

    #[test]
    fn long_help_also_introduces_every_sort_type() {
        let mut command = CliArgs::command();
        let mut buffer = Vec::new();
        command
            .write_long_help(&mut buffer)
            .expect("渲染 --help 失败");
        let help = String::from_utf8(buffer).expect("--help 输出应当是 UTF-8");

        for name in EXPECTED {
            assert!(help.contains(name), "--help 里缺少取值 {name}:\n{help}");
        }
        // 说明只该出现一次：如果 clap 自带的 Possible values 没被隐藏，
        // --help 里就会重复列一遍
        let needle = "用文件的修改时间代替拍摄时间";
        let first = help.find(needle).expect("缺少说明");
        // 用针长切片：中文是多字节，first + 1 会落在字符中间导致 panic
        let second = help[first + needle.len()..].find(needle);
        assert!(second.is_none(), "--help 里说明出现了两遍:\n{help}");
    }

    #[test]
    fn every_sort_type_maps_onto_a_core_variant() {
        // 每个候选值都必须能解析并转成 core 的枚举，不能有解析不了或漏映射的
        for name in EXPECTED {
            let parsed =
                SortTypeArg::from_str(name, false).unwrap_or_else(|_| panic!("{name} 解析失败"));
            let _: NoDateSortType = parsed.into();
        }
    }

    #[test]
    fn unknown_sort_type_is_rejected_with_the_candidate_list() {
        // 走真正的命令行解析路径：给用户看的候选值清单是 clap 在解析参数时拼的，
        // `ValueEnum::from_str` 自己的报错里并没有
        let err = CliArgs::try_parse_from([
            "photo-sorter-cli",
            "-i",
            "D:\\in",
            "--sort-type",
            "by-nothing",
        ])
        .unwrap_err()
        .to_string();

        // 即使帮助里隐藏了 Possible values，报错也仍然要把可选值列出来
        assert!(err.contains("by-modification-date"), "{err}");
    }

    #[test]
    fn filename_date_is_read_unless_the_flag_turns_it_off() {
        let parse = |extra: &[&str]| {
            let mut args = vec!["photo-sorter-cli", "-i", "D:\\in"];
            args.extend_from_slice(extra);
            let config: SortConfig = CliArgs::try_parse_from(args).unwrap().into();
            config.parse_date_from_filename
        };

        assert!(parse(&[]), "默认应当从文件名里读日期");
        assert!(!parse(&["--no-filename-date"]));
    }

    #[test]
    fn the_removed_from_filename_value_is_rejected() {
        // 旧写法必须报错、而不是被静默忽略：
        // 静默忽略的话，用户会以为「从文件名识别日期」还开着
        let err = CliArgs::try_parse_from([
            "photo-sorter-cli",
            "-i",
            "D:\\in",
            "--sort-type",
            "from-filename",
        ])
        .unwrap_err();

        // 报错本身只会说「取值不对」，替代写法在 -h 的说明表里：
        // 用户两边都能看到，才算真的被指了条路
        let text = err.to_string();
        assert!(text.contains("from-filename"), "{text}");
        assert!(
            short_help().contains("--no-filename-date"),
            "帮助里应当指出替代写法：\n{}",
            short_help()
        );
    }
}
