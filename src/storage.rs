//! 持久化：任务与项目统一以 JSON 文件存放在系统应用数据目录。
//!
//! Windows 下默认路径为 `%APPDATA%\quick-todo\todos.json`。
//! 文件缺失视为空数据；损坏 / 旧版格式文件返回错误（由 UI 提示，不崩溃）。
//!
//! 格式为严格 schema（开发阶段破坏性更新不兼容旧数据）：
//! 必填字段缺失、文件非 Store 对象（如旧版纯数组）直接解析失败；
//! 可选字段（`Option`）缺省等同 `null`（serde 固有语义）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{Project, SortMode, Todo};

/// 持久化数据：任务列表 + 项目列表 + 排序偏好，单文件存储。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    pub todos: Vec<Todo>,
    pub projects: Vec<Project>,
    /// 任务排序方式（持久化偏好；缺省 `Combined`）
    pub sort_mode: SortMode,
    /// 项目排序方式（持久化偏好；缺省 `Combined`）
    pub project_sort_mode: SortMode,
}

/// 数据文件路径（若无法确定系统目录，退化为当前目录下的 todos.json）。
pub fn data_file() -> PathBuf {
    directories::ProjectDirs::from("dev", "quick-todo", "quick-todo")
        .map(|dirs| dirs.data_dir().join("todos.json"))
        .unwrap_or_else(|| PathBuf::from("todos.json"))
}

/// 从默认数据文件加载任务与项目。
pub async fn load() -> Result<Store, String> {
    load_from(data_file()).await
}

/// 把序列化好的数据 JSON 写入默认数据文件。
pub async fn save(json: String) -> Result<(), String> {
    save_to(data_file(), json).await
}

/// 从指定路径加载（供测试复用）。
pub async fn load_from(path: PathBuf) -> Result<Store, String> {
    match tokio::fs::read_to_string(&path).await {
        Ok(contents) => {
            parse(&contents).map_err(|error| format!("解析 {} 失败: {error}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(error) => Err(format!("读取 {} 失败: {error}", path.display())),
    }
}

/// 解析数据文件内容（严格 schema：旧版纯数组 / 缺字段格式一律报错）。
fn parse(contents: &str) -> Result<Store, String> {
    serde_json::from_str(contents).map_err(|error| error.to_string())
}

/// 写入指定路径（供测试复用），自动创建父目录。
pub async fn save_to(path: PathBuf, json: String) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("创建目录 {} 失败: {error}", parent.display()))?;
    }
    tokio::fs::write(&path, json)
        .await
        .map_err(|error| format!("写入 {} 失败: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Priority;
    use chrono::Utc;
    use uuid::Uuid;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("quick-todo-test-{}-{name}", std::process::id()))
    }

    fn sample_todos() -> Vec<Todo> {
        vec![
            Todo {
                id: Uuid::now_v7(),
                title: "读书".into(),
                description: "睡前读两章".into(),
                priority: None,
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
    async fn save_then_load_roundtrip() {
        let path = temp_path("roundtrip.json");
        let project = Project::new_full(
            "工作".into(),
            Some(Priority::High),
            Some(Utc::now()),
            Some(Utc::now() + chrono::Duration::days(30)),
            Utc::now(),
        );
        let mut todos = sample_todos();
        todos[0].project_id = Some(project.id);
        let store = Store {
            todos,
            projects: vec![project],
            sort_mode: SortMode::Priority,
            project_sort_mode: SortMode::Combined,
        };

        save_to(path.clone(), serde_json::to_string_pretty(&store).unwrap())
            .await
            .unwrap();

        let loaded = load_from(path.clone()).await.unwrap();
        assert_eq!(loaded.todos.len(), 2);
        assert_eq!(loaded.todos[0].title, "读书");
        assert_eq!(loaded.todos[0].description, "睡前读两章"); // 描述往返保留
        assert_eq!(loaded.todos[0].id, store.todos[0].id);
        assert_eq!(loaded.todos[0].created_at, store.todos[0].created_at);
        assert_eq!(loaded.todos[0].project_id, Some(store.projects[0].id));
        assert_eq!(loaded.todos[0].due_at, store.todos[0].due_at); // 截止时间往返保留
        assert_eq!(loaded.todos[1].title, "跑步");
        assert_eq!(loaded.todos[1].due_at, None); // 无截止时间往返保持 None
        assert!(loaded.todos[1].started_at.is_some());
        assert!(loaded.todos[1].finished_at.is_some());
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "工作");
        assert_eq!(loaded.projects[0].id, store.projects[0].id);
        assert_eq!(loaded.projects[0].started_at, store.projects[0].started_at); // 起止时间往返保留
        assert_eq!(
            loaded.projects[0].finished_at,
            store.projects[0].finished_at
        );
        assert_eq!(loaded.projects[0].priority, store.projects[0].priority); // 优先级往返保留
        assert_eq!(loaded.sort_mode, SortMode::Priority); // 排序偏好往返保留
        assert_eq!(loaded.project_sort_mode, SortMode::Combined);

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn legacy_todos_array_is_rejected() {
        // 严格 schema：旧版纯任务数组格式不再自动迁移，直接报错
        let path = temp_path("legacy.json");
        let legacy = r#"[
            {
                "id": "0195c7e0-0000-7000-8000-000000000001",
                "title": "旧任务",
                "created_at": "2026-01-01T10:00:00Z",
                "started_at": null,
                "finished_at": null
            }
        ]"#;
        save_to(path.clone(), legacy.into()).await.unwrap();

        let result = load_from(path.clone()).await;

        assert!(result.is_err()); // 旧格式不兼容，UI 红字提示

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn missing_file_loads_empty() {
        let path = temp_path("missing.json");
        let _ = tokio::fs::remove_file(&path).await; // 确保不存在
        let loaded = load_from(path).await.unwrap();
        assert!(loaded.todos.is_empty());
        assert!(loaded.projects.is_empty());
    }

    #[tokio::test]
    async fn corrupted_file_reports_error() {
        let path = temp_path("corrupt.json");
        save_to(path.clone(), "这不是 JSON".into()).await.unwrap();

        let result = load_from(path.clone()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("解析"));

        let _ = tokio::fs::remove_file(&path).await;
    }
}
