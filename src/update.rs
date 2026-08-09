//! 消息定义与状态更新逻辑（iced 的 update 层）。
//!
//! update 是纯函数：`(&mut App, Message) -> Task<Message>`。
//! 除持久化之外的所有副作用（时间戳、状态流转）都直接发生在状态上，
//! 异步写盘通过返回的 `Task` 交给 iced 运行时执行。

use chrono::{DateTime, Utc};
use iced::Task;
use uuid::Uuid;

use crate::model::{
    AddDialog, App, Priority, Project, ProjectDialog, ProjectEdit, QuickDue, SortMode, Todo,
    TodoEdit, TodoStatus, format_due, parse_datetime,
};
use crate::storage::{self, Store};
use crate::view::{DIALOG_TITLE_ID, PROJECT_DIALOG_NAME_ID, PROJECT_EDIT_NAME_ID};

/// 应用内所有可触发的消息。
#[derive(Debug, Clone)]
pub enum Message {
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
    /// 任务排序方式切换（持久化偏好，触发落盘）
    SortModeChanged(SortMode),
    /// 项目排序方式切换（持久化偏好，触发落盘）
    ProjectSortModeChanged(SortMode),
    /// 打开弹窗添加项目（名称 + 优先级 + 可选起止时间）
    OpenProjectDialog,
    /// 关闭弹窗添加项目（丢弃已填内容，不落盘）
    CloseProjectDialog,
    /// 弹窗：名称输入框变化
    ProjectNameChanged(String),
    /// 弹窗：开始时间输入框变化（实时解析校验）
    ProjectStartChanged(String),
    /// 弹窗：结束时间输入框变化（实时解析校验）
    ProjectEndChanged(String),
    /// 弹窗：优先级下拉选择
    ProjectDialogPriorityChanged(Option<Priority>),
    /// 弹窗：点击"创建"/回车（校验通过后创建项目并落盘）
    SubmitProjectDialog,
    /// 打开已完成归档弹窗（纯 UI 状态，不落盘）
    OpenCompletedDialog,
    /// 关闭已完成归档弹窗（纯 UI 状态，不落盘）
    CloseCompletedDialog,
    /// 开始编辑项目：进入侧边栏内联编辑态并预填名称与起止时间
    StartEditProject(Uuid),
    /// 编辑：名称输入框变化
    ProjectEditNameChanged(String),
    /// 编辑：开始时间输入框变化（实时解析校验）
    ProjectEditStartChanged(String),
    /// 编辑：结束时间输入框变化（实时解析校验）
    ProjectEditEndChanged(String),
    /// 编辑：优先级下拉选择
    ProjectEditPriorityChanged(Option<Priority>),
    /// 保存项目编辑（校验通过后就地更新并落盘）
    SaveEditProject,
    /// 取消项目编辑：退出编辑态
    CancelEditProject,
    /// 删除项目（其下任务自动解除归属）
    DeleteProject(Uuid),
    /// 选中项目筛选（`None` = 全部）
    SelectProject(Option<Uuid>),
    /// 进入卡片编辑模式（预填当前字段；该卡片即"当前任务"）
    EditTodo(Uuid),
    /// 退出卡片编辑模式（丢弃修改，不落盘）
    CancelEditTodo,
    /// 编辑：标题输入框变化
    EditTitleChanged(String),
    /// 编辑：描述输入框变化
    EditDescriptionChanged(String),
    /// 编辑：项目下拉选择
    EditProjectChanged(Option<Uuid>),
    /// 编辑：优先级下拉选择
    EditPriorityChanged(Option<Priority>),
    /// 编辑：截止时间输入框变化（实时解析校验）
    EditDueChanged(String),
    /// 编辑：快捷时间下拉选择（回填到截止时间输入框）
    EditQuickDue(QuickDue),
    /// 编辑：点击"保存"/回车（校验通过后更新任务并落盘）
    SaveEditTodo,
    /// 收起 / 展开项目侧边栏（纯 UI 状态，不触发落盘）
    ToggleSidebar,
    /// 打开弹窗添加任务（唯一添加入口，预选当前筛选的项目）
    OpenAddDialog,
    /// 关闭弹窗添加任务（丢弃已填内容，不落盘）
    CloseAddDialog,
    /// 关闭当前打开的弹窗（任务 / 项目 / 已完成归档；点击遮罩 / Esc 触发）
    CloseActiveDialog,
    /// 弹窗：标题输入框变化
    DialogTitleChanged(String),
    /// 弹窗：描述输入框变化
    DialogDescriptionChanged(String),
    /// 弹窗：项目下拉选择
    DialogProjectChanged(Option<Uuid>),
    /// 弹窗：优先级下拉选择
    DialogPriorityChanged(Option<Priority>),
    /// 弹窗：截止时间输入框变化（实时解析校验）
    DialogDueChanged(String),
    /// 弹窗：快捷时间下拉选择（回填到截止时间输入框）
    DialogQuickDue(QuickDue),
    /// 弹窗：点击"创建"/回车提交（校验通过后创建任务并落盘）
    SubmitAddDialog,
}

