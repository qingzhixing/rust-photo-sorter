//! 配置窗口的**控制层**。
//!
//! [`PhotoSorterApp`] 持有配置表单、执行状态与结果，负责响应视图上报的
//! [`ViewAction`]，并把要展示的内容整理好交给视图渲染。
//! 除实现 [`eframe::App`] 所需的入口外，这里不做任何界面布局。
//!
//! 整理本身跑在后台线程上（见 [`job`]），所以这里只做三件事：
//! 校验配置、启动任务、每帧收取进度。

pub(crate) mod config_form;
mod file_picker;
mod job;
pub(crate) mod view;

use eframe::egui;
use photo_sorter_core::worker::ExecuteMode;

use self::config_form::ConfigForm;
use self::job::{JobRunner, JobState};
use self::view::{ReadMessage, ViewAction};

/// 照片分类的配置界面。
///
/// 各字段的「初始状态」都由它们自己的 [`Default`] 决定：
/// 配置项用 core 的默认值，执行方式默认「移动」（和 CLI 一致），
/// 没有正在跑的任务、也没有已读出的配置。
#[derive(Default)]
pub struct PhotoSorterApp {
    /// 正在编辑中的配置，表单控件直接绑定到这些字段
    form: ConfigForm,
    /// 点击「读取配置」后校验/汇总的结果
    message: Option<ReadMessage>,
    /// 执行方式：试运行 / 移动 / 复制
    mode: ExecuteMode,
    /// 界面上的执行状态，由后台线程的进度驱动
    state: JobState,
    /// 后台执行器
    jobs: JobRunner,
}

impl PhotoSorterApp {
    /// 读取当前表单，返回校验、补全之后的最终配置。
    ///
    /// 校验结果会写进日志，方便排查「界面上填的」和「实际拿去分类的」不一致的问题。
    fn read_config(&self) -> Result<photo_sorter_core::config::SortConfig, String> {
        match self.form.read_config() {
            Ok(config) => {
                log::info!("读取到配置: {config}");
                Ok(config)
            }
            Err(err) => {
                log::warn!("读取配置失败: {err}");
                Err(err)
            }
        }
    }

    /// 处理「读取配置」：把最终配置的摘要（或错误）记下来给界面显示。
    fn read_config_message(&mut self) {
        self.message = Some(match self.read_config() {
            Ok(config) => ReadMessage::Summary(ConfigForm::summarize(&config)),
            Err(err) => ReadMessage::Error(err),
        });
    }

    /// 处理「执行」：校验配置后把整理交给后台线程。
    ///
    /// 配置不合法时只报错、不起线程——让用户对着一个「跑到一半才发现目录填错了」的
    /// 进度条等待毫无意义。
    fn begin_execute(&mut self, ctx: &egui::Context) {
        let config = match self.read_config() {
            Ok(config) => config,
            Err(err) => {
                self.message = Some(ReadMessage::Error(err));
                return;
            }
        };

        log::info!("执行方式: {}", self.mode);
        // 结果区马上会显示进度，配置摘要再留着只会分散注意力
        self.message = None;
        self.state = JobState::Running {
            progress: Default::default(),
            started_at: std::time::Instant::now(),
        };
        self.jobs.start(config, self.mode, ctx);
    }

    /// 处理视图上报的操作。
    fn handle_actions(&mut self, actions: &[ViewAction], ctx: &egui::Context) {
        for action in actions {
            match action {
                ViewAction::ReadConfig => self.read_config_message(),
                ViewAction::Execute => self.begin_execute(ctx),
                ViewAction::PickInputDir => {
                    if let Some(dir) = file_picker::pick_folder(&self.form.input_dir) {
                        self.form.input_dir = dir;
                    }
                }
                ViewAction::PickOutputDir => {
                    if let Some(dir) = file_picker::pick_folder(&self.form.output_dir) {
                        self.form.output_dir = dir;
                    }
                }
            }
        }
    }
}

impl eframe::App for PhotoSorterApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 先收进度再画界面：这一帧显示的就是最新的数字，不会永远慢一拍
        self.jobs.poll(&mut self.state);

        let ctx = ui.ctx().clone();
        let mut actions = Vec::new();
        view::show(
            ui,
            &mut self.form,
            &mut self.mode,
            self.message.as_ref(),
            &self.state,
            &mut actions,
        );

        self.handle_actions(&actions, &ctx);
    }
}

/// 注册窗口与字体，返回 eframe 需要的应用构造函数。
pub fn build_creator() -> eframe::AppCreator<'static> {
    Box::new(|cc| {
        install_fonts(&cc.egui_ctx);
        Ok(Box::new(PhotoSorterApp::default()))
    })
}

