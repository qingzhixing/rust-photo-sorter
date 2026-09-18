pub enum NoDateSortType {
    // 按修改日期分类
    ByModificationDate,
    // 放入单独文件夹
    BySingleFolder,
    // 不移动
    DoNotMove,
}

pub struct SortConfig {
    // 没有拍摄日期的照片分类标准
    pub no_date_sort_type: NoDateSortType,
    pub input_dir: String,
    pub output_dir: String,
}
