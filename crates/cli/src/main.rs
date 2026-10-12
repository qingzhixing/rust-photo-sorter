//! 命令行前端：把 Core 的执行过程显示成人能读的东西。
//!
//! 试运行（`--dry-run`）的价值全在「看得到打算怎么搬」，所以 [`PlanPrinter`]
//! 会把每条「源 → 目标」都打到 **stdout**：
//! - 想直接看 → 就在终端里；
//! - 想仔细核对或存档 → `photo-sorter-cli ... --dry-run > 计划.txt`。
//!
//! 明细只走 stdout、不写日志（`RUST_LOG` 只管日志），免得同一份内容出现两遍。

use std::cell::RefCell;

use clap::Parser;
use log::info;
use photo_sorter_core::{
    config::SortConfig,
    info::print_project_info,
    planner::Skipped,
    worker::{self, ExecuteMode, Observer, PhotoOutcome},
};

mod cli_args;
mod logging;

/// 试运行时最多列出多少条明细。
///
/// 几万张照片的清单既没人看得完，也会把终端刷爆；超出的部分只报数量。
const MAX_PLAN_LINES: usize = 500;

fn main() {
    // 放在最前面：越早装好 logger，越不容易漏掉启动阶段的日志
    logging::init();

    let args = cli_args::CliArgs::parse();

    // 处理 version 选项
    if args.version {
        // print_ascii_art();
        print_project_info();
        return;
    }

    // 先取出执行方式，后面 args 会被消耗掉转成配置
    let mode = if args.dry_run {
        ExecuteMode::DryRun
    } else if args.copy {
        ExecuteMode::Copy
    } else {
        ExecuteMode::Move
    };

    let config: SortConfig = args.into();
    info!("读取到配置: {config}");
    info!("执行方式: {mode}");

    // 只有试运行需要逐条记录：真搬运时细节都在日志里，
    // stdout 应当留给结论
    let printer = PlanPrinter::default();
    let observer = if mode.is_dry_run() {
        // 借用一直持续到 run 返回；试运行途中没有别的地方碰 printer
        Some(&printer as &dyn Observer)
    } else {
        None
    };

    let report = match worker::run(&config, mode, observer) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("错误：{err}");
            std::process::exit(1);
        }
    };

    // 观察者的借用到这里已经结束，可以把攒下的内容取出来打印
    if mode.is_dry_run() {
        print_plan(&printer, report.planned, report.skipped);
    }
    // 结论是给用户看的正式输出，走 stdout；细节在日志里
    println!("{}", report.summary());
    for problem in &report.problems {
        eprintln!("⚠ {problem}");
    }
    if report.failed > 0 {
        std::process::exit(1);
    }
}

/// 试运行结束后打印完整计划：逐条明细 + 没动的照片。
///
/// 打印的是**这次实际算出来的计划**，所以这里的条数和下面的结论对得上。
fn print_plan(printer: &PlanPrinter, planned: usize, skipped: usize) {
    let moves = printer.moves();
    let skipped_photos = printer.skipped();

    if moves.is_empty() && skipped == 0 {
        return;
    }

    if !moves.is_empty() {
        println!("计划明细（{planned} 条）:");
        for line in &moves {
            println!("  {line}");
        }
        if printer.omitted() > 0 {
            println!(
                "  … 其余 {} 条未列出（一次最多列 {MAX_PLAN_LINES} 条）",
                printer.omitted()
            );
        }
    }

    // 「跳过 74 张」不写清楚是哪 74 张、为什么，用户没法判断这是不是自己想要的
    if skipped > 0 {
        println!("不动（{skipped} 张）:");
        for item in &skipped_photos {
            println!("  {}  ←  {}", item.source.display(), item.reason);
        }
        if skipped > skipped_photos.len() {
            println!(
                "  … 其余 {} 张未列出（一次最多列 {MAX_PLAN_LINES} 张）",
                skipped - skipped_photos.len()
            );
        }
    }
    println!();
}

