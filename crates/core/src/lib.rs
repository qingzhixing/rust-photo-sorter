pub mod config;
pub mod error;
pub mod info;
pub mod photo;
pub mod planner;
pub mod worker;

/// 测试专用的小工具，不参与正式构建。
#[cfg(test)]
mod test_support;
