//! 数据模型：任务记录与应用状态。
//!
//! 核心设计：任务的状态（未开始 / 进行中 / 已完成）**由时间字段推导**，
//! 而不是单独存储一个枚举。这样时间字段是唯一事实来源，
//! 从结构上杜绝"状态与时间不一致"的脏数据。

use chrono::{DateTime, Duration, Utc};
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

/// 一条任务记录：标题 + 归属项目 + 三个关键时间点。
///
/// - `project_id`：所属项目（可选，`None` 表示未归属）
/// - `created_at`：创建时间（添加任务时自动记录，不可为空）
/// - `started_at`：开始时间（点击"开始"时记录）
/// - `finished_at`：结束时间（点击"完成"时记录）
///
/// 时间统一以 UTC 存储，展示时再转换为本地时区。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: Uuid,
    pub title: String,
    /// 所属项目（`None` = 未归属）。
    /// `#[serde(default)]`：兼容旧版数据文件（无此字段）。
    #[serde(default)]
    pub project_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl Todo {
    /// 创建一条新任务（此时即记录创建时间）。
    pub fn new(title: String, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::now_v7(),
            title,
            project_id: None,
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

/// 应用整体状态（iced 函数式 API 中的 `State`）。
#[derive(Debug, Clone)]
pub struct App {
    /// 任务列表，新任务插在最前
    pub todos: Vec<Todo>,
    /// 项目列表，保持创建顺序
    pub projects: Vec<Project>,
    /// 输入框当前文本
    pub input: String,
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
}

impl Default for App {
    fn default() -> Self {
        Self {
            todos: Vec::new(),
            projects: Vec::new(),
            input: String::new(),
            project_input: String::new(),
            selected_project: None,
            editing_project: None,
            project_edit_input: String::new(),
            error: None,
            now: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn new_todo_records_creation_time() {
        let now = dt(1_700_000_000);
        let todo = Todo::new("写方案".into(), now);

        assert_eq!(todo.created_at, now);
        assert_eq!(todo.started_at, None);
        assert_eq!(todo.finished_at, None);
        assert_eq!(todo.status(), TodoStatus::Pending);
    }

    #[test]
    fn status_is_derived_from_times() {
        let now = dt(1_700_000_000);
        let mut todo = Todo::new("编码".into(), now);

        todo.started_at = Some(dt(1_700_000_100));
        assert_eq!(todo.status(), TodoStatus::InProgress);

        todo.finished_at = Some(dt(1_700_000_200));
        assert_eq!(todo.status(), TodoStatus::Done);
    }

    #[test]
    fn duration_matches_status() {
        let mut todo = Todo::new("测试".into(), dt(1_700_000_000));

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
        let a = Todo::new("a".into(), dt(1));
        let b = Todo::new("b".into(), dt(2));
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn new_todo_has_no_project() {
        let todo = Todo::new("无项目".into(), dt(1_700_000_000));
        assert_eq!(todo.project_id, None);
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