/// 加载 HarmonyOS 字体并设为默认字体。
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 加载 HarmonyOS 字体
    let font_data = egui::FontData::from_static(include_bytes!(
        "../../../assets/font/HarmonyOS_Sans_SC_Regular.ttf"
    ));

    fonts
        .font_data
        .insert("HarmonyOS_Sans".to_owned(), std::sync::Arc::new(font_data));

    // 将 HarmonyOS 字体添加到字体家族的最前面
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "HarmonyOS_Sans".to_owned());
    }

    // 将配置应用到上下文
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::job::ResultMessage;
    use super::*;
    use crate::test_support::{self, TempDir};
    use photo_sorter_core::config::WorkMode;
    use std::time::Duration;

    /// 一个不会真的被执行的目录名，用来构造「跑不起来」的场景。
    fn missing_dir() -> String {
        std::env::temp_dir()
            .join("photo-sorter-gui-app-missing")
            .display()
            .to_string()
    }

    fn run_actions(app: &mut PhotoSorterApp, actions: &[ViewAction]) {
        app.handle_actions(actions, &egui::Context::default());
    }

    /// 等后台执行结束；超时就直接判失败，免得测试挂死。
    fn wait_for_finish(app: &mut PhotoSorterApp, timeout: Duration) -> JobState {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline && app.state.is_running() {
            app.jobs.poll(&mut app.state);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.state.is_running(), "等待执行结束超时");
        app.state.clone()
    }

    #[test]
    fn read_config_reports_error_for_empty_input_dir() {
        let mut app = PhotoSorterApp::default();
        run_actions(&mut app, &[ViewAction::ReadConfig]);

        assert_eq!(
            app.message,
            Some(ReadMessage::Error("输入目录不能为空".to_owned()))
        );
    }

    #[test]
    fn read_config_reports_summary_for_valid_form() {
        let mut app = PhotoSorterApp {
            form: ConfigForm {
                input_dir: "D:\\photos".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        };
        run_actions(&mut app, &[ViewAction::ReadConfig]);

        match &app.message {
            Some(ReadMessage::Summary(summary)) => {
                assert!(summary.contains("输入目录: D:\\photos"), "{summary}");
                assert!(summary.contains("输出目录: D:\\photos"), "{summary}");
            }
            other => panic!("期望得到成功摘要，实际为 {other:?}"),
        }
    }

    #[test]
    fn execute_refuses_to_start_with_an_invalid_form() {
        let mut app = PhotoSorterApp::default();
        run_actions(&mut app, &[ViewAction::Execute]);

        // 配置不合法时连线程都不该起，直接把错误摆到界面上
        assert!(!app.state.is_running());
        assert_eq!(
            app.message,
            Some(ReadMessage::Error("输入目录不能为空".to_owned()))
        );
    }

    #[test]
    fn execute_starts_a_job_without_blocking_the_ui_thread() {
        let mut app = PhotoSorterApp {
            form: ConfigForm {
                work_mode: WorkMode::Sort,
                input_dir: missing_dir(),
                ..Default::default()
            },
            mode: ExecuteMode::DryRun,
            ..Default::default()
        };

        run_actions(&mut app, &[ViewAction::Execute]);

        // 校验通过就该立刻进入执行状态；目录不存在要等后台线程报回来
        assert!(app.state.is_running());
        assert!(app.message.is_none(), "开始执行后不该再留着配置摘要");

        match wait_for_finish(&mut app, Duration::from_secs(10)) {
            JobState::Finished(ResultMessage::Error(err)) => {
                assert!(err.contains("输入目录"), "{err}")
            }
            other => panic!("期望得到启动失败的结论，实际为 {other:?}"),
        }
    }

    #[test]
    fn execute_uses_the_mode_picked_in_the_ui() {
        let mut app = PhotoSorterApp {
            form: ConfigForm {
                input_dir: missing_dir(),
                ..Default::default()
            },
            mode: ExecuteMode::Copy,
            ..Default::default()
        };

        run_actions(&mut app, &[ViewAction::Execute]);

        // 执行方式是从界面上取的那一个，而不是写死的默认值
        assert_eq!(app.mode, ExecuteMode::Copy);
        assert!(app.state.is_running());
    }

    #[test]
    fn execute_moves_photos_end_to_end_from_the_gui_path() {
        let tmp = TempDir::new("gui-execute");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        // 两张没有 EXIF 的照片，会一起进「无拍摄日期」目录
        test_support::write(&input.join("trip").join("a.jpg"), "x");
        test_support::write(&input.join("b.jpg"), "y");

        let mut app = PhotoSorterApp {
            form: ConfigForm {
                input_dir: input.display().to_string(),
                output_dir: output.display().to_string(),
                ..Default::default()
            },
            mode: ExecuteMode::Move,
            ..Default::default()
        };

        run_actions(&mut app, &[ViewAction::Execute]);

        match wait_for_finish(&mut app, Duration::from_secs(30)) {
            JobState::Finished(ResultMessage::Report(report)) => {
                assert_eq!(report.succeeded, 2, "{:?}", report.problems);
                assert_eq!(report.failed, 0, "{:?}", report.problems);
                assert_eq!(report.mode, ExecuteMode::Move);
            }
            other => panic!("期望得到执行报告，实际为 {other:?}"),
        }

        // 真搬了：文件出现在输出目录，源文件不在了
        let no_date = output.join("无拍摄日期");
        assert!(no_date.join("a.jpg").exists());
        assert!(no_date.join("b.jpg").exists());
        assert!(!input.join("b.jpg").exists(), "移动模式不该留下源文件");
    }

    #[test]
    fn dry_run_from_the_gui_leaves_every_file_alone() {
        let tmp = TempDir::new("gui-dry-run");
        let input = tmp.path().join("in");
        let output = tmp.path().join("out");
        test_support::write(&input.join("a.jpg"), "x");

        let mut app = PhotoSorterApp {
            form: ConfigForm {
                input_dir: input.display().to_string(),
                output_dir: output.display().to_string(),
                ..Default::default()
            },
            mode: ExecuteMode::DryRun,
            ..Default::default()
        };

        run_actions(&mut app, &[ViewAction::Execute]);

        match wait_for_finish(&mut app, Duration::from_secs(30)) {
            JobState::Finished(ResultMessage::Report(report)) => {
                // 试运行也报「待处理 1 张」，但界面上会额外标出这是试运行
                assert_eq!(report.succeeded, 1);
                assert!(report.mode.is_dry_run());
            }
            other => panic!("期望得到执行报告，实际为 {other:?}"),
        }

        assert!(input.join("a.jpg").exists(), "试运行不该动源文件");
        assert!(!output.exists(), "试运行不该创建输出目录");
    }
}
