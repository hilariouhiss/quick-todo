//! 视图：把应用状态渲染成 iced 元素树。
//!
//! 布局：左侧项目侧边栏 + 右侧主区域（标题栏 + 输入行 + 可滚动任务列表）。
//! 每个任务一张卡片：标题、状态徽章、操作按钮、归属项目选择器，
//! 以及创建 / 开始 / 结束三个时间点和（实时）耗时。

use chrono::{DateTime, Duration, Utc};
use iced::font::Weight;
use iced::widget::{PickList, Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Font, Length};
use uuid::Uuid;

use crate::model::{App, Project, Todo, TodoStatus};
use crate::update::Message;

/// 次要文本（标签、提示）颜色：中性灰
const MUTED: Color = Color::from_rgb(0.55, 0.58, 0.62);
/// 错误提示：红
const ERROR_COLOR: Color = Color::from_rgb(0.92, 0.45, 0.45);
/// 进行中（橙）
const ACCENT: Color = Color::from_rgb(0.98, 0.70, 0.25);
/// 已完成（绿）
const DONE: Color = Color::from_rgb(0.36, 0.78, 0.50);
/// 侧边栏固定宽度
const SIDEBAR_WIDTH: f32 = 220.0;

const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};

/// 应用主视图：左侧项目侧边栏（可收放）+ 右侧任务主区域。
pub fn view(app: &App) -> Element<'_, Message> {
    // 标题栏：侧边栏收起时，左侧显示「展开」按钮
    let header = if app.sidebar_visible {
        row![
            text("待办清单").size(26).font(BOLD),
            Space::new().width(Length::Fill),
            text(summary(app)).size(13).color(MUTED),
        ]
    } else {
        row![
            button(text("» 展开").size(14))
                .on_press(Message::ToggleSidebar)
                .padding([4, 10]),
            Space::new().width(8),
            text("待办清单").size(26).font(BOLD),
            Space::new().width(Length::Fill),
            text(summary(app)).size(13).color(MUTED),
        ]
    }
    .align_y(Alignment::Center);

    // 输入区：标题行 + 描述行（两个输入框回车均可添加）
    let input_column = column![
        row![
            text_input("输入任务内容，回车或点击“添加”", &app.input)
                .on_input(Message::InputChanged)
                .on_submit(Message::AddTodo)
                .padding(10)
                .width(Length::Fill),
            Space::new().width(8),
            button(text("添加").size(15))
                .on_press(Message::AddTodo)
                .padding([10, 22]),
        ]
        .align_y(Alignment::Center),
        text_input("任务描述（可选），回车同样可添加", &app.description_input)
            .on_input(Message::DescriptionInputChanged)
            .on_submit(Message::AddTodo)
            .padding(10),
    ]
    .spacing(8);

    let mut body = column![header, input_column]
        .spacing(12)
        .height(Length::Fill);

    if let Some(error) = &app.error {
        body = body.push(text(error.as_str()).size(12).color(ERROR_COLOR));
    }

    // 按当前筛选的项目过滤任务列表
    let visible: Vec<&Todo> = app
        .todos
        .iter()
        .filter(|todo| app.selected_project.is_none() || todo.project_id == app.selected_project)
        .collect();

    body = body.push(if visible.is_empty() {
        empty_hint(if app.selected_project.is_some() {
            "该项目暂无任务"
        } else {
            "暂无任务，先添加一个吧"
        })
    } else {
        todo_list(app, visible)
    });

    // 侧边栏展开时并排显示；收起时任务区占满
    let content: Element<'_, Message> = if app.sidebar_visible {
        row![project_sidebar(app), Space::new().width(16), body].into()
    } else {
        body.into()
    };

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(24)
        .center_x(Length::Fill)
        .into()
}

/// 顶部摘要：总数 / 进行中 / 已完成。
fn summary(app: &App) -> String {
    let total = app.todos.len();
    let in_progress = app
        .todos
        .iter()
        .filter(|todo| todo.status() == TodoStatus::InProgress)
        .count();
    let done = app
        .todos
        .iter()
        .filter(|todo| todo.status() == TodoStatus::Done)
        .count();
    format!("共 {total} 项 · 进行中 {in_progress} · 已完成 {done}")
}

