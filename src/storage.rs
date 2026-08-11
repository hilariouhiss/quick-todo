//! 持久化：任务 / 项目存 SQLite 单文件（quick-todo.db），排序偏好存独立 JSON（settings.json）。
//!
//! 两个文件都放在**可执行文件同目录**（`current_exe()` 父目录，失败退化当前目录）。
//! 文件缺失视为空数据；损坏文件返回错误（由 UI 提示，不崩溃）。
//!
//! 写盘策略（增量写）：每个数据变更由 `update` 层派发一个 `Op`（携带完整行状态），
//! `apply` 在单事务内执行对应 SQL；排序偏好整文件覆写 `settings.json`（无读-改-写）。
//! rusqlite 为同步 API，一律经 `tokio::task::spawn_blocking` 包裹，不阻塞 UI 线程。

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{Priority, Project, SortMode, ThemeMode, Todo};

/// 内存持久化数据：任务 + 项目 + 排序偏好 + 主题模式（load 结果 / persist 数据源）。
#[derive(Debug, Clone, Default)]
pub struct Store {
    pub todos: Vec<Todo>,
    pub projects: Vec<Project>,
    pub sort_mode: SortMode,
    pub project_sort_mode: SortMode,
    pub theme_mode: ThemeMode,
}

/// 排序偏好 + 主题模式（独立 settings.json 持久化；缺省「综合」/「跟随系统」）。
/// 注意：`theme_mode` 为**必填键**——旧文件缺键解析失败（开发阶段破坏性更新，不迁移）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub sort_mode: SortMode,
    pub project_sort_mode: SortMode,
    pub theme_mode: ThemeMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sort_mode: SortMode::Combined,
            project_sort_mode: SortMode::Combined,
            theme_mode: ThemeMode::System,
        }
    }
}

/// 单次数据变更操作（增量写盘的最小单位，携带完整行状态）。
#[derive(Debug, Clone)]
pub enum Op {
    /// 新增任务
    InsertTodo(Todo),
    /// 更新任务（开始 / 完成 / 编辑保存共用，整行覆盖）
    UpdateTodo(Todo),
    /// 删除任务
    DeleteTodo(Uuid),
    /// 新增项目
    InsertProject(Project),
    /// 更新项目（编辑保存）
    UpdateProject(Project),
    /// 删除项目（单事务内先解除其下任务归属，与内存语义一致）
    DeleteProject(Uuid),
}

/// 数据目录：可执行文件同目录（`current_exe()` 失败时退化当前目录）。
pub fn data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 数据库文件路径：可执行文件同目录下的 quick-todo.db。
pub fn db_file() -> PathBuf {
    data_dir().join("quick-todo.db")
}

/// 排序偏好文件路径：可执行文件同目录下的 settings.json。
pub fn settings_file() -> PathBuf {
    data_dir().join("settings.json")
}

/// 从默认位置加载任务 / 项目 / 排序偏好（DB 缺失视为空数据，settings 缺失取默认「综合」）。
pub async fn load() -> Result<Store, String> {
    let (todos, projects) = load_db_from(&db_file()).await?;
    let settings = load_settings_from(&settings_file()).await?;
    Ok(Store {
        todos,
        projects,
        sort_mode: settings.sort_mode,
        project_sort_mode: settings.project_sort_mode,
        theme_mode: settings.theme_mode,
    })
}

/// 执行一次数据变更（默认数据库文件）。
pub async fn apply(op: Op) -> Result<(), String> {
    apply_to(db_file(), op).await
}

/// 整文件覆写排序偏好与主题模式（值均来自 app 当前状态，无读-改-写竞态）。
/// 经 `write_settings_atomic` 原子替换：崩溃时目标文件要么是旧内容、要么是新内容，不会半写坏。
pub async fn save_settings(
    sort_mode: SortMode,
    project_sort_mode: SortMode,
    theme_mode: ThemeMode,
) -> Result<(), String> {
    let settings = Settings {
        sort_mode,
        project_sort_mode,
        theme_mode,
    };
    write_settings_atomic(&settings_file(), &settings).await
}