/// 处理消息，更新应用状态；必要时返回副作用任务（异步落盘）。
pub fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
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
            // 恢复持久化的排序偏好（旧文件缺省已在反序列化时取「综合」）
            app.sort_mode = store.sort_mode;
            app.project_sort_mode = store.project_sort_mode;
        }
        Message::Loaded(Err(error)) => app.error = Some(format!("加载数据失败: {error}")),
        Message::Saved(Ok(())) => {}
        Message::Saved(Err(error)) => app.error = Some(format!("保存数据失败: {error}")),
        Message::Tick(now) => app.now = now,

        Message::SortModeChanged(mode) => {
            // 排序偏好持久化：切换即落盘
            app.sort_mode = mode;
            return persist(app);
        }

        Message::ProjectSortModeChanged(mode) => {
            // 排序偏好持久化：切换即落盘
            app.project_sort_mode = mode;
            return persist(app);
        }

        Message::OpenProjectDialog => {
            // 弹窗互斥：打开项目弹窗时关闭任务弹窗与归档弹窗
            app.add_dialog = None;
            app.show_completed = false;
            app.project_dialog = Some(ProjectDialog::default());
            // 聚焦弹窗名称输入框（下一次渲染生效）
            return iced::widget::operation::focus(PROJECT_DIALOG_NAME_ID);
        }

        Message::CloseProjectDialog => {
            // 关闭弹窗：丢弃已填内容（弹窗表单是纯内存状态，不落盘）
            app.project_dialog = None;
        }

        Message::ProjectNameChanged(text) => {
            if let Some(dialog) = &mut app.project_dialog {
                dialog.name = text;
            }
        }

        Message::ProjectStartChanged(text) => {
            if let Some(dialog) = &mut app.project_dialog {
                // 实时解析：非法格式立即提示（start_parsed 缓存结果）
                dialog.start_parsed = parse_datetime(&text);
                dialog.start_input = text;
            }
        }

        Message::ProjectEndChanged(text) => {
            if let Some(dialog) = &mut app.project_dialog {
                // 实时解析：非法格式立即提示（end_parsed 缓存结果）
                dialog.end_parsed = parse_datetime(&text);
                dialog.end_input = text;
            }
        }

        Message::ProjectDialogPriorityChanged(priority) => {
            if let Some(dialog) = &mut app.project_dialog {
                dialog.priority = priority;
            }
        }

        Message::SubmitProjectDialog => {
            // 校验不通过时恢复弹窗（保留用户输入），不产生任何副作用
            let Some(dialog) = app.project_dialog.take() else {
                return Task::none();
            };
            let restore = |app: &mut App| app.project_dialog = Some(dialog.clone());

            let name = dialog.name.trim().to_owned();
            if name.is_empty() || app.projects.iter().any(|p| p.name == name) {
                restore(app);
                return Task::none();
            }
            let started_at = match dialog.start_parsed {
                Ok(time) => time,
                Err(_) => {
                    restore(app);
                    return Task::none();
                }
            };
            let finished_at = match dialog.end_parsed {
                Ok(time) => time,
                Err(_) => {
                    restore(app);
                    return Task::none();
                }
            };
            // 起止时间同时设置时必须满足：开始早于结束
            if let (Some(start), Some(finish)) = (started_at, finished_at)
                && start >= finish
            {
                restore(app);
                return Task::none();
            }

            // 校验通过：创建项目（含优先级与起止时间、时间取自 app.now）并关闭弹窗
            let project =
                if dialog.priority.is_none() && started_at.is_none() && finished_at.is_none() {
                    Project::new(name, app.now)
                } else {
                    Project::new_full(name, dialog.priority, started_at, finished_at, app.now)
                };
            app.projects.push(project);
            app.project_dialog = None;
            return persist(app);
        }

        Message::OpenCompletedDialog => {
            // 弹窗互斥：打开归档弹窗时关闭任务 / 项目弹窗
            app.add_dialog = None;
            app.project_dialog = None;
            app.show_completed = true;
        }

        Message::CloseCompletedDialog => {
            // 关闭归档弹窗（纯 UI 状态，不落盘）
            app.show_completed = false;
        }

        Message::StartEditProject(id) => {
            if let Some(project) = app.projects.iter().find(|p| p.id == id) {
                // 预填当前字段；起止时间回填为可解析文本（分钟粒度）
                app.project_edit = Some(ProjectEdit {
                    project_id: id,
                    name: project.name.clone(),
                    priority: project.priority,
                    start_input: project.started_at.map(format_due).unwrap_or_default(),
                    start_parsed: Ok(project.started_at),
                    end_input: project.finished_at.map(format_due).unwrap_or_default(),
                    end_parsed: Ok(project.finished_at),
                });
                // 聚焦编辑名称输入框（下一次渲染生效）
                return iced::widget::operation::focus(PROJECT_EDIT_NAME_ID);
            }
        }

        Message::ProjectEditNameChanged(text) => {
            if let Some(edit) = &mut app.project_edit {
                edit.name = text;
            }
        }

        Message::ProjectEditStartChanged(text) => {
            if let Some(edit) = &mut app.project_edit {
                // 实时解析：非法格式立即提示（start_parsed 缓存结果）
                edit.start_parsed = parse_datetime(&text);
                edit.start_input = text;
            }
        }

        Message::ProjectEditEndChanged(text) => {
            if let Some(edit) = &mut app.project_edit {
                // 实时解析：非法格式立即提示（end_parsed 缓存结果）
                edit.end_parsed = parse_datetime(&text);
                edit.end_input = text;
            }
        }

        Message::ProjectEditPriorityChanged(priority) => {
            if let Some(edit) = &mut app.project_edit {
                edit.priority = priority;
            }
        }

        Message::SaveEditProject => {
            // 校验不通过时保持编辑态（保留用户输入），不产生任何副作用
            let Some(edit) = app.project_edit.take() else {
                return Task::none();
            };
            let restore = |app: &mut App| app.project_edit = Some(edit.clone());

            let name = edit.name.trim().to_owned();
            if name.is_empty()
                || app
                    .projects
                    .iter()
                    .any(|p| p.id != edit.project_id && p.name == name)
            {
                restore(app);
                return Task::none();
            }
            let started_at = match edit.start_parsed {
                Ok(time) => time,
                Err(_) => {
                    restore(app);
                    return Task::none();
                }
            };
            let finished_at = match edit.end_parsed {
                Ok(time) => time,
                Err(_) => {
                    restore(app);
                    return Task::none();
                }
            };
            // 起止时间同时设置时必须满足：开始早于结束
            if let (Some(start), Some(finish)) = (started_at, finished_at)
                && start >= finish
            {
                restore(app);
                return Task::none();
            }

            if let Some(project) = app.projects.iter_mut().find(|p| p.id == edit.project_id) {
                // 校验通过：就地更新名称、优先级与起止时间并退出编辑态
                project.name = name;
                project.priority = edit.priority;
                project.started_at = started_at;
                project.finished_at = finished_at;
                return persist(app);
            }
            // 项目已被删除：退出编辑态（无副作用）
        }

        Message::CancelEditProject => {
            // 退出编辑态：丢弃修改（编辑表单是纯内存状态，不落盘）
            app.project_edit = None;
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
                if app
                    .project_edit
                    .as_ref()
                    .is_some_and(|e| e.project_id == id)
                {
                    app.project_edit = None;
                }
                return persist(app);
            }
        }

        Message::SelectProject(selection) => app.selected_project = selection,

        Message::ToggleSidebar => app.sidebar_visible = !app.sidebar_visible,

        Message::OpenAddDialog => {
            // 弹窗互斥：打开任务弹窗时关闭项目弹窗与归档弹窗
            app.project_dialog = None;
            app.show_completed = false;
            // 弹窗打开：处于项目筛选时预选该项目，作为默认归属
            app.add_dialog = Some(AddDialog {
                project_id: app.selected_project,
                ..AddDialog::default()
            });
            // 聚焦弹窗标题输入框（下一次渲染生效）
            return iced::widget::operation::focus(DIALOG_TITLE_ID);
        }

        Message::CloseAddDialog => {
            // 关闭弹窗：丢弃已填内容（弹窗表单是纯内存状态，不落盘）
            app.add_dialog = None;
        }

        Message::CloseActiveDialog => {
            // 弹窗互斥，当前只可能打开一个：关闭打开的那个
            if app.add_dialog.is_some() {
                app.add_dialog = None;
            } else if app.project_dialog.is_some() {
                app.project_dialog = None;
            } else {
                app.show_completed = false;
            }
        }

        Message::DialogTitleChanged(text) => {
            if let Some(dialog) = &mut app.add_dialog {
                dialog.title = text;
            }
        }

        Message::DialogDescriptionChanged(text) => {
            if let Some(dialog) = &mut app.add_dialog {
                dialog.description = text;
            }
        }

        Message::DialogProjectChanged(project_id) => {
            // 防御：项目必须存在（已被删除的项目不可再被选中）
            if !project_id.is_none_or(|id| app.projects.iter().any(|p| p.id == id)) {
                return Task::none();
            }
            if let Some(dialog) = &mut app.add_dialog {
                dialog.project_id = project_id;
            }
        }

        Message::DialogPriorityChanged(priority) => {
            if let Some(dialog) = &mut app.add_dialog {
                dialog.priority = priority;
            }
        }

        Message::DialogDueChanged(text) => {
            if let Some(dialog) = &mut app.add_dialog {
                // 实时解析：非法格式立即提示（due_parsed 缓存结果）
                dialog.due_parsed = parse_datetime(&text);
                dialog.due_input = text;
            }
        }

        Message::DialogQuickDue(quick) => {
            if let Some(dialog) = &mut app.add_dialog {
                // 快捷时间：基于 app.now 的本地时区计算，回填文本后走统一解析
                let text = quick.due_text(app.now);
                dialog.due_parsed = parse_datetime(&text);
                dialog.due_input = text;
            }
        }

        Message::SubmitAddDialog => {
            // 校验不通过时恢复弹窗（保留用户输入），不产生任何副作用
            let Some(dialog) = app.add_dialog.take() else {
                return Task::none();
            };
            let restore = |app: &mut App| app.add_dialog = Some(dialog.clone());

            let title = dialog.title.trim().to_owned();
            if title.is_empty() {
                restore(app);
                return Task::none();
            }
            let due_at = match dialog.due_parsed {
                Ok(due) => due,
                Err(_) => {
                    restore(app);
                    return Task::none();
                }
            };
            if !dialog
                .project_id
                .is_none_or(|id| app.projects.iter().any(|p| p.id == id))
            {
                restore(app);
                return Task::none();
            }

            // 校验通过：创建任务（插最前、时间取自 app.now）并关闭弹窗
            let description = dialog.description.trim().to_owned();
            let todo = if description.is_empty()
                && dialog.priority.is_none()
                && dialog.project_id.is_none()
                && due_at.is_none()
            {
                // 纯标题场景
                Todo::new(title, app.now)
            } else {
                Todo::new_full(
                    title,
                    description,
                    dialog.priority,
                    dialog.project_id,
                    due_at,
                    app.now,
                )
            };
            app.todos.insert(0, todo);
            app.add_dialog = None;
            return persist(app);
        }

        Message::EditTodo(id) => {
            if let Some(todo) = app.todos.iter().find(|todo| todo.id == id) {
                // 预填当前字段；截止时间回填为可解析文本（分钟粒度）
                app.todo_edit = Some(TodoEdit {
                    todo_id: id,
                    title: todo.title.clone(),
                    description: todo.description.clone(),
                    priority: todo.priority,
                    project_id: todo.project_id,
                    due_input: todo.due_at.map(format_due).unwrap_or_default(),
                    due_parsed: Ok(todo.due_at),
                });
            }
        }

        Message::CancelEditTodo => {
            // 退出编辑模式：丢弃修改（编辑表单是纯内存状态，不落盘）
            app.todo_edit = None;
        }

        Message::EditTitleChanged(text) => {
            if let Some(edit) = &mut app.todo_edit {
                edit.title = text;
            }
        }

        Message::EditDescriptionChanged(text) => {
            if let Some(edit) = &mut app.todo_edit {
                edit.description = text;
            }
        }

        Message::EditProjectChanged(project_id) => {
            // 防御：项目必须存在（已被删除的项目不可再被选中）
            if !project_id.is_none_or(|id| app.projects.iter().any(|p| p.id == id)) {
                return Task::none();
            }
            if let Some(edit) = &mut app.todo_edit {
                edit.project_id = project_id;
            }
        }

        Message::EditPriorityChanged(priority) => {
            if let Some(edit) = &mut app.todo_edit {
                edit.priority = priority;
            }
        }

        Message::EditDueChanged(text) => {
            if let Some(edit) = &mut app.todo_edit {
                // 实时解析：非法格式立即提示（due_parsed 缓存结果）
                edit.due_parsed = parse_datetime(&text);
                edit.due_input = text;
            }
        }

        Message::EditQuickDue(quick) => {
            if let Some(edit) = &mut app.todo_edit {
                // 快捷时间：基于 app.now 的本地时区计算，回填文本后走统一解析
                let text = quick.due_text(app.now);
                edit.due_parsed = parse_datetime(&text);
                edit.due_input = text;
            }
        }

        Message::SaveEditTodo => {
            // 校验不通过时保持编辑态（保留用户输入），不产生任何副作用
            let Some(edit) = app.todo_edit.take() else {
                return Task::none();
            };
            let restore = |app: &mut App| app.todo_edit = Some(edit.clone());

            let title = edit.title.trim().to_owned();
            if title.is_empty() {
                restore(app);
                return Task::none();
            }
            let due_at = match edit.due_parsed {
                Ok(due) => due,
                Err(_) => {
                    restore(app);
                    return Task::none();
                }
            };
            if !edit
                .project_id
                .is_none_or(|id| app.projects.iter().any(|p| p.id == id))
            {
                restore(app);
                return Task::none();
            }

            // 校验通过：更新任务（trim 后存储）并退出编辑模式
            let Some(todo) = app.todos.iter_mut().find(|todo| todo.id == edit.todo_id) else {
                // 任务已被删除：直接退出编辑模式
                return Task::none();
            };
            todo.title = title;
            todo.description = edit.description.trim().to_owned();
            todo.priority = edit.priority;
            todo.project_id = edit.project_id;
            todo.due_at = due_at;
            return persist(app);
        }
    }

    Task::none()
}

