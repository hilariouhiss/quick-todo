//! 消息定义与状态更新逻辑（iced 的 update 层）。
//!
//! update 是纯函数：`(&mut App, Message) -> Task<Message>`。
//! 除持久化之外的所有副作用（时间戳、状态流转）都直接发生在状态上，
//! 异步写盘通过返回的 `Task` 交给 iced 运行时执行。

use chrono::{DateTime, Utc};
use iced::Task;
use uuid::Uuid;

use crate::model::{App, Todo, TodoStatus};
use crate::storage;

/// 应用内所有可触发的消息。
#[derive(Debug, Clone)]
pub enum Message {
    /// 输入框内容变化
    InputChanged(String),
    /// 添加任务（回车或点击"添加"）
    AddTodo,
    /// 开始任务：记录开始时间
    StartTodo(Uuid),
    /// 完成任务：记录结束时间
    FinishTodo(Uuid),
    /// 删除任务
    DeleteTodo(Uuid),
    /// 启动时异步加载完成
    Loaded(Result<Vec<Todo>, String>),
    /// 一次异步保存完成
    Saved(Result<(), String>),
    /// 每秒时钟（携带当前 UTC 时间，用于实时耗时显示）
    Tick(DateTime<Utc>),
}

/// 处理消息，更新应用状态；必要时返回副作用任务（异步落盘）。
pub fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::InputChanged(text) => app.input = text,

        Message::AddTodo => {
            let title = app.input.trim().to_owned();
            if !title.is_empty() {
                // 创建时立即记录创建时间
                app.todos.insert(0, Todo::new(title, app.now));
                app.input.clear();
                return persist(&app.todos);
            }
        }

        Message::StartTodo(id) => {
            if let Some(todo) = app.todos.iter_mut().find(|todo| todo.id == id) {
                // 只有"未开始"的任务可以开始
                if todo.status() == TodoStatus::Pending {
                    todo.started_at = Some(app.now);
                    return persist(&app.todos);
                }
            }
        }

        Message::FinishTodo(id) => {
            if let Some(todo) = app.todos.iter_mut().find(|todo| todo.id == id) {
                // 只有"进行中"的任务可以完成
                if todo.status() == TodoStatus::InProgress {
                    todo.finished_at = Some(app.now);
                    return persist(&app.todos);
                }
            }
        }

        Message::DeleteTodo(id) => {
            let before = app.todos.len();
            app.todos.retain(|todo| todo.id != id);
            if app.todos.len() != before {
                return persist(&app.todos);
            }
        }

        Message::Loaded(Ok(todos)) => app.todos = todos,
        Message::Loaded(Err(error)) => app.error = Some(format!("加载数据失败: {error}")),
        Message::Saved(Ok(())) => {}
        Message::Saved(Err(error)) => app.error = Some(format!("保存数据失败: {error}")),
        Message::Tick(now) => app.now = now,
    }

    Task::none()
}

/// 把当前任务列表序列化并异步写入磁盘（fire-and-forget）。
fn persist(todos: &[Todo]) -> Task<Message> {
    match serde_json::to_string_pretty(todos) {
        Ok(json) => Task::perform(storage::save(json), Message::Saved),
        Err(error) => {
            eprintln!("序列化任务列表失败: {error}");
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(now: DateTime<Utc>) -> App {
        App {
            now,
            ..App::default()
        }
    }

    #[test]
    fn add_todo_records_creation_time_and_clears_input() {
        let now = Utc::now();
        let mut app = app_with(now);
        app.input = "  写周报  ".into();

        let _ = update(&mut app, Message::AddTodo);

        assert_eq!(app.todos.len(), 1);
        assert_eq!(app.todos[0].title, "写周报"); // 自动去除首尾空白
        assert_eq!(app.todos[0].created_at, now); // 创建时间被记录
        assert_eq!(app.todos[0].status(), TodoStatus::Pending);
        assert!(app.input.is_empty());
    }

    #[test]
    fn blank_title_is_ignored() {
        let mut app = App::default();
        app.input = "   ".into();

        let _ = update(&mut app, Message::AddTodo);

        assert!(app.todos.is_empty());
        assert!(!app.input.is_empty()); // 输入内容保留，便于用户修改
    }

    #[test]
    fn add_puts_newest_first() {
        let mut app = App::default();
        for i in 0..3 {
            app.input = format!("任务 {i}");
            let _ = update(&mut app, Message::AddTodo);
        }
        assert_eq!(app.todos[0].title, "任务 2");
    }

    #[test]
    fn start_then_finish_records_times_in_order() {
        let now = Utc::now();
        let mut app = app_with(now);
        app.input = "写代码".into();
        let _ = update(&mut app, Message::AddTodo);
        let id = app.todos[0].id;

        // 未开始的任务不能直接"完成"
        let _ = update(&mut app, Message::FinishTodo(id));
        assert_eq!(app.todos[0].status(), TodoStatus::Pending);
        assert_eq!(app.todos[0].finished_at, None);

        // 开始：记录开始时间
        app.now = now + chrono::Duration::minutes(5);
        let _ = update(&mut app, Message::StartTodo(id));
        assert_eq!(
            app.todos[0].started_at,
            Some(now + chrono::Duration::minutes(5))
        );

        // 重复"开始"无效
        app.now = now + chrono::Duration::minutes(6);
        let _ = update(&mut app, Message::StartTodo(id));
        assert_eq!(
            app.todos[0].started_at,
            Some(now + chrono::Duration::minutes(5))
        );

        // 完成：记录结束时间
        app.now = now + chrono::Duration::hours(2);
        let _ = update(&mut app, Message::FinishTodo(id));
        assert_eq!(
            app.todos[0].finished_at,
            Some(now + chrono::Duration::hours(2))
        );
        assert_eq!(app.todos[0].status(), TodoStatus::Done);
    }

    #[test]
    fn delete_removes_todo() {
        let mut app = app_with(Utc::now());
        app.input = "任务 A".into();
        let _ = update(&mut app, Message::AddTodo);
        app.input = "任务 B".into();
        let _ = update(&mut app, Message::AddTodo);
        let id = app.todos[0].id;

        let _ = update(&mut app, Message::DeleteTodo(id));

        assert_eq!(app.todos.len(), 1);
        assert!(!app.todos.iter().any(|todo| todo.id == id));
    }
}
