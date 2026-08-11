//! 完成统计计算：周 / 月 / 年 / 项目维度的桶统计与全局汇总。
//!
//! 纯函数模块（无 IO、无 iced 依赖）：输入任务 / 项目列表与固定时间 `now`，
//! 输出各维度统计桶（[`Bucket`]）与汇总（[`Totals`]）。
//!
//! 切桶约定：周期按**本地时区**划分（与"UTC 存储、本地展示"一致），
//! 一律用 `NaiveDate` 日期运算（`Days` / `Months` 递减），**禁止**用 `Duration`
//! 相减或 ISO 周号回推——DST 会偏移 1 小时、ISO 跨年（第 1 周 / 52-53 周 /
//! ISO 年 ≠ 日历年）会错桶；`iso_week()` 仅可用于展示或测试校验。

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Days, Duration, Local, Months, NaiveDate, Utc};
use uuid::Uuid;

use crate::model::{Project, Todo};

/// 一个统计桶：一个周期（周 / 月 / 年）或一个项目的完成统计。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    /// 桶标签：周起始日 `MM-DD` / `M月` / `YYYY` / 项目名
    pub label: String,
    /// 已完成任务数
    pub count: usize,
    /// 总耗时（各已完成任务 `finished_at - started_at` 之和；`started_at` 缺失计 0）
    pub total: Duration,
}

impl Bucket {
    /// 空桶（count = 0、total = 0）。
    fn empty(label: String) -> Self {
        Self {
            label,
            count: 0,
            total: Duration::zero(),
        }
    }
}

/// 全局汇总（不随维度变化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Totals {
    /// 已完成任务总数
    pub done_count: usize,
    /// 总耗时
    pub total: Duration,
    /// 平均耗时（无已完成任务时为 0，除零防护）
    pub avg: Duration,
    /// 最长耗时任务（id + 标题 + 耗时；无任务时 `None`）
    pub longest: Option<(Uuid, String, Duration)>,
}

/// 已完成任务迭代器（`finished_at` 有值）。
pub fn completed(todos: &[Todo]) -> impl Iterator<Item = &Todo> {
    todos.iter().filter(|t| t.finished_at.is_some())
}

/// UTC 时刻对应的本地日期（切桶基准）。
fn local_date(dt: DateTime<Utc>) -> NaiveDate {
    dt.with_timezone(&Local).date_naive()
}

/// 最近 `weeks` 个完整周（含当前周，**周一**为一周开始）的完成统计桶。
///
/// 末桶 = 当前周（view 据此高亮"当前周期"）；早于窗口起点的任务忽略。
pub fn week_buckets(todos: &[Todo], now: DateTime<Utc>, weeks: usize) -> Vec<Bucket> {
    let today = local_date(now);
    // 本周一（ISO 周起点；`num_days_from_monday` 直接给出距周一的天数，纯日期运算无 DST 偏移）
    let monday = today - Days::new(u64::from(today.weekday().num_days_from_monday()));
    let first_start = monday - Days::new(7 * weeks.saturating_sub(1) as u64);
    let mut buckets: Vec<Bucket> = (0..weeks)
        .map(|i| {
            Bucket::empty(
                (first_start + Days::new(7 * i as u64))
                    .format("%m-%d")
                    .to_string(),
            )
        })
        .collect();
    for t in completed(todos) {
        let Some(finish) = t.finished_at else {
            continue;
        };
        let d = local_date(finish);
        if d < first_start {
            continue;
        }
        let idx = (d - first_start).num_days() as usize / 7;
        if let Some(b) = buckets.get_mut(idx) {
            b.count += 1;
            b.total += t.duration(now).unwrap_or_default();
        }
    }
    buckets
}

/// 最近 `months` 个自然月（含当前月）的完成统计桶。
///
/// 末桶 = 当前月；月初归一化（`with_day(1)`）后再递减，2 月 29 日 / 3 月 31 日
/// 等边界不会漂移。
pub fn month_buckets(todos: &[Todo], now: DateTime<Utc>, months: usize) -> Vec<Bucket> {
    let first_of_current = local_date(now).with_day(1).expect("每月都有 1 日");
    let first_start = first_of_current
        .checked_sub_months(Months::new(months.saturating_sub(1) as u32))
        .expect("月份回推在 NaiveDate 范围内");
    let mut buckets: Vec<Bucket> = (0..months)
        .map(|i| {
            let start = first_start
                .checked_add_months(Months::new(i as u32))
                .expect("月份前推在 NaiveDate 范围内");
            Bucket::empty(format!("{}月", start.month()))
        })
        .collect();
    for t in completed(todos) {
        let Some(finish) = t.finished_at else {
            continue;
        };
        let d = local_date(finish);
        if d < first_start {
            continue;
        }
        let idx =
            (d.year() - first_start.year()) * 12 + d.month0() as i32 - first_start.month0() as i32;
        if let Some(b) = buckets.get_mut(idx as usize) {
            b.count += 1;
            b.total += t.duration(now).unwrap_or_default();
        }
    }
    buckets
}

