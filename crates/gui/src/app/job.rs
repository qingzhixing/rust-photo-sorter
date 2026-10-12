//! **执行**：把整理任务放到后台线程里跑，并把进度送回界面线程。
//!
//! 界面必须一直能响应，所以 `worker::run` 绝不能直接在 UI 线程上跑：
//! 一次整理可能扫几万张照片、跨盘复制几个 G，卡住的窗口在用户眼里和死机没区别。
//!
//! 于是这里做了转发：工作线程持有 [`SortConfig`] 和 [`ExecuteMode`]，
//! 通过 [`worker::Observer`] 把进度写进通道；界面线程每帧 [`JobRunner::poll`]
//! 一次，把收到的消息收敛成一个 [`JobState`]。
//!
//! 本模块不认识 [`eframe`](eframe) / egui，只依赖一个 [`egui::Context`] 用来
//! 唤醒界面（见 [`JobRunner::start`]）。

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use eframe::egui;
use photo_sorter_core::config::SortConfig;
use photo_sorter_core::worker::{self, ExecuteMode, Observer, PhotoOutcome, Report};

/// 整理过程中的进度快照。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct JobProgress {
    /// 扫描到的照片数
    pub scanned: usize,
    /// 计划搬运的数量
    pub planned: usize,
    /// 明确跳过的数量
    pub skipped: usize,
    /// 已成功的数量
    pub succeeded: usize,
    /// 已失败的数量
    pub failed: usize,
    /// 最近一张处理过的照片（试运行时是「打算搬运」的那张）
    pub current: Option<PathBuf>,
}

impl JobProgress {
    /// 已经处理完的数量（成功 + 失败）。
    pub fn done(&self) -> usize {
        self.succeeded + self.failed
    }

    /// 当前目录里还看得到的、要搬运的照片数。
    pub fn total(&self) -> usize {
        self.planned.max(self.done())
    }

    /// 进度比例，取值 0.0..=1.0；无事可做时按「已完成」处理。
    pub fn ratio(&self) -> f32 {
        let total = self.total();
        if total == 0 {
            1.0
        } else {
            self.done() as f32 / total as f32
        }
    }
}

/// 界面当前该展示的执行状态。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum JobState {
    /// 没在跑，也没跑过
    #[default]
    Idle,
    /// 正在跑
    Running {
        /// 进度快照
        progress: JobProgress,
        /// 本次执行的开始时刻，用来显示已耗时
        started_at: Instant,
    },
    /// 跑完了（[`ResultMessage::Error`] 只代表「没能跑起来」）
    Finished(ResultMessage),
}

impl JobState {
    /// 是否正在执行。
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }
}

/// 跑完之后界面上要展示的内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultMessage {
    /// 正常结束，附上统计报告
    Report(Report),
    /// 压根没能跑起来（目录不存在、输入目录填成了文件…）
    Error(String),
}

/// 工作线程发给界面线程的消息。
#[derive(Debug)]
enum Message {
    /// 开始落地，附上扫描/计划/跳过的数量
    Started {
        scanned: usize,
        planned: usize,
        skipped: usize,
    },
    /// 一张照片处理完了
    Photo(PhotoOutcome),
    /// 整次整理结束
    Done(Result<Report, String>),
}

/// 执行任务的句柄：持有工作线程与它的输出通道。
struct JobHandle {
    /// 工作线程；退出时随通道一起关闭，所以只需要留着。
    ///
    /// 这里不去 join：整理可能正搬着几百兆的照片，关窗口时干等它搬完
    /// 反而像卡死；用户点「关闭」就是要立刻停手。
    _thread: JoinHandle<()>,
    /// 工作线程的输出
    rx: Receiver<Message>,
}

/// 后台执行器：一次只跑一个任务，但可以反复跑。
#[derive(Default)]
pub struct JobRunner {
    job: Option<JobHandle>,
}

impl JobRunner {
    /// 启动一次整理，立即返回。
    ///
    /// `ctx` 只在收到消息时被用来唤醒界面：egui 是按需重绘的，
    /// 没人点、没人动的时候不会自己刷帧，不唤醒的话进度条会一直停在原地。
    pub fn start(&mut self, config: SortConfig, mode: ExecuteMode, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();

        // 放进线程里的是配置和执行方式本身（都是 Send + 'static），
        // 表单随后怎么改都不会影响正在跑的这一次
        let thread = thread::spawn(move || {
            let reporter = Reporter { tx, ctx };
            let result = worker::run(&config, mode, Some(&reporter)).map_err(|err| err.to_string());
            // 界面已经关掉时发送必然失败，丢掉即可——没有谁需要这份报告了
            let _ = reporter.tx.send(Message::Done(result));
        });

        self.job = Some(JobHandle {
            _thread: thread,
            rx,
        });
    }

