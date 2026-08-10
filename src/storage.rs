//! 持久化：任务与项目统一以 JSON 文件存放在系统应用数据目录。
//!
//! Windows 下默认路径为 `%APPDATA%\quick-todo\todos.json`；
//! 重命名前的旧目录（`%APPDATA%\iced-todos`）若有数据，新路径缺失时自动回退加载（一次性迁移）。
//! 文件缺失视为空数据；损坏文件返回错误（由 UI 提示，不崩溃）。
//!
//! 格式演进：旧版本文件为纯任务数组 `Vec<Todo>`，
//! 新版本为 `Store { todos, projects }`；加载时自动兼容迁移旧格式。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{Project, SortMode, Todo};

/// 持久化数据：任务列表 + 项目列表 + 排序偏好，单文件存储。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    pub todos: Vec<Todo>,
    pub projects: Vec<Project>,
    /// 任务排序方式（持久化偏好；旧文件缺省 → 综合）
    #[serde(default)]
    pub sort_mode: SortMode,
    /// 项目排序方式（持久化偏好；旧文件缺省 → 综合）
    #[serde(default)]
    pub project_sort_mode: SortMode,
}

/// 数据文件路径（若无法确定系统目录，退化为当前目录下的 todos.json）。
pub fn data_file() -> PathBuf {
    directories::ProjectDirs::from("dev", "quick-todo", "quick-todo")
        .map(|dirs| dirs.data_dir().join("todos.json"))
        .unwrap_or_else(|| PathBuf::from("todos.json"))
}

/// 旧版数据文件路径（应用重命名前的目录标识，用于一次性迁移旧数据）。
fn legacy_data_file() -> PathBuf {
    directories::ProjectDirs::from("dev", "iced-demo", "iced-todos")
        .map(|dirs| dirs.data_dir().join("todos.json"))
        .unwrap_or_else(|| PathBuf::from("todos.json"))
}

/// 从默认数据文件加载任务与项目；新路径缺失时回退旧版路径（兼容重命名前的数据）。
pub async fn load() -> Result<Store, String> {
    load_with_fallback(data_file(), legacy_data_file()).await
}

/// 优先读新路径，缺失时回退旧路径（供测试直接指定两条路径）。
async fn load_with_fallback(new_path: PathBuf, legacy_path: PathBuf) -> Result<Store, String> {
    match tokio::fs::read_to_string(&new_path).await {
        Ok(contents) => {
            parse(&contents).map_err(|error| format!("解析 {} 失败: {error}", new_path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => load_from(legacy_path).await,
        Err(error) => Err(format!("读取 {} 失败: {error}", new_path.display())),
    }
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

/// 解析数据文件内容；兼容旧版本纯任务数组格式。
fn parse(contents: &str) -> Result<Store, String> {
    match serde_json::from_str::<Store>(contents) {
        Ok(store) => Ok(store),
        // 旧版本：文件是纯任务数组（无项目），自动迁移为空项目列表
        Err(_) => serde_json::from_str::<Vec<Todo>>(contents)
            .map(|todos| Store {
                todos,
                ..Default::default()
            })
            .map_err(|error| error.to_string()),
    }
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
    async fn legacy_store_without_sort_modes_loads() {
        // 旧版 Store：无 sort_mode / project_sort_mode 字段，自动取默认「综合」
        let path = temp_path("legacy-sort.json");
        let json = r#"{
            "todos": [],
            "projects": []
        }"#;
        save_to(path.clone(), json.into()).await.unwrap();

        let loaded = load_from(path.clone()).await.unwrap();

        assert_eq!(loaded.sort_mode, SortMode::default()); // Combined
        assert_eq!(loaded.project_sort_mode, SortMode::default());

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn legacy_store_projects_without_times_load() {
        // 旧版 Store：项目无 started_at / finished_at 字段，自动落空
        let path = temp_path("legacy-project.json");
        let json = r#"{
            "todos": [],
            "projects": [
                { "id": "0195c7e0-0000-7000-8000-000000000001", "name": "旧项目", "created_at": "2026-01-01T10:00:00Z" }
            ]
        }"#;
        save_to(path.clone(), json.into()).await.unwrap();

        let loaded = load_from(path.clone()).await.unwrap();

        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "旧项目");
        assert_eq!(loaded.projects[0].started_at, None); // 缺省字段安全落空
        assert_eq!(loaded.projects[0].finished_at, None);

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn legacy_todos_array_migrates() {
        // 旧版数据文件：纯任务数组（无 project_id / projects 字段）
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

        let loaded = load_from(path.clone()).await.unwrap();

        assert_eq!(loaded.todos.len(), 1);
        assert_eq!(loaded.todos[0].title, "旧任务");
        assert_eq!(loaded.todos[0].description, ""); // 缺省字段安全落空
        assert_eq!(loaded.todos[0].project_id, None); // 缺省字段安全落空
        assert_eq!(loaded.todos[0].due_at, None); // 缺省字段安全落空
        assert!(loaded.projects.is_empty()); // 迁移后无项目

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn new_path_missing_falls_back_to_legacy_path() {
        // 应用重命名前的旧目录数据：新路径缺失时自动回退加载（一次性迁移）
        let new_path = temp_path("rename-new.json");
        let legacy_path = temp_path("rename-legacy.json");
        let _ = tokio::fs::remove_file(&new_path).await; // 确保新路径不存在
        let legacy = r#"[
            {
                "id": "0195c7e0-0000-7000-8000-000000000001",
                "title": "旧数据",
                "created_at": "2026-01-01T10:00:00Z",
                "started_at": null,
                "finished_at": null
            }
        ]"#;
        save_to(legacy_path.clone(), legacy.into()).await.unwrap();

        let loaded = load_with_fallback(new_path, legacy_path.clone())
            .await
            .unwrap();

        assert_eq!(loaded.todos.len(), 1);
        assert_eq!(loaded.todos[0].title, "旧数据");
        assert_eq!(loaded.todos[0].description, ""); // 缺省字段安全落空

        let _ = tokio::fs::remove_file(&legacy_path).await;
    }

    #[tokio::test]
    async fn new_path_takes_precedence_over_legacy_path() {
        // 新路径已有数据时优先读取新路径，不再回退旧路径
        let new_path = temp_path("rename-new2.json");
        let legacy_path = temp_path("rename-legacy2.json");
        let store = Store {
            todos: sample_todos(),
            ..Default::default()
        };
        save_to(
            new_path.clone(),
            serde_json::to_string_pretty(&store).unwrap(),
        )
        .await
        .unwrap();
        save_to(legacy_path.clone(), "[]".into()).await.unwrap();

        let loaded = load_with_fallback(new_path.clone(), legacy_path.clone())
            .await
            .unwrap();

        assert_eq!(loaded.todos.len(), 2);
        assert_eq!(loaded.todos[0].title, "读书"); // 来自新路径

        let _ = tokio::fs::remove_file(&new_path).await;
        let _ = tokio::fs::remove_file(&legacy_path).await;
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