/// 原子覆写 settings.json（供默认路径与测试复用）：
/// 写同目录**唯一名**临时文件 → `rename` 原子替换。
///
/// - 唯一名（uuid 后缀）防并发 `save_settings`（快速连续切排序 / 主题，多个 Task 可同时在飞）
///   互相截断 / 交错写同一临时文件；
/// - `rename` 为原子替换（Windows 下 MoveFileEx + MOVEFILE_REPLACE_EXISTING，覆盖已存在文件成立），
///   不做"删除目标再 rename"兜底（破坏原子性）；失败时尽力清理临时文件。
async fn write_settings_atomic(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("创建目录 {} 失败: {error}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("序列化设置失败: {error}"))?;
    let tmp = path.with_file_name(format!(
        "{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("settings"),
        Uuid::now_v7(),
    ));
    if let Err(error) = tokio::fs::write(&tmp, &json).await {
        // 写入失败：清理残留临时文件（尽力而为），目标文件不受影响
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("写入 {} 失败: {error}", path.display()));
    }
    if let Err(error) = tokio::fs::rename(&tmp, path).await {
        // 替换失败：清理残留临时文件（尽力而为），目标文件保持旧内容
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("写入 {} 失败: {error}", path.display()));
    }
    Ok(())
}

/// 从指定路径加载数据库内容（供测试复用）。
async fn load_db_from(path: &Path) -> Result<(Vec<Todo>, Vec<Project>), String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || load_db_sync(&path))
        .await
        .map_err(|error| format!("加载数据库任务失败: {error}"))?
}

/// 从指定路径加载排序偏好与主题模式（供测试复用）；文件缺失取默认，缺键（如旧文件无
/// `theme_mode`）解析失败报错（开发阶段破坏性更新，不迁移）。
async fn load_settings_from(path: &Path) -> Result<Settings, String> {
    match tokio::fs::read_to_string(path).await {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| format!("解析 {} 失败: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(format!("读取 {} 失败: {error}", path.display())),
    }
}

/// 对指定路径的数据库执行一次变更（供测试复用）。
async fn apply_to(path: PathBuf, op: Op) -> Result<(), String> {
    tokio::task::spawn_blocking(move || apply_sync(&path, &op))
        .await
        .map_err(|error| format!("写盘任务失败: {error}"))?
}

/// 打开数据库并初始化 schema（幂等；垃圾字节文件在此触发 SQLITE_NOTADB）。
fn open_db(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path)
        .map_err(|error| format!("打开数据库 {} 失败: {error}", path.display()))?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|error| format!("设置 busy_timeout 失败: {error}"))?;
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(|error| format!("启用外键失败: {error}"))?;
    conn.execute_batch(SCHEMA)
        .map_err(|error| format!("初始化数据库表失败: {error}"))?;
    Ok(conn)
}

/// 建表 DDL（开发阶段破坏性更新直接改此 SQL，旧库不兼容即报错）。
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS todos (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    priority    TEXT,
    project_id  TEXT REFERENCES projects(id) ON DELETE SET NULL,
    due_at      TEXT,
    created_at  TEXT NOT NULL,
    started_at  TEXT,
    finished_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_todos_project ON todos(project_id);
CREATE INDEX IF NOT EXISTS idx_todos_due ON todos(due_at);

CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    priority    TEXT,
    started_at  TEXT,
    finished_at TEXT,
    created_at  TEXT NOT NULL
);
";

fn apply_sync(path: &Path, op: &Op) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("创建目录 {} 失败: {error}", parent.display()))?;
    }
    let mut conn = open_db(path)?;
    let tx = conn
        .transaction()
        .map_err(|error| format!("开启事务失败: {error}"))?;
    match op {
        Op::InsertTodo(todo) => insert_todo(&tx, todo)?,
        Op::UpdateTodo(todo) => update_todo(&tx, todo)?,
        Op::DeleteTodo(id) => {
            tx.execute("DELETE FROM todos WHERE id = ?1", params![id.to_string()])
                .map_err(|error| format!("删除任务失败: {error}"))?;
        }
        Op::InsertProject(project) => insert_project(&tx, project)?,
        Op::UpdateProject(project) => update_project(&tx, project)?,
        Op::DeleteProject(id) => {
            // 与内存语义一致：先解除其下任务归属，再删除项目本身
            tx.execute(
                "UPDATE todos SET project_id = NULL WHERE project_id = ?1",
                params![id.to_string()],
            )
            .map_err(|error| format!("解除任务归属失败: {error}"))?;
            tx.execute(
                "DELETE FROM projects WHERE id = ?1",
                params![id.to_string()],
            )
            .map_err(|error| format!("删除项目失败: {error}"))?;
        }
    }
    tx.commit()
        .map_err(|error| format!("提交事务失败: {error}"))?;
    Ok(())
}