/// 空列表提示。
fn empty_hint(message: &'static str) -> Element<'static, Message> {
    container(text(message).size(14).color(MUTED))
        .width(Length::Fill)
        .padding(32)
        .center_x(Length::Fill)
        .into()
}

// ---------- 项目侧边栏 ----------

/// 左侧项目侧边栏：新建输入行 + 可滚动项目列表（"全部" + 各项目），头部可收起。
fn project_sidebar(app: &App) -> Element<'_, Message> {
    let header = column![
        row![
            text("项目").size(14).font(BOLD),
            Space::new().width(Length::Fill),
            button(text("« 收起").size(12))
                .on_press(Message::ToggleSidebar)
                .padding([4, 8]),
        ]
        .align_y(Alignment::Center),
        row![
            text_input("新建项目…", &app.project_input)
                .on_input(Message::ProjectInputChanged)
                .on_submit(Message::AddProject)
                .padding(8)
                .width(Length::Fill),
            Space::new().width(6),
            button(text("添加").size(13))
                .on_press(Message::AddProject)
                .padding([6, 12]),
        ]
        .align_y(Alignment::Center),
        Space::new().height(2),
    ]
    .spacing(8);

    let mut list = column![project_row(app, None, "全部", app.todos.len())].spacing(4);

    if app.projects.is_empty() {
        list = list.push(
            container(text("暂无项目").size(12).color(MUTED))
                .padding([6, 10])
                .width(Length::Fill),
        );
    } else {
        for project in &app.projects {
            let count = app
                .todos
                .iter()
                .filter(|todo| todo.project_id == Some(project.id))
                .count();
            list = list.push(project_row(app, Some(project.id), &project.name, count));
        }
    }

    container(
        column![header, scrollable(list).height(Length::Fill)]
            .spacing(10)
            .height(Length::Fill),
    )
    .width(Length::Fixed(SIDEBAR_WIDTH))
    .padding(12)
    .style(card_style)
    .into()
}

/// 单个项目行：选中高亮 + 计数 + 编辑/删除；编辑态切换为内联重命名。
fn project_row<'a>(
    app: &'a App,
    id: Option<Uuid>,
    name: &'a str,
    count: usize,
) -> Element<'a, Message> {
    // 行内重命名编辑态
    if app.editing_project.is_some() && app.editing_project == id {
        return row![
            text_input("项目名称", &app.project_edit_input)
                .on_input(Message::ProjectRenameChanged)
                .on_submit(Message::SaveRenameProject)
                .padding(6)
                .width(Length::Fill),
            Space::new().width(4),
            button(text("保存").size(12))
                .on_press(Message::SaveRenameProject)
                .padding([4, 8]),
            button(text("取消").size(12))
                .on_press(Message::CancelRenameProject)
                .padding([4, 8]),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .padding([4, 2])
        .into();
    }

    let selected = app.selected_project == id;

    let mut actions = row![
        button(text(name).size(13))
            .on_press(Message::SelectProject(id))
            .style(button::text)
            .padding([4, 2])
            .width(Length::Fill),
        text(count.to_string()).size(12).color(MUTED),
    ]
    .align_y(Alignment::Center)
    .spacing(6);

    // 项目行才有"编辑 / 删除"；"全部"行没有
    if let Some(project_id) = id {
        actions = actions
            .push(
                button(text("编辑").size(12))
                    .on_press(Message::StartRenameProject(project_id))
                    .style(button::text)
                    .padding([2, 6]),
            )
            .push(
                button(text("删除").size(12))
                    .on_press(Message::DeleteProject(project_id))
                    .style(button::text)
                    .padding([2, 6]),
            );
    }

    container(actions)
        .width(Length::Fill)
        .padding([4, 8])
        .style(move |theme| project_row_style(theme, selected))
        .into()
}

/// 项目行样式：选中时使用主题主色描边与底色，否则与卡片同风格。
fn project_row_style(theme: &iced::Theme, selected: bool) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(if selected {
            palette.primary.weak.color
        } else {
            palette.background.weak.color
        })),
        border: Border {
            color: if selected {
                palette.primary.base.color
            } else {
                palette.background.strong.color
            },
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

// ---------- 任务列表 ----------

/// 可滚动的任务列表（已按筛选过滤）。
fn todo_list<'a>(app: &'a App, todos: Vec<&'a Todo>) -> Element<'a, Message> {
    scrollable(
        column(todos.into_iter().map(|todo| todo_card(todo, app)))
            .spacing(8)
            .padding(4),
    )
    .height(Length::Fill)
    .into()
}

/// 单个任务的卡片：标题 + 可选描述 + 状态徽章 + 操作按钮 + 时间元信息。
fn todo_card<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let head = row![
        text(todo.title.as_str()).size(16).font(BOLD),
        Space::new().width(Length::Fill),
        text(todo.status().label())
            .size(12)
            .color(status_color(todo.status())),
        Space::new().width(8),
        actions(todo),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    // 描述：非空时在标题下方以灰色小字显示（自动换行）
    let mut content = column![head].spacing(8);
    if !todo.description.is_empty() {
        content = content.push(
            text(todo.description.as_str())
                .size(13)
                .color(MUTED)
                .width(Length::Fill),
        );
    }
    content = content.push(meta_rows(todo, app));

    container(content)
        .width(Length::Fill)
        .padding(12)
        .style(card_style)
        .into()
}

/// 状态徽章颜色：未开始＝灰、进行中＝橙、已完成＝绿。
fn status_color(status: TodoStatus) -> Color {
    match status {
        TodoStatus::Pending => MUTED,
        TodoStatus::InProgress => ACCENT,
        TodoStatus::Done => DONE,
    }
}

/// 操作按钮：按状态显示"开始 / 完成"，始终有"删除"。
fn actions(todo: &Todo) -> Element<'_, Message> {
    let mut actions = row![].spacing(6);

    match todo.status() {
        TodoStatus::Pending => {
            actions = actions.push(
                button(text("开始").size(13))
                    .on_press(Message::StartTodo(todo.id))
                    .style(button::primary)
                    .padding([6, 14]),
            );
        }
        TodoStatus::InProgress => {
            actions = actions.push(
                button(text("完成").size(13))
                    .on_press(Message::FinishTodo(todo.id))
                    .style(success_button)
                    .padding([6, 14]),
            );
        }
        TodoStatus::Done => {}
    }

    actions
        .push(
            button(text("删除").size(13))
                .on_press(Message::DeleteTodo(todo.id))
                .style(button::danger)
                .padding([6, 14]),
        )
        .into()
}