/// 试运行时收集「谁搬到哪」，收尾时一并打印。
///
/// 之所以先收集、最后打印，而不是边跑边打：明细和结论放在一起才像一份报告；
/// 先打明细的话，用户翻到结论前会以为「就这些」。
///
/// 状态放在 [`RefCell`] 里是为了配合 [`Observer`] 的 `&self` 回调；
/// 回调是顺序调用的，不会真的撞上借用冲突。
#[derive(Default)]
struct PlanPrinter {
    /// 已经排好版的明细行
    moves: RefCell<Vec<String>>,
    /// 因为超过 [`MAX_PLAN_LINES`] 而没有列出的条数
    omitted: RefCell<usize>,
    /// 没动的照片：一张都不落地，也得让用户看见它们为什么没动
    skipped: RefCell<Vec<Skipped>>,
}

impl PlanPrinter {
    /// 明细行（借用结束后取用）。
    fn moves(&self) -> Vec<String> {
        self.moves.borrow().clone()
    }

    /// 被省略的条数。
    fn omitted(&self) -> usize {
        *self.omitted.borrow()
    }

    /// 没动的照片。
    fn skipped(&self) -> Vec<Skipped> {
        self.skipped.borrow().clone()
    }
}

impl Observer for PlanPrinter {
    fn on_skipped(&self, skipped: &Skipped) {
        // 明细和跳过各自封顶：两边都用同一个上限，
        // 「列了多少、还剩多少」才好对得上
        let mut collected = self.skipped.borrow_mut();
        if collected.len() < MAX_PLAN_LINES {
            collected.push(skipped.clone());
        }
    }

    fn on_photo(&self, outcome: &PhotoOutcome) {
        let PhotoOutcome::Done {
            source,
            target,
            folder,
        } = outcome
        else {
            // 试运行不会失败；真失败会进 problems，不占明细的位置
            return;
        };

        let mut moves = self.moves.borrow_mut();
        if moves.len() >= MAX_PLAN_LINES {
            *self.omitted.borrow_mut() += 1;
            return;
        }
        moves.push(format!(
            "{}  →  {}（{folder}）",
            source.display(),
            target.display()
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use photo_sorter_core::planner::{FolderKind, SkipReason};

    use super::*;

    #[test]
    fn every_move_is_listed_with_both_ends_and_the_folder_kind() {
        let printer = PlanPrinter::default();
        printer.on_photo(&PhotoOutcome::Done {
            source: PathBuf::from("D:\\照片\\a.jpg"),
            target: PathBuf::from("D:\\已分类\\无拍摄日期\\a.jpg"),
            folder: FolderKind::NoDate,
        });
        printer.on_skipped(&Skipped {
            source: PathBuf::from("D:\\照片\\b.jpg"),
            reason: SkipReason::AlreadyInPlace,
        });

        let moves = printer.moves();
        assert_eq!(moves.len(), 1);
        let line = &moves[0];
        // 两头都要有：光说「搬了 a.jpg」等于没说搬去哪
        assert!(line.contains("D:\\照片\\a.jpg"), "{line}");
        assert!(line.contains("D:\\已分类"), "{line}");
        assert!(
            line.contains("无日期目录"),
            "目标属于哪一类也要看得见：{line}"
        );

        let skipped = printer.skipped();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].reason, SkipReason::AlreadyInPlace);
    }

    #[test]
    fn the_list_is_capped_but_the_count_is_not_lost() {
        let printer = PlanPrinter::default();
        for i in 0..MAX_PLAN_LINES + 7 {
            printer.on_photo(&PhotoOutcome::Done {
                source: PathBuf::from(format!("in\\{i}.jpg")),
                target: PathBuf::from(format!("out\\{i}.jpg")),
                folder: FolderKind::Week,
            });
        }

        assert_eq!(printer.moves().len(), MAX_PLAN_LINES);
        // 少列了不等于没发生：被省略的条数必须留着，好让用户知道清单不全
        assert_eq!(printer.omitted(), 7);
    }
}
