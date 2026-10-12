use std::fmt;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum NoDateSortType {
    // 按修改日期分类
    ByModificationDate,
    // 放入单独文件夹
    BySingleFolder,
    // 不移动
    DoNotMove,
}

impl Default for NoDateSortType {
    fn default() -> Self {
        Self::BySingleFolder
    }
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WorkMode {
    // 将目标目录及其子目录所有照片提取出来并删除空目录
    Extract,
    // 对照片进行分类
    Sort,
}

impl Default for WorkMode {
    fn default() -> Self {
        Self::Sort
    }
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

#[derive(Default, Debug, Clone)]
pub struct SortConfig {
    pub work_mode: WorkMode,
    // 没有拍摄日期的照片分类标准
    pub no_date_sort_type: NoDateSortType,
    pub input_dir: String,
    pub output_dir: String,
    // 照片数量阈值 某一天的照片数量超过阈值时，将该天的照片分类单独放入一个文件夹
    pub photo_count_threshold: u32,
}

/// 配置的单行摘要，供日志使用。
///
/// 日志每条只占一行才好读，所以这里刻意不放换行符；
/// 界面上需要多行展示时由各前端自己排版（见 `photo-sorter-gui` 的 `ConfigForm::summarize`）。
impl fmt::Display for SortConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "工作模式: {}, 无拍摄日期照片: {}, 输入目录: {}, 输出目录: {}, 照片数量阈值: {}",
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
            input_dir: "D:\\in".to_owned(),
            output_dir: "D:\\out".to_owned(),
            photo_count_threshold: 7,
        };

        let text = config.to_string();
        assert!(!text.contains('\n'), "{text}");
        assert!(text.contains("工作模式: 提取"), "{text}");
        assert!(text.contains("无拍摄日期照片: 不移动"), "{text}");
        assert!(text.contains("输入目录: D:\\in"), "{text}");
        assert!(text.contains("输出目录: D:\\out"), "{text}");
        assert!(text.contains("照片数量阈值: 7"), "{text}");
    }
}