/// "完成"按钮样式：成功绿，悬停时加深。
fn success_button(theme: &iced::Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let base = button::Style {
        background: Some(Background::Color(palette.success.base.color)),
        text_color: palette.success.base.text,
        border: Border::default(),
        shadow: iced::Shadow::default(),
        snap: false,
    };

    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(palette.success.strong.color)),
            ..base
        },
        _ => base,
    }
}

/// 卡片样式：比窗口背景稍亮的底色 + 圆角边框（跟随主题）。
fn card_style(theme: &iced::Theme) -> container::Style {
    let background = theme.extended_palette().background;
    container::Style {
        background: Some(Background::Color(background.weak.color)),
        border: Border {
            color: background.strong.color,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

// ---------- 任务元信息 ----------

/// 归属项目选择器（pick_list）：切换 / 解除任务的项目归属。
fn project_picker_row<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let options: Vec<ProjectChoice> = std::iter::once(ProjectChoice::none())
        .chain(app.projects.iter().map(ProjectChoice::of))
        .collect();

    row![
        text("项目")
            .size(13)
            .color(MUTED)
            .width(Length::Fixed(72.0)),
        PickList::new(
            options,
            Some(ProjectChoice::of_todo(todo, &app.projects)),
            move |choice| Message::AssignProject {
                todo_id: todo.id,
                project_id: choice.id,
            },
        )
        .placeholder("无项目")
        .text_size(13)
        .padding([4, 8])
        .width(Length::Fill),
    ]
    .spacing(6)
    .into()
}

/// pick_list 的选项包装：`id = None` 表示"无项目"（解除归属）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectChoice {
    id: Option<Uuid>,
    label: String,
}

impl ProjectChoice {
    /// "无项目"选项。
    fn none() -> Self {
        Self {
            id: None,
            label: "无项目".into(),
        }
    }

    /// 由项目构造选项。
    fn of(project: &Project) -> Self {
        Self {
            id: Some(project.id),
            label: project.name.clone(),
        }
    }

    /// 任务的当前归属（项目已被删除时回落为"无项目"）。
    fn of_todo(todo: &Todo, projects: &[Project]) -> Self {
        match todo
            .project_id
            .and_then(|id| projects.iter().find(|p| p.id == id))
        {
            Some(project) => Self::of(project),
            None => Self::none(),
        }
    }
}

impl std::fmt::Display for ProjectChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// 时间元信息：项目归属 + 创建 / 开始 / 结束；进行中附实时耗时，已完成附总耗时。
fn meta_rows<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let mut meta = column![
        project_picker_row(todo, app),
        time_row("创建时间", format_time(todo.created_at), MUTED),
        time_row(
            "开始时间",
            todo.started_at
                .map(format_time)
                .unwrap_or_else(|| "—".into()),
            MUTED,
        ),
    ]
    .spacing(3);

    if todo.status() == TodoStatus::InProgress {
        let elapsed = todo
            .duration(app.now)
            .map(format_duration)
            .unwrap_or_else(|| "—".into());
        meta = meta.push(time_row("已耗时", format!("{elapsed}（实时）"), ACCENT));
    }

    meta = meta.push(time_row(
        "结束时间",
        todo.finished_at
            .map(format_time)
            .unwrap_or_else(|| "—".into()),
        MUTED,
    ));

    if todo.status() == TodoStatus::Done {
        let total = todo
            .duration(app.now)
            .map(format_duration)
            .unwrap_or_else(|| "—".into());
        meta = meta.push(time_row("总耗时", total, DONE));
    }

    meta.into()
}

/// 带固定宽度标签的一行时间信息。
fn time_row(label: &str, value: String, color: Color) -> Element<'_, Message> {
    row![
        text(label).size(13).color(MUTED).width(Length::Fixed(72.0)),
        text(value).size(13).color(color),
    ]
    .spacing(6)
    .into()
}

/// 时间格式化：UTC 存储、本地时区显示。
fn format_time(dt: DateTime<Utc>) -> String {
    dt.with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// 耗时格式化："X 小时 Y 分 Z 秒" / "Y 分 Z 秒" / "Z 秒"。
fn format_duration(d: Duration) -> String {
    let total = d.num_seconds().max(0);
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    match (hours, minutes) {
        (0, 0) => format!("{seconds} 秒"),
        (0, _) => format!("{minutes} 分 {seconds} 秒"),
        _ => format!("{hours} 小时 {minutes} 分 {seconds} 秒"),
    }
}
