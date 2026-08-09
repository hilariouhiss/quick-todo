//! 数据模型：任务记录与应用状态。
//!
//! 核心设计：任务的状态（未开始 / 进行中 / 已完成）**由时间字段推导**，
//! 而不是单独存储一个枚举。这样时间字段是唯一事实来源，
//! 从结构上杜绝"状态与时间不一致"的脏数据。

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    /// 已创建，尚未开始
    Pending,
    /// 已点击"开始"，正在执行
    InProgress,
    /// 已点击"完成"
    Done,
}

impl TodoStatus {
    /// 状态的中文显示名
    pub const fn label(self) -> &'static str {
        match self {
            TodoStatus::Pending => "未开始",
            TodoStatus::InProgress => "进行中",
            TodoStatus::Done => "已完成",
        }
    }
}

/// 一条任务记录：标题 + 可选描述 + 归属项目 + 截止时间 + 三个关键时间点。
///
/// - `description`：可选描述（空串 = 无描述，创建时填写，卡片只读显示）
/// - `project_id`：所属项目（可选，`None` 表示未归属）
/// - `due_at`：截止时间（可选，`None` 表示无截止；由弹窗添加时设置）
/// - `created_at`：创建时间（添加任务时自动记录，不可为空）
/// - `started_at`：开始时间（点击"开始"时记录）
/// - `finished_at`：结束时间（点击"完成"时记录）
///
/// 时间统一以 UTC 存储，展示时再转换为本地时区。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: Uuid,
    pub title: String,
    /// 可选描述（空串 = 无描述）。
    /// `#[serde(default)]`：兼容旧版数据文件（无此字段）。
    #[serde(default)]
    pub description: String,
    /// 所属项目（`None` = 未归属）。
    /// `#[serde(default)]`：兼容旧版数据文件（无此字段）。
    #[serde(default)]
    pub project_id: Option<Uuid>,
    /// 截止时间（`None` = 无截止）。
    /// `#[serde(default)]`：兼容旧版数据文件（无此字段）。
    #[serde(default)]
    pub due_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl Todo {
    /// 创建一条新任务（此时即记录创建时间与描述）。
    pub fn new(title: String, description: String, now: DateTime<Utc>) -> Self {
        Self::new_full(title, description, None, None, now)
    }

    /// 创建一条完整配置的新任务（弹窗添加用）：描述 + 归属项目 + 截止时间。
    pub fn new_full(
        title: String,
        description: String,
        project_id: Option<Uuid>,
        due_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            title,
            description,
            project_id,
            due_at,
            created_at: now,
            started_at: None,
            finished_at: None,
        }
    }

    /// 由时间字段推导当前状态。
    pub fn status(&self) -> TodoStatus {
        if self.finished_at.is_some() {
            TodoStatus::Done
        } else if self.started_at.is_some() {
            TodoStatus::InProgress
        } else {
            TodoStatus::Pending
        }
    }

    /// 任务耗时：
    /// - 进行中：开始时间 → 当前时间（实时）
    /// - 已完成：开始时间 → 结束时间
    /// - 未开始：`None`
    pub fn duration(&self, now: DateTime<Utc>) -> Option<Duration> {
        match (self.started_at, self.finished_at) {
            (Some(start), Some(finish)) => Some(finish - start),
            (Some(start), None) => Some(now - start),
            (None, _) => None,
        }
    }
}

/// 一个项目：任务的可选归属容器。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl Project {
    /// 创建一条新项目（此时即记录创建时间）。
    pub fn new(name: String, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::now_v7(),
            name,
            created_at: now,
        }
    }
}

/// 弹窗添加任务的表单状态（纯内存，不持久化；`App.add_dialog = None` 表示弹窗关闭）。
#[derive(Debug, Clone)]
pub struct AddDialog {
    /// 标题输入
    pub title: String,
    /// 描述输入
    pub description: String,
    /// 所属项目（`None` = 无项目）
    pub project_id: Option<Uuid>,
    /// 截止时间输入框的原始文本
    pub due_input: String,
    /// 截止时间的实时解析结果：
    /// `Ok(None)` = 留空；`Ok(Some)` = 解析成功；`Err` = 格式错误提示
    pub due_parsed: Result<Option<DateTime<Utc>>, String>,
}

impl Default for AddDialog {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            project_id: None,
            due_input: String::new(),
            due_parsed: Ok(None),
        }
    }
}

