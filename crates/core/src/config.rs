use std::fmt;

#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub enum NoDateSortType {
    // 按修改日期分类
    ByModificationDate,
    // 放入单独文件夹
    #[default]
    BySingleFolder,
    // 不移动
    DoNotMove,
}

impl fmt::Display for NoDateSortType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            NoDateSortType::ByModificationDate => "按修改日期分类",
            NoDateSortType::BySingleFolder => "放入单独文件夹",
            NoDateSortType::DoNotMove => "不移动",
        };
        f.write_str(text)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub enum WorkMode {
    // 将目标目录及其子目录所有照片提取出来并删除空目录
    Extract,
    // 对照片进行分类
    #[default]
    Sort,
}

impl fmt::Display for WorkMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            WorkMode::Extract => "提取",
            WorkMode::Sort => "分类",
        };
        f.write_str(text)
    }
}

/// 一次整理的完整配置。
///
/// 刻意手写 [`Default`] 而不是派生：`parse_date_from_filename` 的**产品默认值是开启**，
/// 派生出来的 `false` 会让「默认配置」和「界面上默认勾选的东西」对不上——
/// 这种偏差最难查，因为代码读起来完全正常。
#[derive(Debug, Clone)]
pub struct SortConfig {
    pub work_mode: WorkMode,
    // 没有拍摄日期的照片分类标准
    pub no_date_sort_type: NoDateSortType,
    /// 读不到拍摄日期时，是否再从**文件名**里的时间戳推断。
    ///
    /// 默认开启：截图类工具（Steam、部分手机）不写 EXIF，却会把时间戳塞进文件名
    /// （`1629520_20260911110905_1.png`），这类照片光看 EXIF 只能进「无拍摄日期」，
    /// 而文件名里的时间往往正是它们的拍摄时间。
    ///
    /// 它是「拍摄日期的来源之一」，不是「没有日期时的兜底」，所以优先级排在
    /// EXIF 之后、[`Self::no_date_sort_type`] 之前。
    pub parse_date_from_filename: bool,
    pub input_dir: String,
    pub output_dir: String,
    // 照片数量阈值 某一天的照片数量超过阈值时，将该天的照片分类单独放入一个文件夹
    pub photo_count_threshold: u32,
}

impl Default for SortConfig {
    fn default() -> Self {
        Self {
            work_mode: WorkMode::default(),
            no_date_sort_type: NoDateSortType::default(),
            parse_date_from_filename: true,
            input_dir: String::new(),
            output_dir: String::new(),
            photo_count_threshold: 20,
        }
    }
}

/// 配置的单行摘要，供日志使用。
///
/// 日志每条只占一行才好读，所以这里刻意不放换行符；
/// 界面上需要多行展示时由各前端自己排版（见 `photo-sorter-gui` 的 `ConfigForm::summarize`）。
impl fmt::Display for SortConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 「从文件名读日期」默认开着，只有被关掉时才值得写进摘要：
        // 一行摘要里塞满「是/否」会稀释真正在变的那些字段
        let filename = if self.parse_date_from_filename {
            ""
        } else {
            ", 从文件名读日期: 关"
        };
        write!(
            f,
            "工作模式: {}, 无拍摄日期照片: {}{filename}, 输入目录: {}, 输出目录: {}, 照片数量阈值: {}",
            self.work_mode,
            self.no_date_sort_type,
            self.input_dir,
            self.output_dir,
            self.photo_count_threshold,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_single_line_and_keeps_all_fields() {
        let config = SortConfig {
            work_mode: WorkMode::Extract,
            no_date_sort_type: NoDateSortType::DoNotMove,
            parse_date_from_filename: false,
            input_dir: "D:\\in".to_owned(),
            output_dir: "D:\\out".to_owned(),
            photo_count_threshold: 7,
        };

        let text = config.to_string();
        assert!(!text.contains('\n'), "{text}");
        assert!(text.contains("工作模式: 提取"), "{text}");
        assert!(text.contains("无拍摄日期照片: 不移动"), "{text}");
        assert!(text.contains("从文件名读日期: 关"), "{text}");
        assert!(text.contains("输入目录: D:\\in"), "{text}");
        assert!(text.contains("输出目录: D:\\out"), "{text}");
        assert!(text.contains("照片数量阈值: 7"), "{text}");
    }

    #[test]
    fn filename_date_is_on_by_default_and_stays_out_of_the_summary() {
        let config = SortConfig::default();
        assert!(config.parse_date_from_filename, "默认应当开启");

        // 默认开启属于常态，不必在每行日志里重复声明
        assert!(!config.to_string().contains("从文件名读日期"), "{config}");
    }
}
