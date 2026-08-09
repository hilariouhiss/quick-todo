//! 消息定义与状态更新逻辑（iced 的 update 层）。
//!
//! update 是纯函数：`(&mut App, Message) -> Task<Message>`。
//! 除持久化之外的所有副作用（时间戳、状态流转）都直接发生在状态上，
//! 异步写盘通过返回的 `Task` 交给 iced 运行时执行。

use chrono::{DateTime, Utc};
use iced::Task;
use uuid::Uuid;

use crate::model::{App, Project, Todo, TodoStatus};
use crate::storage::{self, Store};

/// 应用内所有可触发的消息。
#[derive(Debug, Clone)]
pub enum Message {
    /// 输入框内容变化
    InputChanged(String),
    /// 描述输入框内容变化
    DescriptionInputChanged(String),
    /// 添加任务（回车或点击"添加"）
    AddTodo,
    /// 开始任务：记录开始时间
    StartTodo(Uuid),
    /// 完成任务：记录结束时间
    FinishTodo(Uuid),
    /// 删除任务
    DeleteTodo(Uuid),
    /// 启动时异步加载完成
    Loaded(Result<Store, String>),
    /// 一次异步保存完成
    Saved(Result<(), String>),
    /// 每秒时钟（携带当前 UTC 时间，用于实时耗时显示）
    Tick(DateTime<Utc>),
    /// 新建项目输入框内容变化
    ProjectInputChanged(String),
    /// 添加项目（回车或点击"添加"）
    AddProject,
    /// 开始重命名项目：进入编辑态并预填当前名称
    StartRenameProject(Uuid),
    /// 重命名输入框内容变化
    ProjectRenameChanged(String),
    /// 保存重命名（校验通过后提交并退出编辑态）
    SaveRenameProject,
    /// 取消重命名：退出编辑态
    CancelRenameProject,
    /// 删除项目（其下任务自动解除归属）
    DeleteProject(Uuid),
    /// 选中项目筛选（`None` = 全部）
    SelectProject(Option<Uuid>),
    /// 设置任务的归属项目（`None` = 解除归属）
    AssignProject {
        todo_id: Uuid,
        project_id: Option<Uuid>,
    },
    /// 收起 / 展开项目侧边栏（纯 UI 状态，不触发落盘）
    ToggleSidebar,
}

/// 处理消息，更新应用状态；必要时返回副作用任务（异步落盘）。
pub fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::InputChanged(text) => app.input = text,
        Message::DescriptionInputChanged(text) => app.description_input = text,

        Message::AddTodo => {
            let title = app.input.trim().to_owned();
            if !title.is_empty() {
                // 创建时立即记录创建时间与描述
                let description = app.description_input.trim().to_owned();
                app.todos.insert(0, Todo::new(title, description, app.now));
                app.input.clear();
                app.description_input.clear();
                return persist(app);
            }
        }

        Message::StartTodo(id) => {
            if let Some(todo) = app.todos.iter_mut().find(|todo| todo.id == id) {
                // 只有"未开始"的任务可以开始
                if todo.status() == TodoStatus::Pending {
                    todo.started_at = Some(app.now);
                    return persist(app);
                }
            }
        }

        Message::FinishTodo(id) => {
            if let Some(todo) = app.todos.iter_mut().find(|todo| todo.id == id) {
                // 只有"进行中"的任务可以完成
                if todo.status() == TodoStatus::InProgress {
                    todo.finished_at = Some(app.now);
                    return persist(app);
                }
            }
        }

        Message::DeleteTodo(id) => {
            let before = app.todos.len();
            app.todos.retain(|todo| todo.id != id);
            if app.todos.len() != before {
                return persist(app);
            }
        }

        Message::Loaded(Ok(store)) => {
            app.todos = store.todos;
            app.projects = store.projects;
        }
        Message::Loaded(Err(error)) => app.error = Some(format!("加载数据失败: {error}")),
        Message::Saved(Ok(())) => {}
        Message::Saved(Err(error)) => app.error = Some(format!("保存数据失败: {error}")),
        Message::Tick(now) => app.now = now,

        Message::ProjectInputChanged(text) => app.project_input = text,

        Message::AddProject => {
            let name = app.project_input.trim().to_owned();
            if !name.is_empty() && !app.projects.iter().any(|p| p.name == name) {
                app.projects.push(Project::new(name, app.now));
                app.project_input.clear();
                return persist(app);
            }
        }

        Message::StartRenameProject(id) => {
            if let Some(project) = app.projects.iter().find(|p| p.id == id) {
                app.editing_project = Some(id);
                app.project_edit_input = project.name.clone();
            }
        }

        Message::ProjectRenameChanged(text) => app.project_edit_input = text,

        Message::SaveRenameProject => {
            let Some(id) = app.editing_project else {
                return Task::none();
            };
            let name = app.project_edit_input.trim().to_owned();
            let valid =
                !name.is_empty() && !app.projects.iter().any(|p| p.id != id && p.name == name);
            match app.projects.iter_mut().find(|p| p.id == id) {
                // 名称非法（空 / 与其他项目重名）：保持编辑态，等待用户修改
                Some(project) if valid => {
                    project.name = name;
                    app.editing_project = None;
                    return persist(app);
                }
                Some(_) => {}
                // 项目已被删除：退出编辑态
                None => app.editing_project = None,
            }
        }

        Message::CancelRenameProject => {
            app.editing_project = None;
            app.project_edit_input.clear();
        }

        Message::DeleteProject(id) => {
            let before = app.projects.len();
            app.projects.retain(|project| project.id != id);
            if app.projects.len() != before {
                // 其下任务解除归属（任务本身保留）
                for todo in app.todos.iter_mut() {
                    if todo.project_id == Some(id) {
                        todo.project_id = None;
                    }
                }
                // 若被删项目正被筛选或编辑，同步复位
                if app.selected_project == Some(id) {
                    app.selected_project = None;
                }
                if app.editing_project == Some(id) {
                    app.editing_project = None;
                    app.project_edit_input.clear();
                }
                return persist(app);
            }
        }

        Message::SelectProject(selection) => app.selected_project = selection,

        Message::ToggleSidebar => app.sidebar_visible = !app.sidebar_visible,

        Message::AssignProject {
            todo_id,
            project_id,
        } => {
            // 防御：项目必须存在（已被删除的项目不可再被选中）
            if !project_id.is_none_or(|id| app.projects.iter().any(|p| p.id == id)) {
                return Task::none();
            }
            if let Some(todo) = app.todos.iter_mut().find(|todo| todo.id == todo_id)
                && todo.project_id != project_id
            {
                todo.project_id = project_id;
                return persist(app);
            }
        }
    }

    Task::none()
}