fn insert_todo(tx: &rusqlite::Transaction<'_>, todo: &Todo) -> Result<(), String> {
    tx.execute(
        "INSERT INTO todos (id, title, description, priority, project_id, due_at, created_at, started_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            todo.id.to_string(),
            todo.title,
            todo.description,
            priority_text(todo.priority),
            option_id_text(todo.project_id),
            option_time_text(todo.due_at),
            todo.created_at.to_rfc3339(),
            option_time_text(todo.started_at),
            option_time_text(todo.finished_at),
        ],
    )
    .map_err(|error| format!("插入任务失败: {error}"))?;
    Ok(())
}

fn update_todo(tx: &rusqlite::Transaction<'_>, todo: &Todo) -> Result<(), String> {
    tx.execute(
        "UPDATE todos SET title = ?2, description = ?3, priority = ?4, project_id = ?5,
         due_at = ?6, created_at = ?7, started_at = ?8, finished_at = ?9 WHERE id = ?1",
        params![
            todo.id.to_string(),
            todo.title,
            todo.description,
            priority_text(todo.priority),
            option_id_text(todo.project_id),
            option_time_text(todo.due_at),
            todo.created_at.to_rfc3339(),
            option_time_text(todo.started_at),
            option_time_text(todo.finished_at),
        ],
    )
    .map_err(|error| format!("更新任务失败: {error}"))?;
    Ok(())
}

fn insert_project(tx: &rusqlite::Transaction<'_>, project: &Project) -> Result<(), String> {
    tx.execute(
        "INSERT INTO projects (id, name, priority, started_at, finished_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            project.id.to_string(),
            project.name,
            priority_text(project.priority),
            option_time_text(project.started_at),
            option_time_text(project.finished_at),
            project.created_at.to_rfc3339(),
        ],
    )
    .map_err(|error| format!("插入项目失败: {error}"))?;
    Ok(())
}

fn update_project(tx: &rusqlite::Transaction<'_>, project: &Project) -> Result<(), String> {
    tx.execute(
        "UPDATE projects SET name = ?2, priority = ?3, started_at = ?4, finished_at = ?5,
         created_at = ?6 WHERE id = ?1",
        params![
            project.id.to_string(),
            project.name,
            priority_text(project.priority),
            option_time_text(project.started_at),
            option_time_text(project.finished_at),
            project.created_at.to_rfc3339(),
        ],
    )
    .map_err(|error| format!("更新项目失败: {error}"))?;
    Ok(())
}

