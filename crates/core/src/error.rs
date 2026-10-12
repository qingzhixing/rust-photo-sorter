//! Core 的错误类型。
//!
//! 只有「整个任务没法开始」的问题才作为 [`SortError`] 返回；
//! 单个文件的失败会收集进 [`crate::worker::Report`]，
//! 不让一张坏照片毁掉整次整理。

use std::path::PathBuf;

use thiserror::Error;

/// 整理任务无法继续时返回的错误。
#[derive(Debug, Error)]
pub enum SortError {
    /// 输入目录不存在（或没有权限访问）
    #[error("输入目录不存在或无法访问: {0}")]
    InputDirMissing(PathBuf),

    /// 输入目录存在，但不是目录
    #[error("输入目录不是目录: {0}")]
    InputDirNotADirectory(PathBuf),

    /// 输出目录存在，但不是目录
    #[error("输出目录不是目录: {0}")]
    OutputDirNotADirectory(PathBuf),
}
