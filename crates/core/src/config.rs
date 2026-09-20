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

#[derive(Default, Debug)]
pub struct SortConfig {
    // 没有拍摄日期的照片分类标准
    pub no_date_sort_type: NoDateSortType,
    pub input_dir: String,
    pub output_dir: String,
    // 照片数量阈值 某一天的照片数量超过阈值时，将该天的照片分类单独放入一个文件夹
    pub photo_count_threshold: u32,
}
