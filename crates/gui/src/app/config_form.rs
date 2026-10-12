//! 配置表单的**控制层**。
//!
//! 本模块只描述「界面正在编辑的数据」以及由它派生的校验/汇总逻辑，
//! 不引用 [`eframe`] / [`egui`]，因此可以脱离界面单独测试，
//! 也可以被命令行或其他前端复用。

use photo_sorter_core::config::{NoDateSortType, SortConfig, WorkMode};

/// 界面表单所对应的编辑态配置。
///
/// 控件直接绑定到这些字段；点击「读取配置」时再通过
/// [`ConfigForm::read_config`] 做校验与补全，得到可用的 [`SortConfig`]。
#[derive(Debug, Clone)]
pub struct ConfigForm {
    pub work_mode: WorkMode,
    /// 没有拍摄日期的照片分类标准
    pub no_date_sort_type: NoDateSortType,
    pub input_dir: String,
    pub output_dir: String,
    /// 照片数量阈值：某一天的照片数量超过阈值时，单独放入一个文件夹
    pub photo_count_threshold: u32,
}

impl Default for ConfigForm {
    fn default() -> Self {
        Self {
            work_mode: WorkMode::default(),
            no_date_sort_type: NoDateSortType::default(),
            input_dir: String::new(),
            output_dir: String::new(),
            photo_count_threshold: 20,
        }
    }
}

impl ConfigForm {
    /// 将表单中的内容整理成最终的 [`SortConfig`]。
    ///
    /// 输出目录留空时默认与输入目录相同，与 CLI 的行为保持一致。
    /// 若表单内容不合法，返回面向用户的错误信息。
    pub fn read_config(&self) -> Result<SortConfig, String> {
        let input_dir = self.input_dir.trim();
        if input_dir.is_empty() {
            return Err("输入目录不能为空".to_owned());
        }

        let output_dir = match self.output_dir.trim() {
            "" => input_dir,
            trimmed => trimmed,
        };

        Ok(SortConfig {
            work_mode: self.work_mode.clone(),
            no_date_sort_type: self.no_date_sort_type.clone(),
            input_dir: input_dir.to_owned(),
            output_dir: output_dir.to_owned(),
            photo_count_threshold: self.photo_count_threshold,
        })
    }

    /// 生成配置的摘要文本，用于在界面上确认读取结果。
    ///
    /// 打印的是校验、补全之后的最终配置，而不是表单里的原始输入。
    pub fn summarize(config: &SortConfig) -> String {
        format!(
            "工作模式: {}\n无拍摄日期照片: {}\n输入目录: {}\n输出目录: {}\n照片数量阈值: {}",
            config.work_mode,
            config.no_date_sort_type,
            config.input_dir,
            config.output_dir,
            config.photo_count_threshold,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(input_dir: &str, output_dir: &str) -> ConfigForm {
        ConfigForm {
            input_dir: input_dir.to_owned(),
            output_dir: output_dir.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn default_threshold_is_20() {
        assert_eq!(ConfigForm::default().photo_count_threshold, 20);
    }

    #[test]
    fn empty_input_dir_is_rejected() {
        let err = form("   ", "").read_config().unwrap_err();
        assert_eq!(err, "输入目录不能为空");
    }

    #[test]
    fn blank_output_dir_falls_back_to_input_dir() {
        let config = form("  D:\\photos  ", "   ").read_config().unwrap();
        assert_eq!(config.input_dir, "D:\\photos");
        assert_eq!(config.output_dir, "D:\\photos");
    }

    #[test]
    fn explicit_output_dir_wins_and_is_trimmed() {
        let config = form("D:\\in", "  D:\\out  ").read_config().unwrap();
        assert_eq!(config.input_dir, "D:\\in");
        assert_eq!(config.output_dir, "D:\\out");
    }

    #[test]
    fn other_fields_are_carried_over() {
        let mut form = form("D:\\in", "");
        form.work_mode = WorkMode::Extract;
        form.no_date_sort_type = NoDateSortType::DoNotMove;
        form.photo_count_threshold = 7;

        let config = form.read_config().unwrap();
        assert_eq!(config.work_mode, WorkMode::Extract);
        assert_eq!(config.no_date_sort_type, NoDateSortType::DoNotMove);
        assert_eq!(config.photo_count_threshold, 7);
    }

    #[test]
    fn summarize_reports_the_final_config() {
        // 输出目录留空，摘要里应当已经是补全后的输入目录
        let summary = ConfigForm::summarize(&form("D:\\in", "").read_config().unwrap());
        assert!(summary.contains("输入目录: D:\\in"), "{summary}");
        assert!(summary.contains("输出目录: D:\\in"), "{summary}");
        assert!(summary.contains("照片数量阈值: 20"), "{summary}");
    }
}