/// 弹窗内截止时间的快捷选项（选中后回填到文本输入框，仍可手动修改）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickDue {
    /// 今天 23:59
    Today,
    /// 明天 23:59
    Tomorrow,
    /// 本周日 23:59（本日为周日时即今天）
    Sunday,
}

impl QuickDue {
    /// 快捷选项的显示名。
    pub const fn label(self) -> &'static str {
        match self {
            QuickDue::Today => "今天 23:59",
            QuickDue::Tomorrow => "明天 23:59",
            QuickDue::Sunday => "本周日 23:59",
        }
    }

    /// 基于当前时间计算目标时刻的**本地日期**（当天结束 23:59）。
    fn target_date(self, now: DateTime<Utc>) -> NaiveDate {
        let today = now.with_timezone(&Local).date_naive();
        match self {
            QuickDue::Today => today,
            QuickDue::Tomorrow => today + Duration::days(1),
            // 距本周日天数：周日 = 0 → 今天；周一 = 6 → 本周日
            QuickDue::Sunday => {
                let days = (7 - today.weekday().num_days_from_sunday()) % 7;
                today + Duration::days(i64::from(days))
            }
        }
    }

    /// 生成回填到截止时间输入框的文本（如 `2026-01-31 23:59`）。
    pub fn due_text(self, now: DateTime<Utc>) -> String {
        format!("{}", self.target_date(now).format("%Y-%m-%d 23:59"))
    }
}

/// 解析截止时间输入文本（本地时区语义，存储转 UTC）。
///
/// - 空串 → `Ok(None)`（无截止时间）
/// - `YYYY-MM-DD` → 本地当天 **23:59:59**（"截止到当天结束"）
/// - `YYYY-MM-DD HH:MM` / `YYYY-MM-DD HH:MM:SS` → 精确时刻
/// - 其他 → `Err(提示文案)`
pub fn parse_due(input: &str) -> Result<Option<DateTime<Utc>>, String> {
    const FORMAT_HINT: &str = "截止时间格式：2026-01-31 或 2026-01-31 18:30";

    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }

    // 先试完整日期时间（分钟 / 秒两种粒度）
    for format in ["%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(input, format) {
            return local_to_utc(naive).map(Some);
        }
    }
    // 再试纯日期：当天结束（23:59:59）
    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d")
        && let Some(naive) = date.and_hms_opt(23, 59, 59)
    {
        return local_to_utc(naive).map(Some);
    }

    Err(FORMAT_HINT.into())
}

/// 本地时间（naive）转 UTC；DST 边界（不存在/歧义）取最早的映射。
fn local_to_utc(naive: NaiveDateTime) -> Result<DateTime<Utc>, String> {
    match naive.and_local_timezone(Local).earliest() {
        Some(local) => Ok(local.with_timezone(&Utc)),
        None => Err("无效的本地时间".into()),
    }
}

