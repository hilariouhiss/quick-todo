//! 持久化：任务列表以 JSON 文件存放在系统应用数据目录。
//!
//! Windows 下默认路径为 `%APPDATA%\iced-todos\todos.json`。
//! 文件缺失视为空列表；损坏文件返回错误（由 UI 提示，不崩溃）。

use std::path::PathBuf;

use crate::model::Todo;

/// 数据文件路径（若无法确定系统目录，退化为当前目录下的 todos.json）。
pub fn data_file() -> PathBuf {
    directories::ProjectDirs::from("dev", "iced-demo", "iced-todos")
        .map(|dirs| dirs.data_dir().join("todos.json"))
        .unwrap_or_else(|| PathBuf::from("todos.json"))
}

/// 从默认数据文件加载任务列表。
pub async fn load() -> Result<Vec<Todo>, String> {
    load_from(data_file()).await
}

/// 把任务列表的 JSON 写入默认数据文件。
pub async fn save(json: String) -> Result<(), String> {
    save_to(data_file(), json).await
}

/// 从指定路径加载（供测试复用）。
pub async fn load_from(path: PathBuf) -> Result<Vec<Todo>, String> {
    match tokio::fs::read_to_string(&path).await {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| format!("解析 {} 失败: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("读取 {} 失败: {error}", path.display())),
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
    use chrono::Utc;
    use uuid::Uuid;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("iced-todos-test-{}-{name}", std::process::id()))
    }

    #[tokio::test]
    async fn save_then_load_roundtrip() {
        let path = temp_path("roundtrip.json");
        let todos = vec![
            Todo {
                id: Uuid::now_v7(),
                title: "读书".into(),
                created_at: Utc::now(),
                started_at: None,
                finished_at: None,
            },
            Todo {
                id: Uuid::now_v7(),
                title: "跑步".into(),
                created_at: Utc::now(),
                started_at: Some(Utc::now()),
                finished_at: Some(Utc::now()),
            },
        ];

        save_to(path.clone(), serde_json::to_string(&todos).unwrap())
            .await
            .unwrap();

        let loaded = load_from(path.clone()).await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "读书");
        assert_eq!(loaded[0].id, todos[0].id);
        assert_eq!(loaded[0].created_at, todos[0].created_at);
        assert_eq!(loaded[1].title, "跑步");
        assert!(loaded[1].started_at.is_some());
        assert!(loaded[1].finished_at.is_some());

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn missing_file_loads_empty() {
        let path = temp_path("missing.json");
        let _ = tokio::fs::remove_file(&path).await; // 确保不存在
        let loaded = load_from(path).await.unwrap();
        assert!(loaded.is_empty());
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