    /// 把工作线程发来的消息收进来，返回状态是否有变化。
    ///
    /// 每帧调用一次；一次收多条消息（例如一帧里处理完了上百张）也只会让
    /// 界面重绘一次，不会因为消息多而反复刷新。
    pub fn poll(&mut self, state: &mut JobState) -> bool {
        let Some(handle) = self.job.as_ref() else {
            return false;
        };

        let mut changed = false;
        loop {
            match handle.rx.try_recv() {
                Ok(Message::Started {
                    scanned,
                    planned,
                    skipped,
                }) => {
                    if let JobState::Running { progress, .. } = state {
                        progress.scanned = scanned;
                        progress.planned = planned;
                        progress.skipped = skipped;
                    }
                    changed = true;
                }
                Ok(Message::Photo(outcome)) => {
                    if let JobState::Running { progress, .. } = state {
                        // 成功与否都算「处理完了」：目标已存在之类的失败也是结论
                        if outcome.is_done() {
                            progress.succeeded += 1;
                        } else {
                            progress.failed += 1;
                        }
                        progress.current = Some(outcome.source().to_path_buf());
                    }
                    changed = true;
                }
                Ok(Message::Done(result)) => {
                    *state = JobState::Finished(match result {
                        Ok(report) => ResultMessage::Report(report),
                        Err(err) => ResultMessage::Error(err),
                    });
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // 通道关了却没送来 Done：工作线程 panic 了。
                    // 此时状态若还停在「执行中」，界面上的按钮会永远转下去，
                    // 得把这次执行收尾成一个明确的失败
                    if state.is_running() {
                        *state = JobState::Finished(ResultMessage::Error(
                            "执行线程意外结束，请查看日志了解详情".to_owned(),
                        ));
                        changed = true;
                    }
                    break;
                }
            }
        }

        changed
    }
}

/// 把 Core 的观察者回调转成通道消息。
struct Reporter {
    tx: mpsc::Sender<Message>,
    /// 消息到达时唤醒界面，免得进度条停摆
    ctx: egui::Context,
}

impl Reporter {
    /// 通知界面重绘。
    ///
    /// 界面已经关掉、或这一帧本来就会重绘时，这只是个便宜的空操作。
    fn notify(&self) {
        self.ctx.request_repaint();
    }
}

impl Observer for Reporter {
    fn on_started(&self, scanned: usize, planned: usize, skipped: usize) {
        if self
            .tx
            .send(Message::Started {
                scanned,
                planned,
                skipped,
            })
            .is_ok()
        {
            self.notify();
        }
    }

    fn on_photo(&self, outcome: &PhotoOutcome) {
        if self.tx.send(Message::Photo(outcome.clone())).is_ok() {
            self.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 等后台执行结束；超时就直接判失败，免得测试挂死。
    fn wait_for_finish(runner: &mut JobRunner, state: &mut JobState, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline && state.is_running() {
            runner.poll(state);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!state.is_running(), "等待执行结束超时");
    }

    #[test]
    fn missing_input_dir_fails_before_any_file_is_touched() {
        let tmp = crate::test_support::TempDir::new("gui-job-missing");
        let config = SortConfig {
            input_dir: tmp.path().join("definitely-not-here").display().to_string(),
            output_dir: tmp.path().display().to_string(),
            ..SortConfig::default()
        };

        let mut runner = JobRunner::default();
        let mut state = JobState::Running {
            progress: JobProgress::default(),
            started_at: Instant::now(),
        };
        runner.start(config, ExecuteMode::Move, &egui::Context::default());
        wait_for_finish(&mut runner, &mut state, Duration::from_secs(10));

        match state {
            JobState::Finished(ResultMessage::Error(err)) => {
                assert!(err.contains("输入目录"), "{err}");
            }
            other => panic!("期望得到启动失败的结论，实际为 {other:?}"),
        }
    }

    #[test]
    fn poll_keeps_a_running_state_untouched_when_there_is_nothing_new() {
        let mut runner = JobRunner::default();
        let mut state = JobState::Running {
            progress: JobProgress::default(),
            started_at: Instant::now(),
        };

        // 没有任务时 poll 不该动状态，也不该报告「有变化」
        assert!(!runner.poll(&mut state));
        assert!(state.is_running());
    }

    #[test]
    fn progress_counts_and_ratio_are_consistent() {
        let progress = JobProgress {
            scanned: 10,
            planned: 4,
            skipped: 6,
            succeeded: 3,
            failed: 1,
            current: Some(PathBuf::from("D:\\照片\\a.jpg")),
        };

        assert_eq!(progress.done(), 4);
        assert_eq!(progress.total(), 4);
        assert_eq!(progress.ratio(), 1.0);

        // 计划为 0（例如目录里啥都没有）时按「已完成」算，进度条不该卡在 0
        assert_eq!(JobProgress::default().ratio(), 1.0);
    }
}