/// 全部年份（含当前年，**自最早完成年份起**）的完成统计桶。末桶 = 当前年。
///
/// 按完成年份（而非创建年份）起算：避免"2023 创建、2026 完成"产生整段空年份桶。
pub fn year_buckets(todos: &[Todo], now: DateTime<Utc>) -> Vec<Bucket> {
    let this_year = local_date(now).year();
    let first_year = completed(todos)
        .filter_map(|t| t.finished_at.map(local_date))
        .map(|d| d.year())
        .min()
        .unwrap_or(this_year)
        .min(this_year);
    let mut buckets: Vec<Bucket> = (first_year..=this_year)
        .map(|y| Bucket::empty(format!("{y}")))
        .collect();
    for t in completed(todos) {
        let Some(finish) = t.finished_at else {
            continue;
        };
        let y = local_date(finish).year();
        if let Some(b) = buckets.get_mut((y - first_year) as usize) {
            b.count += 1;
            b.total += t.duration(now).unwrap_or_default();
        }
    }
    buckets
}

/// 各项目的完成统计桶（**全部历史**，不受周期窗口限制）。
///
/// - 无归属任务归「无项目」；`projects` 中不存在的 id 显示「未知项目」（防御性）；
/// - 按完成数降序（稳定排序：同数保持任务列表首次出现顺序）。
pub fn project_buckets(todos: &[Todo], projects: &[Project], now: DateTime<Utc>) -> Vec<Bucket> {
    let name_of: HashMap<Uuid, String> = projects.iter().map(|p| (p.id, p.name.clone())).collect();
    // 按 project_id 分组聚合，order 保持首次出现顺序（稳定排序基准）
    let mut order: Vec<Option<Uuid>> = Vec::new();
    let mut agg: HashMap<Option<Uuid>, (usize, Duration)> = HashMap::new();
    for t in completed(todos) {
        let key = t.project_id;
        if !agg.contains_key(&key) {
            order.push(key);
        }
        let entry = agg.entry(key).or_insert((0, Duration::zero()));
        entry.0 += 1;
        entry.1 += t.duration(now).unwrap_or_default();
    }
    let mut buckets: Vec<Bucket> = order
        .into_iter()
        .map(|id| {
            let (count, total) = agg.remove(&id).expect("聚合表必有该桶");
            let label = match id {
                None => "无项目".to_string(),
                Some(pid) => name_of
                    .get(&pid)
                    .cloned()
                    .unwrap_or_else(|| "未知项目".to_string()),
            };
            Bucket {
                label,
                count,
                total,
            }
        })
        .collect();
    buckets.sort_by_key(|b| std::cmp::Reverse(b.count));
    buckets
}