/// 把当前任务、项目与排序偏好序列化并异步写入磁盘（fire-and-forget）。
fn persist(app: &App) -> Task<Message> {
    let store = Store {
        todos: app.todos.clone(),
        projects: app.projects.clone(),
        sort_mode: app.sort_mode,
        project_sort_mode: app.project_sort_mode,
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

    /// 通过任务弹窗添加任务（唯一添加入口）并返回其 id
    fn add_todo(app: &mut App, title: &str) -> Uuid {
        let _ = update(app, Message::OpenAddDialog);
        let _ = update(app, Message::DialogTitleChanged(title.into()));
        let _ = update(app, Message::SubmitAddDialog);
        app.todos[0].id
    }
    /// 直接构造项目（弹窗创建路径由 submit_project_dialog 系列测试覆盖）
    fn add_project(app: &mut App, name: &str) -> Uuid {
        app.projects.push(Project::new(name.into(), app.now));
        app.projects.last().unwrap().id
    }

    #[test]
    fn add_via_dialog_records_creation_time() {
        let now = Utc::now();
        let mut app = app_with(now);
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::DialogTitleChanged("  写周报  ".into()));

        let _ = update(&mut app, Message::SubmitAddDialog);

        assert_eq!(app.todos.len(), 1);
        assert_eq!(app.todos[0].title, "写周报"); // 自动去除首尾空白
        assert_eq!(app.todos[0].created_at, now); // 创建时间被记录
        assert_eq!(app.todos[0].description, "");
        assert_eq!(app.todos[0].status(), TodoStatus::Pending);
        assert!(app.add_dialog.is_none()); // 提交后弹窗关闭
    }

    #[test]
    fn add_puts_newest_first() {
        let mut app = App::default();
        for i in 0..3 {
            add_todo(&mut app, &format!("任务 {i}"));
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

    // ---------- 弹窗添加项目 ----------

    #[test]
    fn open_project_dialog_initializes_form() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenProjectDialog);

        let dialog = app.project_dialog.as_ref().unwrap();
        assert!(dialog.name.is_empty());
        assert!(dialog.start_parsed.is_ok());
        assert!(dialog.end_parsed.is_ok());
    }

    #[test]
    fn project_dialogs_are_mutually_exclusive() {
        let mut app = App::default();
        // 打开任务弹窗后打开项目弹窗：任务弹窗被关闭
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::OpenProjectDialog);
        assert!(app.project_dialog.is_some());
        assert!(app.add_dialog.is_none());

        // 再打开任务弹窗：项目弹窗被关闭
        let _ = update(&mut app, Message::OpenAddDialog);
        assert!(app.add_dialog.is_some());
        assert!(app.project_dialog.is_none());
    }

    #[test]
    fn completed_dialog_is_mutually_exclusive() {
        let mut app = App::default();
        // 打开归档弹窗：关闭任务 / 项目弹窗
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::OpenCompletedDialog);
        assert!(app.show_completed);
        assert!(app.add_dialog.is_none());

        // 打开项目弹窗：关闭归档弹窗
        let _ = update(&mut app, Message::OpenProjectDialog);
        assert!(app.project_dialog.is_some());
        assert!(!app.show_completed);

        // 打开任务弹窗：关闭项目弹窗
        let _ = update(&mut app, Message::OpenAddDialog);
        assert!(app.add_dialog.is_some());
        assert!(app.project_dialog.is_none());
        assert!(!app.show_completed);
    }

    #[test]
    fn close_completed_dialog() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenCompletedDialog);
        assert!(app.show_completed);

        let _ = update(&mut app, Message::CloseCompletedDialog);
        assert!(!app.show_completed);
    }

    #[test]
    fn close_project_dialog_discards_input() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenProjectDialog);
        let _ = update(&mut app, Message::ProjectNameChanged("工作".into()));

        let _ = update(&mut app, Message::CloseProjectDialog);

        assert!(app.project_dialog.is_none());
        assert!(app.projects.is_empty()); // 未创建任何项目
    }

    #[test]
    fn close_active_dialog_closes_whichever_open() {
        let mut app = App::default();

        // 任务弹窗打开时关闭任务弹窗
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::CloseActiveDialog);
        assert!(app.add_dialog.is_none());

        // 项目弹窗打开时关闭项目弹窗
        let _ = update(&mut app, Message::OpenProjectDialog);
        let _ = update(&mut app, Message::CloseActiveDialog);
        assert!(app.project_dialog.is_none());

        // 归档弹窗打开时关闭归档弹窗
        let _ = update(&mut app, Message::OpenCompletedDialog);
        let _ = update(&mut app, Message::CloseActiveDialog);
        assert!(!app.show_completed);

        // 无弹窗打开时无副作用
        let _ = update(&mut app, Message::CloseActiveDialog);
        assert!(app.add_dialog.is_none());
        assert!(app.project_dialog.is_none());
        assert!(!app.show_completed);
    }

    #[test]
    fn project_dialog_inputs_update_form() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenProjectDialog);

        let _ = update(&mut app, Message::ProjectNameChanged("工作".into()));
        let _ = update(&mut app, Message::ProjectStartChanged("2026-01-01".into()));
        let _ = update(
            &mut app,
            Message::ProjectEndChanged("2026-01-31 18:30".into()),
        );

        let dialog = app.project_dialog.as_ref().unwrap();
        assert_eq!(dialog.name, "工作");
        assert!(dialog.start_parsed.as_ref().unwrap().is_some());
        assert!(dialog.end_parsed.as_ref().unwrap().is_some());
    }

    #[test]
    fn project_dialog_inputs_ignored_when_closed() {
        let mut app = App::default();
        let _ = update(&mut app, Message::ProjectNameChanged("工作".into()));
        let _ = update(&mut app, Message::ProjectStartChanged("2026-01-01".into()));
        assert!(app.project_dialog.is_none());
    }

    #[test]
    fn submit_project_dialog_creates_project_with_times() {
        let now = Utc::now();
        let mut app = app_with(now);
        let _ = update(&mut app, Message::OpenProjectDialog);
        let _ = update(&mut app, Message::ProjectNameChanged("  工作  ".into()));
        let _ = update(
            &mut app,
            Message::ProjectDialogPriorityChanged(Some(Priority::High)),
        );
        let _ = update(&mut app, Message::ProjectStartChanged("2026-01-01".into()));
        let _ = update(
            &mut app,
            Message::ProjectEndChanged("2026-01-31 18:30".into()),
        );

        let _ = update(&mut app, Message::SubmitProjectDialog);

        assert!(app.project_dialog.is_none()); // 弹窗关闭
        assert_eq!(app.projects.len(), 1);
        let project = &app.projects[0];
        assert_eq!(project.name, "工作"); // trim 后存储
        assert_eq!(project.priority, Some(Priority::High));
        assert!(project.started_at.is_some());
        assert!(project.finished_at.is_some());
        assert_eq!(project.created_at, now); // 时间取自 app.now
    }

    #[test]
    fn submit_project_dialog_without_times_creates_project() {
        let now = Utc::now();
        let mut app = app_with(now);
        let _ = update(&mut app, Message::OpenProjectDialog);
        let _ = update(&mut app, Message::ProjectNameChanged("无时间项目".into()));

        let _ = update(&mut app, Message::SubmitProjectDialog);

        assert!(app.project_dialog.is_none());
        assert_eq!(app.projects[0].started_at, None);
        assert_eq!(app.projects[0].finished_at, None);
    }

    #[test]
    fn submit_project_dialog_blank_name_keeps_dialog() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenProjectDialog);
        let _ = update(&mut app, Message::ProjectNameChanged("   ".into()));

        let _ = update(&mut app, Message::SubmitProjectDialog);

        assert!(app.project_dialog.is_some()); // 弹窗保持打开
        assert_eq!(app.project_dialog.as_ref().unwrap().name, "   "); // 输入保留
        assert!(app.projects.is_empty());
    }

    #[test]
    fn submit_project_dialog_duplicate_name_keeps_dialog() {
        let mut app = App::default();
        add_project(&mut app, "工作");
        let _ = update(&mut app, Message::OpenProjectDialog);
        let _ = update(&mut app, Message::ProjectNameChanged("工作".into()));

        let _ = update(&mut app, Message::SubmitProjectDialog);

        assert!(app.project_dialog.is_some());
        assert_eq!(app.projects.len(), 1); // 未重复创建
    }

    #[test]
    fn submit_project_dialog_invalid_time_keeps_dialog() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenProjectDialog);
        let _ = update(&mut app, Message::ProjectNameChanged("工作".into()));
        let _ = update(&mut app, Message::ProjectStartChanged("后天".into()));

        let _ = update(&mut app, Message::SubmitProjectDialog);

        assert!(app.project_dialog.is_some()); // 开始时间格式非法 → 拒绝
        assert!(app.projects.is_empty());

        // 结束时间格式非法同样拒绝
        let _ = update(&mut app, Message::ProjectStartChanged("".into()));
        let _ = update(&mut app, Message::ProjectEndChanged("2026/01/31".into()));
        let _ = update(&mut app, Message::SubmitProjectDialog);
        assert!(app.project_dialog.is_some());
        assert!(app.projects.is_empty());
    }

    #[test]
    fn submit_project_dialog_start_after_end_keeps_dialog() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenProjectDialog);
        let _ = update(&mut app, Message::ProjectNameChanged("工作".into()));
        let _ = update(&mut app, Message::ProjectStartChanged("2026-02-01".into()));
        let _ = update(&mut app, Message::ProjectEndChanged("2026-01-31".into()));

        let _ = update(&mut app, Message::SubmitProjectDialog);

        assert!(app.project_dialog.is_some()); // 开始不早于结束 → 拒绝
        assert!(app.projects.is_empty());
    }

    #[test]
    fn submit_without_open_project_dialog_is_noop() {
        let mut app = App::default();
        let _ = update(&mut app, Message::SubmitProjectDialog);
        assert!(app.projects.is_empty());
    }

    #[test]
    fn start_edit_project_prefills_fields() {
        let now = Utc::now();
        let mut app = app_with(now);
        let id = add_project(&mut app, "工作");
        app.projects[0].priority = Some(Priority::High);
        app.projects[0].started_at = Some(now);
        app.projects[0].finished_at = Some(now + chrono::Duration::days(30));

        let _ = update(&mut app, Message::StartEditProject(id));

        let edit = app.project_edit.as_ref().unwrap();
        assert_eq!(edit.project_id, id);
        assert_eq!(edit.name, "工作");
        assert_eq!(edit.priority, Some(Priority::High));
        assert!(!edit.start_input.is_empty()); // 起止时间回填为可解析文本
        assert!(!edit.end_input.is_empty());
        assert!(edit.start_parsed.is_ok());
        assert!(edit.end_parsed.is_ok());
    }

    #[test]
    fn edit_project_unknown_id_is_noop() {
        let mut app = App::default();
        let _ = update(&mut app, Message::StartEditProject(Uuid::now_v7()));
        assert!(app.project_edit.is_none());
    }

    #[test]
    fn edit_project_inputs_update_form() {
        let mut app = App::default();
        let id = add_project(&mut app, "工作");
        let _ = update(&mut app, Message::StartEditProject(id));

        let _ = update(&mut app, Message::ProjectEditNameChanged(" 个人  ".into()));
        let _ = update(
            &mut app,
            Message::ProjectEditStartChanged("2026-01-01".into()),
        );
        let _ = update(
            &mut app,
            Message::ProjectEditEndChanged("2026-01-31".into()),
        );

        let edit = app.project_edit.as_ref().unwrap();
        assert_eq!(edit.name, " 个人  ");
        assert!(edit.start_parsed.as_ref().unwrap().is_some());
        assert!(edit.end_parsed.as_ref().unwrap().is_some());
    }

    #[test]
    fn save_edit_project_commits_name_and_times() {
        let now = Utc::now();
        let mut app = app_with(now);
        let id = add_project(&mut app, "工作");
        let _ = update(&mut app, Message::StartEditProject(id));
        let _ = update(&mut app, Message::ProjectEditNameChanged("  个人  ".into()));
        let _ = update(
            &mut app,
            Message::ProjectEditPriorityChanged(Some(Priority::Medium)),
        );
        let _ = update(
            &mut app,
            Message::ProjectEditStartChanged("2026-01-01".into()),
        );
        let _ = update(
            &mut app,
            Message::ProjectEditEndChanged("2026-01-31 18:30".into()),
        );

        let _ = update(&mut app, Message::SaveEditProject);

        assert!(app.project_edit.is_none()); // 退出编辑态
        let project = &app.projects[0];
        assert_eq!(project.name, "个人"); // trim 后提交
        assert_eq!(project.priority, Some(Priority::Medium));
        assert!(project.started_at.is_some());
        assert!(project.finished_at.is_some());
        assert_eq!(project.created_at, now); // 创建时间不受影响
    }

    #[test]
    fn save_edit_project_blank_or_duplicate_keeps_editing() {
        let mut app = App::default();
        let id = add_project(&mut app, "工作");
        add_project(&mut app, "生活");
        let _ = update(&mut app, Message::StartEditProject(id));

        // 空名称
        let _ = update(&mut app, Message::ProjectEditNameChanged("   ".into()));
        let _ = update(&mut app, Message::SaveEditProject);
        assert_eq!(app.projects[0].name, "工作");
        assert!(app.project_edit.is_some()); // 保持编辑态

        // 与其他项目重名
        let _ = update(&mut app, Message::ProjectEditNameChanged("生活".into()));
        let _ = update(&mut app, Message::SaveEditProject);
        assert_eq!(app.projects[0].name, "工作");
        assert!(app.project_edit.is_some());

        // 与自身同名（重名校验排除自身）：允许提交
        let _ = update(&mut app, Message::ProjectEditNameChanged("工作".into()));
        let _ = update(&mut app, Message::SaveEditProject);
        assert!(app.project_edit.is_none());
        assert_eq!(app.projects[0].name, "工作");
    }

    #[test]
    fn save_edit_project_invalid_times_keep_editing() {
        let mut app = App::default();
        let id = add_project(&mut app, "工作");
        let _ = update(&mut app, Message::StartEditProject(id));

        // 时间格式非法
        let _ = update(&mut app, Message::ProjectEditStartChanged("后天".into()));
        let _ = update(&mut app, Message::SaveEditProject);
        assert!(app.project_edit.is_some());
        assert_eq!(app.projects[0].started_at, None);

        // 开始 ≥ 结束
        let _ = update(
            &mut app,
            Message::ProjectEditStartChanged("2026-02-01".into()),
        );
        let _ = update(
            &mut app,
            Message::ProjectEditEndChanged("2026-01-31".into()),
        );
        let _ = update(&mut app, Message::SaveEditProject);
        assert!(app.project_edit.is_some());
        assert_eq!(app.projects[0].started_at, None);
    }

    #[test]
    fn cancel_edit_project_discards_changes() {
        let mut app = App::default();
        let id = add_project(&mut app, "工作");
        let _ = update(&mut app, Message::StartEditProject(id));
        let _ = update(&mut app, Message::ProjectEditNameChanged("改到一半".into()));

        let _ = update(&mut app, Message::CancelEditProject);

        assert!(app.project_edit.is_none());
        assert_eq!(app.projects[0].name, "工作"); // 名称未变
    }

    #[test]
    fn save_edit_project_deleted_project_exits_editing() {
        let mut app = App::default();
        let id = add_project(&mut app, "工作");
        let _ = update(&mut app, Message::StartEditProject(id));
        app.projects.clear(); // 项目在编辑期间被删除

        let _ = update(&mut app, Message::SaveEditProject);

        assert!(app.project_edit.is_none()); // 仅退出编辑态，无副作用
    }

    #[test]
    fn save_without_editing_project_is_noop() {
        let mut app = App::default();
        let _ = update(&mut app, Message::SaveEditProject);
        assert!(app.projects.is_empty());
    }

    #[test]
    fn delete_project_unassigns_todos_and_resets_selection() {
        let mut app = App::default();
        let pid = add_project(&mut app, "工作");
        let todo_id = add_todo(&mut app, "写方案");
        app.todos
            .iter_mut()
            .find(|t| t.id == todo_id)
            .unwrap()
            .project_id = Some(pid);
        let _ = update(&mut app, Message::SelectProject(Some(pid)));
        let _ = update(&mut app, Message::StartEditProject(pid));
        assert_eq!(app.todos[0].project_id, Some(pid));

        let _ = update(&mut app, Message::DeleteProject(pid));

        assert!(app.projects.is_empty());
        assert_eq!(app.todos[0].project_id, None); // 任务保留，归属解除
        assert_eq!(app.selected_project, None); // 筛选复位
        assert!(app.project_edit.is_none()); // 编辑态复位
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
            todos: vec![Todo::new("任务".into(), Utc::now())],
            projects: vec![Project::new("工作".into(), Utc::now())],
            sort_mode: SortMode::Priority,
            project_sort_mode: SortMode::Due,
        };

        let _ = update(&mut app, Message::Loaded(Ok(store)));

        assert_eq!(app.todos.len(), 1);
        assert_eq!(app.projects.len(), 1);
        assert_eq!(app.sort_mode, SortMode::Priority); // 排序偏好随加载恢复
        assert_eq!(app.project_sort_mode, SortMode::Due);
    }

    #[test]
    fn sort_mode_changed_switches_and_persists() {
        let mut app = App::default();
        assert_eq!(app.sort_mode, SortMode::default()); // 默认综合

        let _ = update(&mut app, Message::SortModeChanged(SortMode::Priority));
        assert_eq!(app.sort_mode, SortMode::Priority);

        let _ = update(&mut app, Message::ProjectSortModeChanged(SortMode::Due));
        assert_eq!(app.project_sort_mode, SortMode::Due);
        assert_eq!(app.sort_mode, SortMode::Priority); // 互不影响
    }

    #[test]
    fn loaded_error_sets_hint() {
        let mut app = App::default();
        let _ = update(&mut app, Message::Loaded(Err("磁盘错误".into())));
        assert!(app.error.is_some());
    }

    // ---------- 弹窗添加任务 ----------

    #[test]
    fn open_dialog_prefills_selected_project() {
        let mut app = App::default();
        let pid = add_project(&mut app, "工作");
        let _ = update(&mut app, Message::SelectProject(Some(pid)));

        let _ = update(&mut app, Message::OpenAddDialog);

        let dialog = app.add_dialog.as_ref().unwrap();
        assert_eq!(dialog.project_id, Some(pid)); // 预选当前筛选的项目
        assert!(dialog.title.is_empty());
        assert_eq!(dialog.due_parsed, Ok(None)); // 截止时间默认留空
    }

    #[test]
    fn open_dialog_without_selection_has_no_project() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenAddDialog);
        assert_eq!(app.add_dialog.as_ref().unwrap().project_id, None);
    }

    #[test]
    fn close_dialog_discards_input() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::DialogTitleChanged("写方案".into()));

        let _ = update(&mut app, Message::CloseAddDialog);

        assert!(app.add_dialog.is_none());
        assert!(app.todos.is_empty()); // 未创建任何任务
    }

    #[test]
    fn dialog_inputs_update_form() {
        let mut app = App::default();
        let pid = add_project(&mut app, "工作");
        let _ = update(&mut app, Message::OpenAddDialog);

        let _ = update(&mut app, Message::DialogTitleChanged("写方案".into()));
        let _ = update(
            &mut app,
            Message::DialogDescriptionChanged("先读需求".into()),
        );
        let _ = update(&mut app, Message::DialogProjectChanged(Some(pid)));
        let _ = update(
            &mut app,
            Message::DialogDueChanged("2026-01-31 18:30".into()),
        );

        let dialog = app.add_dialog.as_ref().unwrap();
        assert_eq!(dialog.title, "写方案");
        assert_eq!(dialog.description, "先读需求");
        assert_eq!(dialog.project_id, Some(pid));
        assert!(dialog.due_parsed.as_ref().unwrap().is_some());
    }

    #[test]
    fn dialog_inputs_ignored_when_closed() {
        let mut app = App::default();
        let _ = update(&mut app, Message::DialogTitleChanged("写方案".into()));
        let _ = update(&mut app, Message::DialogDueChanged("2026-01-31".into()));
        assert!(app.add_dialog.is_none());
    }

    #[test]
    fn dialog_unknown_project_is_rejected() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(
            &mut app,
            Message::DialogProjectChanged(Some(Uuid::now_v7())),
        );
        assert_eq!(app.add_dialog.as_ref().unwrap().project_id, None);
    }

    #[test]
    fn dialog_due_invalid_shows_error() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::DialogDueChanged("后天".into()));
        assert!(app.add_dialog.as_ref().unwrap().due_parsed.is_err());
    }

    #[test]
    fn dialog_quick_due_fills_and_parses() {
        let mut app = app_with(Utc::now());
        let _ = update(&mut app, Message::OpenAddDialog);

        let _ = update(&mut app, Message::DialogQuickDue(QuickDue::Tomorrow));

        let dialog = app.add_dialog.as_ref().unwrap();
        assert!(dialog.due_input.contains("23:59")); // 回填文本
        assert!(dialog.due_parsed.as_ref().unwrap().is_some()); // 可解析
    }

    #[test]
    fn submit_dialog_creates_full_todo() {
        let now = Utc::now();
        let mut app = app_with(now);
        let pid = add_project(&mut app, "工作");
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::DialogTitleChanged("  写方案  ".into()));
        let _ = update(
            &mut app,
            Message::DialogDescriptionChanged("  先读需求  ".into()),
        );
        let _ = update(
            &mut app,
            Message::DialogPriorityChanged(Some(Priority::High)),
        );
        let _ = update(&mut app, Message::DialogProjectChanged(Some(pid)));
        let _ = update(
            &mut app,
            Message::DialogDueChanged("2026-01-31 18:30".into()),
        );

        let _ = update(&mut app, Message::SubmitAddDialog);

        assert!(app.add_dialog.is_none()); // 弹窗关闭
        assert_eq!(app.todos.len(), 1);
        let todo = &app.todos[0];
        assert_eq!(todo.title, "写方案"); // trim 后存储
        assert_eq!(todo.description, "先读需求"); // trim 后存储
        assert_eq!(todo.priority, Some(Priority::High));
        assert_eq!(todo.project_id, Some(pid));
        assert!(todo.due_at.is_some());
        assert_eq!(todo.created_at, now); // 时间取自 app.now
        assert_eq!(todo.status(), TodoStatus::Pending);
    }

    #[test]
    fn submit_dialog_without_due_creates_todo() {
        let now = Utc::now();
        let mut app = app_with(now);
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::DialogTitleChanged("无截止时间".into()));

        let _ = update(&mut app, Message::SubmitAddDialog);

        assert!(app.add_dialog.is_none());
        assert_eq!(app.todos[0].due_at, None);
    }

    #[test]
    fn submit_dialog_blank_title_keeps_dialog() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::DialogTitleChanged("   ".into()));

        let _ = update(&mut app, Message::SubmitAddDialog);

        assert!(app.add_dialog.is_some()); // 弹窗保持打开
        assert_eq!(app.add_dialog.as_ref().unwrap().title, "   "); // 输入保留
        assert!(app.todos.is_empty());
    }

    #[test]
    fn submit_dialog_invalid_due_keeps_dialog() {
        let mut app = App::default();
        let _ = update(&mut app, Message::OpenAddDialog);
        let _ = update(&mut app, Message::DialogTitleChanged("写方案".into()));
        let _ = update(&mut app, Message::DialogDueChanged("后天".into()));

        let _ = update(&mut app, Message::SubmitAddDialog);

        assert!(app.add_dialog.is_some());
        assert!(app.todos.is_empty());
    }

    #[test]
    fn submit_without_open_dialog_is_noop() {
        let mut app = App::default();
        let _ = update(&mut app, Message::SubmitAddDialog);
        assert!(app.todos.is_empty());
    }

    // ---------- 卡片编辑 ----------

    #[test]
    fn edit_todo_prefills_current_fields() {
        let now = Utc::now();
        let mut app = app_with(now);
        let pid = add_project(&mut app, "工作");
        let todo_id = add_todo(&mut app, "写方案");
        app.todos[0].description = "先读需求".into();
        app.todos[0].priority = Some(Priority::Medium);
        app.todos[0].project_id = Some(pid);
        app.todos[0].due_at = Some(now);

        let _ = update(&mut app, Message::EditTodo(todo_id));

        let edit = app.todo_edit.as_ref().unwrap();
        assert_eq!(edit.todo_id, todo_id);
        assert_eq!(edit.title, "写方案");
        assert_eq!(edit.description, "先读需求");
        assert_eq!(edit.priority, Some(Priority::Medium));
        assert_eq!(edit.project_id, Some(pid));
        assert!(!edit.due_input.is_empty()); // 截止时间回填为可解析文本
        assert!(edit.due_parsed.is_ok());
    }

    #[test]
    fn edit_todo_unknown_id_is_noop() {
        let mut app = App::default();
        let _ = update(&mut app, Message::EditTodo(Uuid::now_v7()));
        assert!(app.todo_edit.is_none());
    }

    #[test]
    fn edit_inputs_update_form() {
        let mut app = App::default();
        let pid = add_project(&mut app, "工作");
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(&mut app, Message::EditTodo(todo_id));

        let _ = update(&mut app, Message::EditTitleChanged(" 改标题 ".into()));
        let _ = update(&mut app, Message::EditDescriptionChanged("新描述".into()));
        let _ = update(&mut app, Message::EditProjectChanged(Some(pid)));
        let _ = update(&mut app, Message::EditDueChanged("2026-01-31 18:30".into()));

        let edit = app.todo_edit.as_ref().unwrap();
        assert_eq!(edit.title, " 改标题 ");
        assert_eq!(edit.description, "新描述");
        assert_eq!(edit.project_id, Some(pid));
        assert!(edit.due_parsed.as_ref().unwrap().is_some());
    }

    #[test]
    fn edit_inputs_ignored_when_not_editing() {
        let mut app = App::default();
        let _ = update(&mut app, Message::EditTitleChanged("x".into()));
        let _ = update(&mut app, Message::EditDueChanged("2026-01-31".into()));
        assert!(app.todo_edit.is_none());
    }

    #[test]
    fn edit_unknown_project_is_rejected() {
        let mut app = App::default();
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(&mut app, Message::EditTodo(todo_id));
        let _ = update(&mut app, Message::EditProjectChanged(Some(Uuid::now_v7())));
        assert_eq!(app.todo_edit.as_ref().unwrap().project_id, None);
    }

    #[test]
    fn edit_quick_due_fills_and_parses() {
        let mut app = app_with(Utc::now());
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(&mut app, Message::EditTodo(todo_id));

        let _ = update(&mut app, Message::EditQuickDue(QuickDue::Tomorrow));

        let edit = app.todo_edit.as_ref().unwrap();
        assert!(edit.due_input.contains("23:59")); // 回填文本
        assert!(edit.due_parsed.as_ref().unwrap().is_some()); // 可解析
    }

    #[test]
    fn cancel_edit_discards_changes() {
        let mut app = App::default();
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(&mut app, Message::EditTodo(todo_id));
        let _ = update(&mut app, Message::EditTitleChanged("改到一半".into()));

        let _ = update(&mut app, Message::CancelEditTodo);

        assert!(app.todo_edit.is_none());
        assert_eq!(app.todos[0].title, "写方案"); // 任务未变
    }

    #[test]
    fn switching_edit_target_discards_uncommitted() {
        let mut app = App::default();
        let a = add_todo(&mut app, "任务 A");
        let b = add_todo(&mut app, "任务 B");
        let _ = update(&mut app, Message::EditTodo(a));
        let _ = update(&mut app, Message::EditTitleChanged("改 A".into()));

        let _ = update(&mut app, Message::EditTodo(b));

        let edit = app.todo_edit.as_ref().unwrap();
        assert_eq!(edit.todo_id, b);
        assert_eq!(edit.title, "任务 B"); // A 的未保存修改被丢弃
    }

    #[test]
    fn save_edit_commits_all_fields() {
        let now = Utc::now();
        let mut app = app_with(now);
        let pid = add_project(&mut app, "工作");
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(&mut app, Message::EditTodo(todo_id));
        let _ = update(&mut app, Message::EditTitleChanged("  写周报  ".into()));
        let _ = update(
            &mut app,
            Message::EditDescriptionChanged("  整理数据  ".into()),
        );
        let _ = update(&mut app, Message::EditPriorityChanged(Some(Priority::Low)));
        let _ = update(&mut app, Message::EditProjectChanged(Some(pid)));
        let _ = update(&mut app, Message::EditDueChanged("2026-01-31 18:30".into()));

        let _ = update(&mut app, Message::SaveEditTodo);

        assert!(app.todo_edit.is_none()); // 退出编辑模式
        let todo = &app.todos[0];
        assert_eq!(todo.title, "写周报"); // trim 后提交
        assert_eq!(todo.description, "整理数据");
        assert_eq!(todo.priority, Some(Priority::Low));
        assert_eq!(todo.project_id, Some(pid));
        assert!(todo.due_at.is_some());
        assert_eq!(todo.created_at, now); // 时间字段不受影响
        assert_eq!(todo.status(), TodoStatus::Pending);
    }

    #[test]
    fn save_edit_blank_title_keeps_editing() {
        let mut app = App::default();
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(&mut app, Message::EditTodo(todo_id));
        let _ = update(&mut app, Message::EditTitleChanged("   ".into()));

        let _ = update(&mut app, Message::SaveEditTodo);

        assert!(app.todo_edit.is_some()); // 保持编辑模式
        assert_eq!(app.todo_edit.as_ref().unwrap().title, "   "); // 输入保留
        assert_eq!(app.todos[0].title, "写方案"); // 任务未变
    }

    #[test]
    fn save_edit_invalid_due_keeps_editing() {
        let mut app = App::default();
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(&mut app, Message::EditTodo(todo_id));
        let _ = update(&mut app, Message::EditDueChanged("后天".into()));

        let _ = update(&mut app, Message::SaveEditTodo);

        assert!(app.todo_edit.is_some());
        assert_eq!(app.todos[0].due_at, None);
    }

    #[test]
    fn save_edit_unknown_project_keeps_editing() {
        let mut app = App::default();
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(&mut app, Message::EditTodo(todo_id));
        // 防御层漏检场景：直接构造非法归属（如项目在编辑期间被删除）
        app.todo_edit.as_mut().unwrap().project_id = Some(Uuid::now_v7());

        let _ = update(&mut app, Message::SaveEditTodo);

        assert!(app.todo_edit.is_some()); // 保持编辑模式
        assert_eq!(app.todos[0].project_id, None); // 任务未变
    }

    #[test]
    fn save_edit_deleted_todo_exits_editing() {
        let mut app = App::default();
        let todo_id = add_todo(&mut app, "写方案");
        let _ = update(&mut app, Message::EditTodo(todo_id));
        // 任务在编辑期间被删除（防御路径）：保存仅退出编辑态，无副作用
        app.todos.clear();

        let _ = update(&mut app, Message::SaveEditTodo);

        assert!(app.todo_edit.is_none());
        assert!(app.todos.is_empty());
    }

    #[test]
    fn save_without_editing_is_noop() {
        let mut app = App::default();
        let _ = update(&mut app, Message::SaveEditTodo);
        assert!(app.todos.is_empty());
    }
}