fn load_db_sync(path: &Path) -> Result<(Vec<Todo>, Vec<Project>), String> {
    if !path.exists() {
        // 缺失视为空数据（不建表，首次写入时自动创建）
        return Ok((Vec::new(), Vec::new()));
    }
    let conn = open_db(path)?;

    let mut todos = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, priority, project_id, due_at, created_at, started_at, finished_at
             FROM todos ORDER BY rowid",
        )
        .map_err(|error| format!("查询任务失败: {error}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })
        .map_err(|error| format!("读取任务失败: {error}"))?;
    for row in rows {
        let (
            id,
            title,
            description,
            priority,
            project_id,
            due_at,
            created_at,
            started_at,
            finished_at,
        ) = row.map_err(|error| format!("解析任务行失败: {error}"))?;
        todos.push(Todo {
            id: parse_uuid(&id)?,
            title,
            description,
            priority: parse_priority(priority.as_deref()),
            project_id: parse_option_uuid(project_id.as_deref())?,
            due_at: parse_option_time(due_at.as_deref())?,
            created_at: parse_time(&created_at)?,
            started_at: parse_option_time(started_at.as_deref())?,
            finished_at: parse_option_time(finished_at.as_deref())?,
        });
    }

    let mut projects = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT id, name, priority, started_at, finished_at, created_at
             FROM projects ORDER BY rowid",
        )
        .map_err(|error| format!("查询项目失败: {error}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|error| format!("读取项目失败: {error}"))?;
    for row in rows {
        let (id, name, priority, started_at, finished_at, created_at) =
            row.map_err(|error| format!("解析项目行失败: {error}"))?;
        projects.push(Project {
            id: parse_uuid(&id)?,
            name,
            priority: parse_priority(priority.as_deref()),
            started_at: parse_option_time(started_at.as_deref())?,
            finished_at: parse_option_time(finished_at.as_deref())?,
            created_at: parse_time(&created_at)?,
        });
    }

    Ok((todos, projects))
}

/// 优先级 → 英文变体名文本（`None` → NULL）。
fn priority_text(priority: Option<Priority>) -> Option<String> {
    priority.map(|value| format!("{value:?}"))
}

/// 解析优先级文本；非法值容错为 `None`。
fn parse_priority(text: Option<&str>) -> Option<Priority> {
    match text {
        Some("Low") => Some(Priority::Low),
        Some("Medium") => Some(Priority::Medium),
        Some("High") => Some(Priority::High),
        _ => None,
    }
}

fn option_id_text(id: Option<Uuid>) -> Option<String> {
    id.map(|value| value.to_string())
}

fn parse_uuid(text: &str) -> Result<Uuid, String> {
    Uuid::parse_str(text).map_err(|error| format!("解析 UUID「{text}」失败: {error}"))
}

fn parse_option_uuid(text: Option<&str>) -> Result<Option<Uuid>, String> {
    text.map(parse_uuid).transpose()
}

/// 时间 → ISO-8601 UTC 文本（`None` → NULL）。
fn option_time_text(time: Option<DateTime<Utc>>) -> Option<String> {
    time.map(|value| value.to_rfc3339())
}

/// 解析 ISO-8601 文本为 UTC 时间（rfc3339 解析结果为 FixedOffset，统一转 UTC）。
fn parse_time(text: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(text)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| format!("解析时间「{text}」失败: {error}"))
}

fn parse_option_time(text: Option<&str>) -> Result<Option<DateTime<Utc>>, String> {
    text.map(parse_time).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Priority;
    use chrono::TimeZone;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("quick-todo-test-{}-{name}", std::process::id()))
    }

    /// 向指定路径覆写 settings.json（测试辅助，走真实原子写路径）。
    async fn save_settings_to(path: &Path, settings: Settings) -> Result<(), String> {
        write_settings_atomic(path, &settings).await
    }

    fn sample_todos() -> Vec<Todo> {
        vec![
            Todo {
                id: Uuid::now_v7(),
                title: "读书".into(),
                description: "睡前读两章".into(),
                priority: Some(Priority::High),
                project_id: None,
                due_at: Some(chrono::Utc::now() + chrono::Duration::days(1)),
                created_at: Utc::now(),
                started_at: None,
                finished_at: None,
            },
            Todo {
                id: Uuid::now_v7(),
                title: "跑步".into(),
                description: String::new(),
                priority: None,
                project_id: None,
                due_at: None,
                created_at: Utc::now(),
                started_at: Some(Utc::now()),
                finished_at: Some(Utc::now()),
            },
        ]
    }

    #[tokio::test]
    async fn apply_insert_then_load_roundtrip() {
        let path = temp_path("roundtrip.db");
        let _ = std::fs::remove_file(&path).ok();
        let project = Project::new_full(
            "工作".into(),
            Some(Priority::High),
            Some(Utc::now()),
            Some(Utc::now() + chrono::Duration::days(30)),
            Utc::now(),
        );
        let mut todos = sample_todos();
        todos[0].project_id = Some(project.id);

        apply_to(path.clone(), Op::InsertProject(project.clone()))
            .await
            .unwrap();
        for todo in &todos {
            apply_to(path.clone(), Op::InsertTodo(todo.clone()))
                .await
                .unwrap();
        }

        let (loaded_todos, loaded_projects) = load_db_from(&path).await.unwrap();
        assert_eq!(loaded_todos.len(), 2);
        assert_eq!(loaded_todos[0].title, "读书");
        assert_eq!(loaded_todos[0].description, "睡前读两章"); // 描述往返保留
        assert_eq!(loaded_todos[0].id, todos[0].id);
        assert_eq!(loaded_todos[0].created_at, todos[0].created_at);
        assert_eq!(loaded_todos[0].project_id, Some(project.id));
        assert_eq!(loaded_todos[0].due_at, todos[0].due_at); // 截止时间往返保留
        assert_eq!(loaded_todos[0].priority, Some(Priority::High)); // 优先级往返保留
        assert_eq!(loaded_todos[1].title, "跑步");
        assert_eq!(loaded_todos[1].due_at, None); // 无截止时间往返保持 None
        assert!(loaded_todos[1].started_at.is_some());
        assert!(loaded_todos[1].finished_at.is_some());
        assert_eq!(loaded_projects.len(), 1);
        assert_eq!(loaded_projects[0].name, "工作");
        assert_eq!(loaded_projects[0].id, project.id);
        assert_eq!(loaded_projects[0].started_at, project.started_at); // 起止时间往返保留
        assert_eq!(loaded_projects[0].finished_at, project.finished_at);
        assert_eq!(loaded_projects[0].priority, project.priority);

        let _ = std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn apply_update_persists_changes() {
        let path = temp_path("update.db");
        let _ = std::fs::remove_file(&path).ok();
        let mut todo = Todo::new("先开始".into(), dt());
        apply_to(path.clone(), Op::InsertTodo(todo.clone()))
            .await
            .unwrap();

        // 开始 → 完成 → 编辑标题：三次 UpdateTodo 携带完整行状态
        todo.started_at = Some(dt());
        apply_to(path.clone(), Op::UpdateTodo(todo.clone()))
            .await
            .unwrap();
        todo.finished_at = Some(dt());
        apply_to(path.clone(), Op::UpdateTodo(todo.clone()))
            .await
            .unwrap();
        todo.title = "已改标题".into();
        todo.due_at = Some(dt());
        apply_to(path.clone(), Op::UpdateTodo(todo.clone()))
            .await
            .unwrap();

        let (loaded, _) = load_db_from(&path).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "已改标题");
        assert!(loaded[0].started_at.is_some());
        assert!(loaded[0].finished_at.is_some());
        assert!(loaded[0].due_at.is_some());

        let _ = std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn apply_delete_removes_row() {
        let path = temp_path("delete.db");
        let _ = std::fs::remove_file(&path).ok();
        let todo = Todo::new("待删".into(), dt());
        let project = Project::new("待删项目".into(), dt());
        apply_to(path.clone(), Op::InsertTodo(todo.clone()))
            .await
            .unwrap();
        apply_to(path.clone(), Op::InsertProject(project.clone()))
            .await
            .unwrap();

        apply_to(path.clone(), Op::DeleteTodo(todo.id))
            .await
            .unwrap();
        apply_to(path.clone(), Op::DeleteProject(project.id))
            .await
            .unwrap();

        let (todos, projects) = load_db_from(&path).await.unwrap();
        assert!(todos.is_empty());
        assert!(projects.is_empty());

        let _ = std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn apply_delete_project_unassigns_todos() {
        let path = temp_path("cascade.db");
        let _ = std::fs::remove_file(&path).ok();
        let project = Project::new("将被删".into(), dt());
        let mut todo = Todo::new("归属任务".into(), dt());
        todo.project_id = Some(project.id);
        apply_to(path.clone(), Op::InsertProject(project.clone()))
            .await
            .unwrap();
        apply_to(path.clone(), Op::InsertTodo(todo.clone()))
            .await
            .unwrap();

        // 删除项目：其下任务归属被解除（与内存语义一致），任务本身保留
        apply_to(path.clone(), Op::DeleteProject(project.id))
            .await
            .unwrap();

        let (todos, projects) = load_db_from(&path).await.unwrap();
        assert!(projects.is_empty());
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].project_id, None);

        let _ = std::fs::remove_file(&path).ok();
    }

    /// 断言指定路径所在目录无本测试产生的 `.tmp` 残留（原子写测试辅助）。
    /// 只匹配以目标文件名开头的临时文件（临时目录混有系统其他组件的 `.tmp`，如
    /// dict_cache / TCD / 裸 uuid 命名，不能全量扫描）。
    async fn assert_no_tmp_leftover(path: &Path) {
        let parent = path.parent().unwrap();
        let target = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let leftovers: Vec<String> = std::fs::read_dir(parent)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(&target) && name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件: {leftovers:?}");
    }

    #[tokio::test]
    async fn settings_atomic_overwrite_no_corruption() {
        let path = temp_path("settings-atomic.json");
        let _ = std::fs::remove_file(&path).ok();

        // 连续覆写多次（模拟快速连续切换排序 / 主题）：每次目标内容完整、可解析
        for mode in [SortMode::Priority, SortMode::Due, SortMode::Combined] {
            save_settings_to(
                &path,
                Settings {
                    sort_mode: mode,
                    project_sort_mode: SortMode::Combined,
                    theme_mode: ThemeMode::System,
                },
            )
            .await
            .unwrap();
            let loaded = load_settings_from(&path).await.unwrap();
            assert_eq!(loaded.sort_mode, mode, "覆写后内容应为最新完整值");
        }

        // 无 .tmp 残留（每次覆写的临时文件均已 rename 或清理）
        assert_no_tmp_leftover(&path).await;
        let _ = std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn settings_atomic_write_failure_leaves_no_tmp() {
        // 失败路径：目标路径是目录（写入 / rename 均失败），应清理临时文件、目标不受影响
        let dir = temp_path("settings-as-dir");
        let _ = std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();

        let result = save_settings_to(&dir, Settings::default()).await;
        assert!(result.is_err());
        assert_no_tmp_leftover(&dir).await;

        let _ = std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn settings_json_roundtrip() {
        let path = temp_path("settings.json");
        let _ = std::fs::remove_file(&path).ok();

        // 缺失 → 默认「综合」/「跟随系统」
        let settings = load_settings_from(&path).await.unwrap();
        assert_eq!(settings.sort_mode, SortMode::Combined);
        assert_eq!(settings.project_sort_mode, SortMode::Combined);
        assert_eq!(settings.theme_mode, ThemeMode::System);

        // 写入后往返
        save_settings_to(
            &path,
            Settings {
                sort_mode: SortMode::Priority,
                project_sort_mode: SortMode::Due,
                theme_mode: ThemeMode::Dark,
            },
        )
        .await
        .unwrap();
        let loaded = load_settings_from(&path).await.unwrap();
        assert_eq!(loaded.sort_mode, SortMode::Priority);
        assert_eq!(loaded.project_sort_mode, SortMode::Due);
        assert_eq!(loaded.theme_mode, ThemeMode::Dark);

        // 旧文件缺 theme_mode 键 → 解析失败报错（破坏性更新，不迁移）
        std::fs::write(
            &path,
            "{\"sort_mode\":\"Combined\",\"project_sort_mode\":\"Combined\"}",
        )
        .unwrap();
        let result = load_settings_from(&path).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("解析"));

        // 损坏 → Err
        std::fs::write(&path, "这不是 JSON").unwrap();
        let result = load_settings_from(&path).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("解析"));

        let _ = std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn missing_db_loads_empty_then_writes() {
        let path = temp_path("missing.db");
        let _ = std::fs::remove_file(&path).ok();

        // 缺失 → 空数据
        let (todos, projects) = load_db_from(&path).await.unwrap();
        assert!(todos.is_empty());
        assert!(projects.is_empty());

        // 首次写入自动建表成功
        let todo = Todo::new("新任务".into(), dt());
        apply_to(path.clone(), Op::InsertTodo(todo.clone()))
            .await
            .unwrap();
        let (todos, _) = load_db_from(&path).await.unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].title, "新任务");

        let _ = std::fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn corrupted_db_reports_error() {
        let path = temp_path("corrupt.db");
        std::fs::write(&path, "这不是 SQLite 数据库").unwrap();

        // Connection::open 对垃圾字节成功，实际执行语句时才触发 NOTADB
        let result = load_db_from(&path).await;
        assert!(result.is_err());

        let _ = std::fs::remove_file(&path).ok();
    }

    fn dt() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn priority_text_roundtrip() {
        assert_eq!(priority_text(None), None);
        assert_eq!(priority_text(Some(Priority::Low)).as_deref(), Some("Low"));
        assert_eq!(
            priority_text(Some(Priority::Medium)).as_deref(),
            Some("Medium")
        );
        assert_eq!(priority_text(Some(Priority::High)).as_deref(), Some("High"));
        assert_eq!(parse_priority(Some("Low")), Some(Priority::Low));
        assert_eq!(parse_priority(Some("Medium")), Some(Priority::Medium));
        assert_eq!(parse_priority(Some("High")), Some(Priority::High));
        assert_eq!(parse_priority(Some("未知")), None);
        assert_eq!(parse_priority(None), None);
    }

    #[test]
    fn time_text_roundtrip() {
        let time = dt();
        assert_eq!(option_time_text(None), None);
        let text = option_time_text(Some(time)).unwrap();
        assert_eq!(parse_option_time(Some(&text)).unwrap(), Some(time));
        assert_eq!(parse_option_time(None).unwrap(), None);
    }
}