/// 应用整体状态（iced 函数式 API 中的 `State`）。
#[derive(Debug, Clone)]
pub struct App {
    /// 任务列表，新任务插在最前
    pub todos: Vec<Todo>,
    /// 项目列表，保持创建顺序
    pub projects: Vec<Project>,
    /// 输入框当前文本
    pub input: String,
    /// 描述输入框当前文本
    pub description_input: String,
    /// 新建项目输入框当前文本
    pub project_input: String,
    /// 当前筛选的项目（`None` = 全部）
    pub selected_project: Option<Uuid>,
    /// 正在内联重命名的项目（`None` = 无）
    pub editing_project: Option<Uuid>,
    /// 重命名输入框当前文本
    pub project_edit_input: String,
    /// 加载 / 保存出错时的提示
    pub error: Option<String>,
    /// "当前时间"，由每秒的时钟订阅刷新，用于实时耗时显示
    pub now: DateTime<Utc>,
    /// 项目侧边栏是否展开（纯 UI 状态，不持久化，启动默认收起）
    pub sidebar_visible: bool,
    /// 弹窗添加任务表单（`None` = 弹窗关闭；纯内存状态，不持久化）
    pub add_dialog: Option<AddDialog>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            todos: Vec::new(),
            projects: Vec::new(),
            input: String::new(),
            description_input: String::new(),
            project_input: String::new(),
            selected_project: None,
            editing_project: None,
            project_edit_input: String::new(),
            error: None,
            now: Utc::now(),
            sidebar_visible: false,
            add_dialog: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone};

    fn dt(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn new_todo_records_creation_time() {
        let now = dt(1_700_000_000);
        let todo = Todo::new("写方案".into(), "".into(), now);

        assert_eq!(todo.created_at, now);
        assert_eq!(todo.started_at, None);
        assert_eq!(todo.finished_at, None);
        assert_eq!(todo.status(), TodoStatus::Pending);
    }

    #[test]
    fn new_todo_stores_description() {
        let now = dt(1_700_000_000);
        let todo = Todo::new("写方案".into(), "先读需求再动手".into(), now);

        assert_eq!(todo.description, "先读需求再动手");

        let no_description = Todo::new("空描述".into(), "".into(), now);
        assert!(no_description.description.is_empty());
    }

    #[test]
    fn legacy_json_without_description_defaults_to_empty() {
        // 旧版数据：没有 description 字段，反序列化应自动落空
        let json = r#"{
            "id": "0195c7e0-0000-7000-8000-000000000001",
            "title": "旧任务",
            "created_at": "2026-01-01T10:00:00Z",
            "started_at": null,
            "finished_at": null
        }"#;
        let todo: Todo = serde_json::from_str(json).unwrap();
        assert_eq!(todo.title, "旧任务");
        assert!(todo.description.is_empty());
        assert_eq!(todo.project_id, None);
    }

    #[test]
    fn status_is_derived_from_times() {
        let now = dt(1_700_000_000);
        let mut todo = Todo::new("编码".into(), "".into(), now);

        todo.started_at = Some(dt(1_700_000_100));
        assert_eq!(todo.status(), TodoStatus::InProgress);

        todo.finished_at = Some(dt(1_700_000_200));
        assert_eq!(todo.status(), TodoStatus::Done);
    }

    #[test]
    fn duration_matches_status() {
        let mut todo = Todo::new("测试".into(), "".into(), dt(1_700_000_000));

        // 未开始：没有耗时
        assert_eq!(todo.duration(dt(1_700_000_300)), None);

        // 进行中：到当前时间的实时耗时
        todo.started_at = Some(dt(1_700_000_100));
        assert_eq!(
            todo.duration(dt(1_700_000_300)),
            Some(Duration::seconds(200))
        );

        // 已完成：开始到结束，与"当前时间"无关
        todo.finished_at = Some(dt(1_700_000_250));
        assert_eq!(
            todo.duration(dt(1_700_000_999)),
            Some(Duration::seconds(150))
        );
    }

    #[test]
    fn todo_ids_are_unique() {
        let a = Todo::new("a".into(), "".into(), dt(1));
        let b = Todo::new("b".into(), "".into(), dt(2));
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn new_todo_has_no_project() {
        let todo = Todo::new("无项目".into(), "".into(), dt(1_700_000_000));
        assert_eq!(todo.project_id, None);
    }

    #[test]
    fn new_full_sets_all_fields() {
        let now = dt(1_700_000_000);
        let due = dt(1_700_100_000);
        let project = Uuid::now_v7();
        let todo = Todo::new_full(
            "写方案".into(),
            "详细描述".into(),
            Some(project),
            Some(due),
            now,
        );

        assert_eq!(todo.title, "写方案");
        assert_eq!(todo.description, "详细描述");
        assert_eq!(todo.project_id, Some(project));
        assert_eq!(todo.due_at, Some(due));
        assert_eq!(todo.created_at, now);
        assert_eq!(todo.status(), TodoStatus::Pending);

        // 无项目、无截止时间同样合法
        let plain = Todo::new_full("简单任务".into(), "".into(), None, None, now);
        assert_eq!(plain.project_id, None);
        assert_eq!(plain.due_at, None);
    }

    #[test]
    fn legacy_json_without_due_at_defaults_to_none() {
        // 旧版数据：没有 due_at 字段，反序列化应自动落空
        let json = r#"{
            "id": "0195c7e0-0000-7000-8000-000000000001",
            "title": "旧任务",
            "created_at": "2026-01-01T10:00:00Z",
            "started_at": null,
            "finished_at": null
        }"#;
        let todo: Todo = serde_json::from_str(json).unwrap();
        assert_eq!(todo.due_at, None);
    }

    #[test]
    fn new_todo_defaults_have_no_due() {
        let todo = Todo::new("快捷添加".into(), "".into(), dt(1_700_000_000));
        assert_eq!(todo.due_at, None);
    }

    #[test]
    fn parse_due_empty_means_no_due() {
        assert_eq!(parse_due("").unwrap(), None);
        assert_eq!(parse_due("   ").unwrap(), None);
    }

    #[test]
    fn parse_due_date_means_end_of_day() {
        let due = parse_due("2026-01-31").unwrap().unwrap();
        let local = due.with_timezone(&Local);
        assert_eq!(
            local.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-01-31 23:59:59"
        );
    }

    #[test]
    fn parse_due_datetime_keeps_minutes() {
        let due = parse_due("2026-01-31 18:30").unwrap().unwrap();
        let local = due.with_timezone(&Local);
        assert_eq!(
            local.format("%Y-%m-%d %H:%M").to_string(),
            "2026-01-31 18:30"
        );

        let with_seconds = parse_due("2026-01-31 18:30:45").unwrap().unwrap();
        let local = with_seconds.with_timezone(&Local);
        assert_eq!(
            local.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-01-31 18:30:45"
        );
    }

    #[test]
    fn parse_due_whitespace_is_trimmed() {
        let due = parse_due("  2026-01-31  ").unwrap();
        assert!(due.is_some());
    }

    #[test]
    fn parse_due_invalid_returns_hint() {
        for bad in [
            "明天",
            "2026-13-45",
            "2026-01-31 25:00",
            "2026/01/31",
            "abc",
        ] {
            let err = parse_due(bad).unwrap_err();
            assert!(err.contains("2026-01-31"), "非法输入 {bad:?} 的提示: {err}");
        }
    }

    #[test]
    fn parse_due_roundtrips_through_utc() {
        let due = parse_due("2026-01-31 18:30").unwrap().unwrap();
        // UTC 存储与本地时刻一致：本地转回后仍为 18:30
        let back = due.with_timezone(&Local);
        assert_eq!(
            back.format("%Y-%m-%d %H:%M").to_string(),
            "2026-01-31 18:30"
        );
    }

    #[test]
    fn quick_due_labels_are_readable() {
        assert_eq!(QuickDue::Today.label(), "今天 23:59");
        assert_eq!(QuickDue::Tomorrow.label(), "明天 23:59");
        assert_eq!(QuickDue::Sunday.label(), "本周日 23:59");
    }

    #[test]
    fn quick_due_today_is_local_today_end() {
        let now = dt(1_700_000_000); // 固定时刻，本地日期确定
        let text = QuickDue::Today.due_text(now);
        let local = now.with_timezone(&Local);
        let expected = format!("{} 23:59", local.format("%Y-%m-%d"));
        assert_eq!(text, expected);
    }

    #[test]
    fn quick_due_tomorrow_is_next_day() {
        let now = dt(1_700_000_000);
        let today = now.with_timezone(&Local).date_naive();
        let text = QuickDue::Tomorrow.due_text(now);
        assert_eq!(
            text,
            format!("{} 23:59", (today + Duration::days(1)).format("%Y-%m-%d"))
        );
    }

    #[test]
    fn quick_due_sunday_is_this_week_sunday() {
        let now = dt(1_700_000_000);
        let today = now.with_timezone(&Local).date_naive();
        let days = (7 - today.weekday().num_days_from_sunday()) % 7;
        let text = QuickDue::Sunday.due_text(now);
        assert_eq!(
            text,
            format!(
                "{} 23:59",
                (today + Duration::days(i64::from(days))).format("%Y-%m-%d")
            )
        );
        // 回填文本必须能被解析函数接受（闭环校验）
        assert!(parse_due(&text).unwrap().is_some());
    }

    #[test]
    fn quick_due_text_is_parseable() {
        for quick in [QuickDue::Today, QuickDue::Tomorrow, QuickDue::Sunday] {
            let text = quick.due_text(dt(1_700_000_000));
            assert!(parse_due(&text).is_ok(), "{text} 应可被 parse_due 接受");
        }
    }

    #[test]
    fn project_new_records_creation_time() {
        let now = dt(1_700_000_000);
        let project = Project::new("工作".into(), now);

        assert_eq!(project.name, "工作");
        assert_eq!(project.created_at, now);
    }

    #[test]
    fn project_ids_are_unique() {
        let a = Project::new("a".into(), dt(1));
        let b = Project::new("b".into(), dt(2));
        assert_ne!(a.id, b.id);
    }
}