/// 全局汇总：总完成数 / 总耗时 / 平均耗时 / 最长耗时任务。
///
/// **除零防护**：无已完成任务时 `avg = 0`（不 panic——view 空数据场景会真实触发）。
pub fn totals(todos: &[Todo], now: DateTime<Utc>) -> Totals {
    let mut done_count = 0usize;
    let mut total = Duration::zero();
    let mut longest: Option<(Uuid, String, Duration)> = None;
    for t in completed(todos) {
        done_count += 1;
        let d = t.duration(now).unwrap_or_default();
        total += d;
        if longest.as_ref().is_none_or(|(_, _, l)| d > *l) {
            longest = Some((t.id, t.title.clone(), d));
        }
    }
    let avg = if done_count == 0 {
        Duration::zero()
    } else {
        total / done_count as i32
    };
    Totals {
        done_count,
        total,
        avg,
        longest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Todo;

    /// 构造「本地日期为 (y, m, d) 正午」的 UTC 时刻。
    ///
    /// 测试期望一律基于该**本地日期**推演（相对断言），与运行机器时区无关，
    /// 保证测试可移植（正午在 UTC-12 ~ +12 均同日，且本地正午不存在 DST 歧义）。
    fn local_midday(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        local_dt(NaiveDate::from_ymd_opt(y, m, d).expect("合法日期"), 12, 0)
    }

    /// 本地日期某时刻 → UTC（正午 / 日间时刻，`and_local_timezone` 无歧义）。
    fn local_dt(date: NaiveDate, hour: u32, minute: u32) -> DateTime<Utc> {
        date.and_hms_opt(hour, minute, 0)
            .expect("合法时刻")
            .and_local_timezone(Local)
            .earliest()
            .expect("本地日间时刻必存在")
            .with_timezone(&Utc)
    }

    /// 已完成任务：`finish` 前 `started_before` 开始。
    fn done(title: &str, finish: DateTime<Utc>, started_before: Duration) -> Todo {
        let mut t = Todo::new_full(title.into(), String::new(), None, None, None, None, finish);
        t.started_at = Some(finish - started_before);
        t.finished_at = Some(finish);
        t
    }

    /// 已完成任务但缺 `started_at`（防御场景：状态推导保证实际不可能）。
    fn done_no_start(title: &str, finish: DateTime<Utc>) -> Todo {
        let mut t = Todo::new_full(title.into(), String::new(), None, None, None, None, finish);
        t.finished_at = Some(finish);
        t
    }

    /// `now` 所在周的周一（本地日期）。
    fn week_monday(now: DateTime<Utc>) -> NaiveDate {
        let today = local_date(now);
        today - Days::new(u64::from(today.weekday().num_days_from_monday()))
    }

    #[test]
    fn week_buckets_12_buckets_ending_current_week() {
        let now = local_midday(2026, 6, 15);
        let monday = week_monday(now);
        // 本周一完成（30 分钟）；3 周前（2 小时）；11 周前（第 0 桶）；12 周前（窗口外忽略）
        let this_week = done("本周", local_dt(monday, 10, 0), Duration::minutes(30));
        let three_weeks = done(
            "三周前",
            local_dt(monday - Days::new(21), 9, 0),
            Duration::hours(2),
        );
        let eleven_weeks = done(
            "十一周前",
            local_dt(monday - Days::new(77), 8, 0),
            Duration::minutes(5),
        );
        let outside = done(
            "窗口外",
            local_dt(monday - Days::new(84), 7, 0),
            Duration::minutes(1),
        );
        let todos = vec![this_week, three_weeks, eleven_weeks, outside];

        let buckets = week_buckets(&todos, now, 12);

        assert_eq!(buckets.len(), 12);
        // 末桶 = 当前周
        assert_eq!(buckets[11].label, monday.format("%m-%d").to_string());
        assert_eq!(buckets[11].count, 1);
        assert_eq!(buckets[11].total, Duration::minutes(30));
        // 3 周前 → 第 8 桶；11 周前 → 第 0 桶
        assert_eq!(buckets[8].count, 1);
        assert_eq!(buckets[8].total, Duration::hours(2));
        assert_eq!(buckets[0].count, 1);
        // 窗口外忽略、其余桶 0 补齐
        assert_eq!(buckets[0].total, Duration::minutes(5));
        let total_count: usize = buckets.iter().map(|b| b.count).sum();
        assert_eq!(total_count, 3);
        assert_eq!(buckets[7].count, 0);
        assert_eq!(buckets[7].total, Duration::zero());
    }

    #[test]
    fn week_buckets_empty_is_zero_filled() {
        let now = local_midday(2026, 6, 15);
        let buckets = week_buckets(&[], now, 12);
        assert_eq!(buckets.len(), 12);
        assert!(
            buckets
                .iter()
                .all(|b| b.count == 0 && b.total == Duration::zero())
        );
    }

    #[test]
    fn month_buckets_cross_year_window() {
        let now = local_midday(2026, 1, 15);
        let buckets = month_buckets(&[], now, 12);
        // 窗口 = 2025-02 ~ 2026-01（跨年），标签从「2月」到「1月」
        let labels: Vec<&str> = buckets.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels.len(), 12);
        assert_eq!(labels[0], "2月");
        assert_eq!(labels[10], "12月");
        assert_eq!(labels[11], "1月");

        // 2025-12-25 完成的任务 → 倒数第 2 桶
        let t = done(
            "跨年任务",
            local_midday(2025, 12, 25),
            Duration::minutes(20),
        );
        let buckets = month_buckets(&[t], now, 12);
        assert_eq!(buckets[10].count, 1);
        assert_eq!(buckets[10].label, "12月");
        assert_eq!(buckets[10].total, Duration::minutes(20));
    }

    #[test]
    fn month_buckets_leap_feb29_and_mar31_edge() {
        // 2024-03-31（本地）：窗口 2023-04 ~ 2024-03，月份递减经过 2 月 29 日与 31 日边界
        let now = local_midday(2024, 3, 31);
        let buckets = month_buckets(&[], now, 12);
        let labels: Vec<&str> = buckets.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels[0], "4月");
        assert_eq!(labels[11], "3月");
        assert_eq!(labels[10], "2月");

        // 2024-02-29（闰日）完成 → 倒数第 2 桶
        let t = done("闰日任务", local_midday(2024, 2, 29), Duration::hours(1));
        let buckets = month_buckets(&[t], now, 12);
        assert_eq!(buckets[10].count, 1);
        assert_eq!(buckets[10].label, "2月");
    }

    #[test]
    fn year_buckets_all_years_with_zero_fill() {
        let now = local_midday(2026, 6, 1);
        let t2024 = done("2024 年", local_midday(2024, 5, 1), Duration::minutes(10));
        let t2026a = done("2026a", local_midday(2026, 1, 2), Duration::minutes(20));
        let t2026b = done("2026b", local_midday(2026, 5, 30), Duration::minutes(30));

        let buckets = year_buckets(&[t2024, t2026a, t2026b], now);

        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].label, "2024");
        assert_eq!(buckets[1].label, "2025");
        assert_eq!(buckets[2].label, "2026");
        assert_eq!(buckets[0].count, 1);
        assert_eq!(buckets[1].count, 0); // 中间年份 0 补齐
        assert_eq!(buckets[2].count, 2);
        assert_eq!(buckets[2].total, Duration::minutes(50));
    }

    #[test]
    fn year_buckets_empty_contains_current_year_only() {
        let now = local_midday(2026, 6, 1);
        let buckets = year_buckets(&[], now);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].label, "2026");
    }

    #[test]
    fn project_buckets_group_unassigned_unknown_and_sort() {
        let now = local_midday(2026, 6, 1);
        let p1 = crate::model::Project::new("工作".into(), now);
        let p2 = crate::model::Project::new("学习".into(), now);
        let orphan = Uuid::now_v7();

        let mut t1 = done("工作1", local_midday(2026, 5, 1), Duration::minutes(10));
        t1.project_id = Some(p1.id);
        let mut t2 = done("工作2", local_midday(2026, 5, 2), Duration::minutes(20));
        t2.project_id = Some(p1.id);
        let mut t3 = done("无项目", local_midday(2026, 5, 3), Duration::minutes(30));
        t3.project_id = None;
        let mut t4 = done("孤儿", local_midday(2026, 5, 4), Duration::minutes(40));
        t4.project_id = Some(orphan);
        let mut t5 = done("学习1", local_midday(2026, 5, 5), Duration::minutes(50));
        t5.project_id = Some(p2.id);

        let todos = vec![t1, t2, t3, t4, t5];
        let buckets = project_buckets(&todos, &[p1, p2], now);

        // 按完成数降序：工作 2 个最前；同 count(1) 稳定保持首次出现顺序
        let labels: Vec<&str> = buckets.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["工作", "无项目", "未知项目", "学习"]);
        let counts: Vec<usize> = buckets.iter().map(|b| b.count).collect();
        assert_eq!(counts, [2, 1, 1, 1]);
        assert_eq!(buckets[0].total, Duration::minutes(30));
        assert_eq!(buckets[3].total, Duration::minutes(50));
    }

    #[test]
    fn totals_empty_no_panic_and_zero_avg() {
        let now = local_midday(2026, 6, 1);
        let t = totals(&[], now);
        assert_eq!(t.done_count, 0);
        assert_eq!(t.total, Duration::zero());
        assert_eq!(t.avg, Duration::zero());
        assert!(t.longest.is_none());
    }

    #[test]
    fn totals_basic_avg_and_longest() {
        let now = local_midday(2026, 6, 1);
        let a = done("短任务", local_midday(2026, 5, 1), Duration::minutes(30));
        let b = done("长任务", local_midday(2026, 5, 2), Duration::minutes(90));
        let c = done("中任务", local_midday(2026, 5, 3), Duration::minutes(60));

        let t = totals(&[a, b, c], now);

        assert_eq!(t.done_count, 3);
        assert_eq!(t.total, Duration::hours(3));
        assert_eq!(t.avg, Duration::hours(1));
        let (_, title, d) = t.longest.expect("有任务必有最长");
        assert_eq!(title, "长任务");
        assert_eq!(d, Duration::minutes(90));
    }

    #[test]
    fn totals_defensive_started_at_none_counts_zero_duration() {
        let now = local_midday(2026, 6, 1);
        let t = done_no_start("缺开始时间", local_midday(2026, 5, 1));
        let totals = totals(&[t], now);
        assert_eq!(totals.done_count, 1);
        assert_eq!(totals.total, Duration::zero());
        assert_eq!(totals.avg, Duration::zero());
    }
}
