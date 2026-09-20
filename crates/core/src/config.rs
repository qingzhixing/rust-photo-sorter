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