/// 把当前任务与项目序列化并异步写入磁盘（fire-and-forget）。
fn persist(app: &App) -> Task<Message> {
    let store = Store {
        todos: app.todos.clone(),
        projects: app.projects.clone(),
    };
    match serde_json::to_string_pretty(&store) {
        Ok(json) => Task::perform(storage::save(json), Message::Saved),
        Err(error) => {
            eprintln!("序列化数据失败: {error}");
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

    fn add_todo(app: &mut App, title: &str) -> Uuid {
        app.input = title.into();
        let _ = update(app, Message::AddTodo);
        app.todos[0].id
    }
    fn add_project(app: &mut App, name: &str) -> Uuid {
        app.project_input = name.into();
        let _ = update(app, Message::AddProject);
        app.projects.last().unwrap().id
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
        let mut app = App {
            input: "   ".into(),
            ..App::default()
        };

        let _ = update(&mut app, Message::AddTodo);

        assert!(app.todos.is_empty());
        assert!(!app.input.is_empty()); // 输入内容保留，便于用户修改
    }

    #[test]
    fn add_todo_with_description_trims_and_clears() {
        let now = Utc::now();
        let mut app = app_with(now);
        app.input = "写周报".into();
        app.description_input = "  整理本周数据  ".into();

        let _ = update(&mut app, Message::AddTodo);

        assert_eq!(app.todos[0].title, "写周报");
        assert_eq!(app.todos[0].description, "整理本周数据"); // trim 后存储
        assert!(app.input.is_empty());
        assert!(app.description_input.is_empty());

        // 空描述同样可以创建
        app.input = "无描述任务".into();
        let _ = update(&mut app, Message::AddTodo);
        assert_eq!(app.todos[0].description, "");
    }

    #[test]
    fn blank_title_ignored_even_with_description() {
        let mut app = App {
            input: "   ".into(),
            description_input: "有描述但标题为空".into(),
            ..App::default()
        };

        let _ = update(&mut app, Message::AddTodo);

        assert!(app.todos.is_empty());
        assert!(!app.input.is_empty()); // 标题输入保留
        assert_eq!(app.description_input, "有描述但标题为空"); // 描述输入保留
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
        let id = add_todo(&mut app, "写代码");

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
        add_todo(&mut app, "任务 A");
        let id = add_todo(&mut app, "任务 B");

        let _ = update(&mut app, Message::DeleteTodo(id));

        assert_eq!(app.todos.len(), 1);
        assert!(!app.todos.iter().any(|todo| todo.id == id));
    }

    // ---------- 项目 ----------

    #[test]
    fn add_project_trims_and_clears_input() {
        let mut app = app_with(Utc::now());
        app.project_input = "  工作  ".into();

        let _ = update(&mut app, Message::AddProject);

        assert_eq!(app.projects.len(), 1);
        assert_eq!(app.projects[0].name, "工作"); // 自动去除首尾空白
        assert!(app.project_input.is_empty());
    }

    #[test]
    fn blank_project_name_is_ignored() {
        let mut app = App {
            project_input: "   ".into(),
            ..App::default()
        };

        let _ = update(&mut app, Message::AddProject);

        assert!(app.projects.is_empty());
        assert!(!app.project_input.is_empty()); // 输入保留，便于修改
    }

    #[test]
    fn duplicate_project_name_is_ignored() {
        let mut app = App::default();
        add_project(&mut app, "工作");

        app.project_input = "工作".into();
        let _ = update(&mut app, Message::AddProject);

        assert_eq!(app.projects.len(), 1);
    }

    #[test]
    fn rename_project_prefills_and_commits() {
        let mut app = App::default();
        let id = add_project(&mut app, "工作");

        let _ = update(&mut app, Message::StartRenameProject(id));
        assert_eq!(app.editing_project, Some(id));
        assert_eq!(app.project_edit_input, "工作"); // 预填当前名称

        app.project_edit_input = " 个人  ".into();
        let _ = update(&mut app, Message::SaveRenameProject);

        assert_eq!(app.projects[0].name, "个人"); // trim 后提交
        assert_eq!(app.editing_project, None);
    }

    #[test]
    fn rename_to_blank_or_duplicate_keeps_editing() {
        let mut app = App::default();
        let id = add_project(&mut app, "工作");
        add_project(&mut app, "生活");

        // 空名称
        let _ = update(&mut app, Message::StartRenameProject(id));
        app.project_edit_input = "   ".into();
        let _ = update(&mut app, Message::SaveRenameProject);
        assert_eq!(app.projects[0].name, "工作");
        assert_eq!(app.editing_project, Some(id)); // 保持编辑态

        // 与其他项目重名
        app.project_edit_input = "生活".into();
        let _ = update(&mut app, Message::SaveRenameProject);
        assert_eq!(app.projects[0].name, "工作");
        assert_eq!(app.editing_project, Some(id)); // 保持编辑态
    }

    #[test]
    fn cancel_rename_exits_editing() {
        let mut app = App::default();
        let id = add_project(&mut app, "工作");

        let _ = update(&mut app, Message::StartRenameProject(id));
        app.project_edit_input = "改到一半".into();
        let _ = update(&mut app, Message::CancelRenameProject);

        assert_eq!(app.editing_project, None);
        assert!(app.project_edit_input.is_empty());
        assert_eq!(app.projects[0].name, "工作"); // 名称未变
    }

    #[test]
    fn delete_project_unassigns_todos_and_resets_selection() {
        let mut app = App::default();
        let pid = add_project(&mut app, "工作");
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(
            &mut app,
            Message::AssignProject {
                todo_id,
                project_id: Some(pid),
            },
        );
        let _ = update(&mut app, Message::SelectProject(Some(pid)));
        let _ = update(&mut app, Message::StartRenameProject(pid));
        assert_eq!(app.todos[0].project_id, Some(pid));

        let _ = update(&mut app, Message::DeleteProject(pid));

        assert!(app.projects.is_empty());
        assert_eq!(app.todos[0].project_id, None); // 任务保留，归属解除
        assert_eq!(app.selected_project, None); // 筛选复位
        assert_eq!(app.editing_project, None); // 编辑态复位
    }

    #[test]
    fn assign_project_sets_and_clears() {
        let mut app = App::default();
        let pid = add_project(&mut app, "工作");
        let todo_id = add_todo(&mut app, "写方案");

        // 归属
        let _ = update(
            &mut app,
            Message::AssignProject {
                todo_id,
                project_id: Some(pid),
            },
        );
        assert_eq!(app.todos[0].project_id, Some(pid));

        // 解除归属
        let _ = update(
            &mut app,
            Message::AssignProject {
                todo_id,
                project_id: None,
            },
        );
        assert_eq!(app.todos[0].project_id, None);
    }

    #[test]
    fn assign_unknown_project_is_rejected() {
        let mut app = App::default();
        let todo_id = add_todo(&mut app, "写方案");

        let _ = update(
            &mut app,
            Message::AssignProject {
                todo_id,
                project_id: Some(Uuid::now_v7()),
            },
        );

        assert_eq!(app.todos[0].project_id, None);
    }

    #[test]
    fn select_project_is_pure_state() {
        let mut app = App::default();
        let pid = add_project(&mut app, "工作");

        let _ = update(&mut app, Message::SelectProject(Some(pid)));
        assert_eq!(app.selected_project, Some(pid));

        let _ = update(&mut app, Message::SelectProject(None));
        assert_eq!(app.selected_project, None);
    }

    #[test]
    fn toggle_sidebar_flips_visibility() {
        let mut app = App::default();
        assert!(!app.sidebar_visible); // 启动默认收起

        let _ = update(&mut app, Message::ToggleSidebar);
        assert!(app.sidebar_visible);

        let _ = update(&mut app, Message::ToggleSidebar);
        assert!(!app.sidebar_visible);
    }

    #[test]
    fn loaded_populates_todos_and_projects() {
        let mut app = App::default();
        let store = Store {
            todos: vec![Todo::new("任务".into(), "描述".into(), Utc::now())],
            projects: vec![Project::new("工作".into(), Utc::now())],
        };

        let _ = update(&mut app, Message::Loaded(Ok(store)));

        assert_eq!(app.todos.len(), 1);
        assert_eq!(app.projects.len(), 1);
    }

    #[test]
    fn loaded_error_sets_hint() {
        let mut app = App::default();
        let _ = update(&mut app, Message::Loaded(Err("磁盘错误".into())));
        assert!(app.error.is_some());
    }
}
