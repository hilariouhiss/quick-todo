//! 视图：把应用状态渲染成 iced 元素树。
//!
//! 布局：标题栏（含「＋ 添加任务 / ＋ 添加项目」双按钮）+ 任务排序行 + 项目单行栏
//! （横向滚动芯片）+ 双列任务区；右下角悬浮「已完成 (N)」归档入口。
//! 每个任务一张卡片：标题、状态徽章、操作按钮、只读属性展示（含项目归属），
//! 以及创建 / 开始 / 结束三个时间点和（实时）耗时；「编辑」按钮在卡片右下角。

use chrono::{DateTime, Duration, Utc};
use iced::font::Weight;
use iced::widget::{
    PickList, Space, button, column, container, mouse_area, opaque, row, scrollable, stack, text,
    text_input, tooltip,
};
use iced::{Alignment, Background, Border, Color, Element, Font, Length};
use uuid::Uuid;

use crate::model::{App, Priority, Project, QuickDue, SortMode, Todo, TodoStatus};
use crate::update::Message;

/// 次要文本（标签、提示）颜色：中性灰
const MUTED: Color = Color::from_rgb(0.55, 0.58, 0.62);
/// 错误提示：红
const ERROR_COLOR: Color = Color::from_rgb(0.92, 0.45, 0.45);
/// 进行中（橙）
const ACCENT: Color = Color::from_rgb(0.98, 0.70, 0.25);
/// 已完成（绿）
const DONE: Color = Color::from_rgb(0.36, 0.78, 0.50);

/// 弹窗标题输入框的 widget Id（打开弹窗时聚焦用）
pub(crate) const DIALOG_TITLE_ID: iced::widget::Id = iced::widget::Id::new("add-dialog-title");

/// 项目弹窗名称输入框的 widget Id（打开弹窗时聚焦用）
pub(crate) const PROJECT_DIALOG_NAME_ID: iced::widget::Id =
    iced::widget::Id::new("add-project-dialog-name");

/// 项目内联编辑名称输入框的 widget Id（进入编辑态时聚焦用）
pub(crate) const PROJECT_EDIT_NAME_ID: iced::widget::Id =
    iced::widget::Id::new("edit-project-name");

/// 弹窗截止时间的快捷选项（选中后回填到文本输入框，仍可手动修改）
const QUICK_DUE_OPTIONS: [QuickDue; 3] = [QuickDue::Today, QuickDue::Tomorrow, QuickDue::Sunday];

/// 排序方式下拉的固定选项（任务区与项目栏共用）。
const SORT_MODES: [SortMode; 3] = [SortMode::Priority, SortMode::Due, SortMode::Combined];

/// 优先级下拉的选项（"无" + 低 / 中 / 高）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PriorityChoice {
    value: Option<Priority>,
    label: &'static str,
}

impl PriorityChoice {
    /// "无"选项。
    const NONE: Self = Self {
        value: None,
        label: "无",
    };

    /// 由优先级构造选项（`None` = "无"）。
    const fn of(value: Option<Priority>) -> Self {
        match value {
            None => Self::NONE,
            Some(priority) => Self {
                value: Some(priority),
                label: priority.label(),
            },
        }
    }
}

impl std::fmt::Display for PriorityChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

/// 优先级下拉组件（任务 / 项目弹窗与编辑表单共用）。
fn priority_picker<'a>(
    selected: Option<Priority>,
    on_select: fn(Option<Priority>) -> Message,
) -> Element<'a, Message> {
    PickList::new(
        &[
            PriorityChoice::NONE,
            PriorityChoice {
                value: Some(Priority::Low),
                label: "低",
            },
            PriorityChoice {
                value: Some(Priority::Medium),
                label: "中",
            },
            PriorityChoice {
                value: Some(Priority::High),
                label: "高",
            },
        ][..],
        Some(PriorityChoice::of(selected)),
        move |choice| on_select(choice.value),
    )
    .text_size(13)
    .padding([4, 8])
    .width(Length::Fill)
    .into()
}

/// 优先级徽章 / 圆点颜色：高=红、中=橙、低=灰。
fn priority_color(priority: Priority) -> Color {
    match priority {
        Priority::High => ERROR_COLOR,
        Priority::Medium => ACCENT,
        Priority::Low => MUTED,
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};

/// 应用主视图：标题栏（双按钮）+ 项目单行栏 + 双列任务区；右下角悬浮归档入口。
/// 任一弹窗（任务添加 / 项目添加编辑 / 已完成归档）打开时，叠加模态遮罩与弹窗卡片。
pub fn view(app: &App) -> Element<'_, Message> {
    // 标题栏：左侧标题；右端摘要 + 「＋ 添加任务」下方「＋ 添加项目」双按钮
    let header = row![
        text("待办清单").size(26).font(BOLD),
        Space::new().width(Length::Fill),
        text(summary(app)).size(13).color(MUTED),
        Space::new().width(10),
        column![
            button(text("＋ 添加任务").size(13))
                .on_press(Message::OpenAddDialog)
                .style(button::primary)
                .padding([6, 12]),
            button(text("＋ 添加项目").size(13))
                .on_press(Message::OpenProjectDialog)
                .style(button::secondary)
                .padding([6, 12]),
        ]
        .spacing(6)
        .align_x(Alignment::End),
    ]
    .align_y(Alignment::Center);

    let mut body = column![header].spacing(12).height(Length::Fill);

    if let Some(error) = &app.error {
        body = body.push(text(error.as_str()).size(12).color(ERROR_COLOR));
    }

    // 任务排序选择行（控制两列的组内排序）
    body = body.push(sort_picker_row(app));

    // 项目单行栏（任务列表上方；点击芯片筛选该项目任务）
    body = body.push(project_bar(app));

    // 项目编辑面板：选中项目点「编辑」后在项目栏下方展开（防御：项目已删除则不渲染）
    if app
        .project_edit
        .as_ref()
        .is_some_and(|edit| app.projects.iter().any(|p| p.id == edit.project_id))
    {
        body = body.push(project_edit_panel(app));
    }

    // 按当前筛选的项目过滤任务列表，再按状态分列（已完成归档到弹窗）
    let visible: Vec<&Todo> = app
        .todos
        .iter()
        .filter(|todo| app.selected_project.is_none() || todo.project_id == app.selected_project)
        .collect();
    body = body.push(grouped_columns(app, visible));

    let base = container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(24)
        .center_x(Length::Fill);

    // 右下角悬浮「已完成 (N)」入口（叠加在内容之上、弹窗遮罩之下）
    let content = stack![base, completed_fab(app)]
        .width(Length::Fill)
        .height(Length::Fill);

    // 弹窗打开时：内容之上叠加 遮罩（点击关闭）+ 弹窗卡片（不透明，防穿透）
    // 任务 / 项目 / 归档弹窗互斥；任务弹窗内「＋ 新建」会叠加打开项目弹窗（顶层）
    // ——因此项目弹窗优先渲染，Esc / 遮罩关闭后返回任务弹窗（update 层保证）
    let dialog = if app.project_dialog.is_some() {
        Some(project_dialog_card(app))
    } else if app.add_dialog.is_some() {
        Some(add_dialog_card(app))
    } else if app.show_completed {
        Some(completed_dialog_card(app))
    } else {
        None
    };
    match dialog {
        Some(card) => modal_overlay(content.into(), card),
        None => content.into(),
    }
}

/// 模态叠加层：底部内容 + 半透明遮罩（点击关闭当前弹窗）+ 居中弹窗卡片。
fn modal_overlay<'a>(
    base: Element<'a, Message>,
    card: Element<'a, Message>,
) -> Element<'a, Message> {
    stack![
        base,
        mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(scrim_style),
        )
        .on_press(Message::CloseActiveDialog),
        container(opaque(card))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// 模态遮罩样式：半透明黑（弹窗打开时压暗背景）。
fn scrim_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.55))),
        ..Default::default()
    }
}

// ---------- 弹窗添加任务 ----------

/// 弹窗卡片：标题 / 描述 / 所属项目 / 截止时间（输入 + 快捷下拉）+ 操作按钮。
fn add_dialog_card<'a>(app: &'a App) -> Element<'a, Message> {
    let dialog = app.add_dialog.as_ref().expect("弹窗卡片仅在弹窗打开时渲染");

    // 标题（必填）：回车提交
    let title_input = text_input("任务标题（必填）", &dialog.title)
        .id(DIALOG_TITLE_ID)
        .on_input(Message::DialogTitleChanged)
        .on_submit(Message::SubmitAddDialog)
        .padding(10);

    // 描述（可选）：回车提交
    let description_input = text_input("任务描述（可选）", &dialog.description)
        .on_input(Message::DialogDescriptionChanged)
        .on_submit(Message::SubmitAddDialog)
        .padding(10);

    // 所属项目："无项目" + 全部项目（与编辑模式共用选项包装）+ 快速新建入口
    // （点击「＋ 新建」弹出与标题栏相同的新建项目弹窗，创建成功自动选中）
    let quick_btn = button(text("＋ 新建").size(13))
        .on_press(Message::OpenQuickProjectDialog)
        .padding([4, 8]);
    let project_picker = row![
        text("所属项目")
            .size(13)
            .color(MUTED)
            .width(Length::Fixed(72.0)),
        PickList::new(
            project_choices(app),
            Some(ProjectChoice::of_id(dialog.project_id, &app.projects)),
            move |choice| { Message::DialogProjectChanged(choice.id) },
        )
        .placeholder("无项目")
        .text_size(13)
        .padding([4, 8])
        .width(Length::Fill),
        quick_btn,
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // 优先级（可选）：下拉
    let priority_row = row![
        text("优先级")
            .size(13)
            .color(MUTED)
            .width(Length::Fixed(72.0)),
        priority_picker(dialog.priority, Message::DialogPriorityChanged),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // 截止时间：文本输入（实时校验）+ 快捷下拉（回填后仍可手动修改）
    let due_row = row![
        text("截止时间")
            .size(13)
            .color(MUTED)
            .width(Length::Fixed(72.0)),
        text_input("2026-01-31 或 2026-01-31 18:30", &dialog.due_input)
            .on_input(Message::DialogDueChanged)
            .on_submit(Message::SubmitAddDialog)
            .padding(10)
            .width(Length::Fill),
        Space::new().width(6),
        PickList::new(
            &QUICK_DUE_OPTIONS[..],
            Option::<QuickDue>::None,
            Message::DialogQuickDue
        )
        .placeholder("快捷时间")
        .text_size(13)
        .padding([4, 8]),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // 表单主体；截止时间格式错误时追加红字提示
    let mut form = column![
        text("添加任务").size(20).font(BOLD),
        Space::new().height(4),
        form_field("标题", title_input),
        form_field("描述", description_input),
        project_picker,
        priority_row,
        due_row,
    ]
    .spacing(10);

    if let Err(hint) = &dialog.due_parsed {
        form = form.push(text(hint.as_str()).size(12).color(ERROR_COLOR));
    }

    // 按钮行：标题为空或截止时间非法时"创建"禁用
    let can_submit = !dialog.title.trim().is_empty() && dialog.due_parsed.is_ok();
    let actions = row![
        Space::new().width(Length::Fill),
        button(text("取消").size(14))
            .on_press(Message::CloseAddDialog)
            .padding([8, 18]),
        Space::new().width(8),
        button(text("创建").size(14))
            .on_press_maybe(can_submit.then_some(Message::SubmitAddDialog))
            .style(button::primary)
            .padding([8, 18]),
    ]
    .align_y(Alignment::Center);

    container(
        column![form, Space::new().height(2), actions]
            .spacing(10)
            .width(Length::Fixed(460.0)),
    )
    .padding(20)
    .style(card_style)
    .into()
}

// ---------- 弹窗添加项目 ----------

/// 项目弹窗卡片：名称（必填）+ 可选开始 / 结束时间 + 操作按钮。
fn project_dialog_card<'a>(app: &'a App) -> Element<'a, Message> {
    let dialog = app
        .project_dialog
        .as_ref()
        .expect("项目弹窗仅在弹窗打开时渲染");

    // 名称（必填）：回车提交
    let name_input = text_input("项目名称（必填）", &dialog.name)
        .id(PROJECT_DIALOG_NAME_ID)
        .on_input(Message::ProjectNameChanged)
        .on_submit(Message::SubmitProjectDialog)
        .padding(10);

    // 开始 / 结束时间（可选）：回车提交，实时解析校验
    let start_input = text_input("2026-01-31 或 2026-01-31 18:30", &dialog.start_input)
        .on_input(Message::ProjectStartChanged)
        .on_submit(Message::SubmitProjectDialog)
        .padding(10);
    let end_input = text_input("2026-01-31 或 2026-01-31 18:30", &dialog.end_input)
        .on_input(Message::ProjectEndChanged)
        .on_submit(Message::SubmitProjectDialog)
        .padding(10);

    // 派生校验（视图层实时反馈，update 层提交时再防御一次）
    let name = dialog.name.trim();
    let name_conflict = !name.is_empty() && app.projects.iter().any(|p| p.name == name);
    let range_invalid = match (&dialog.start_parsed, &dialog.end_parsed) {
        (Ok(Some(start)), Ok(Some(finish))) => start >= finish,
        _ => false,
    };

    // 表单主体；按需追加红字提示
    let mut form = column![
        text("添加项目").size(20).font(BOLD),
        Space::new().height(4),
        form_field("名称", name_input),
        form_field("开始时间", start_input),
        form_field("结束时间", end_input),
        form_field(
            "优先级",
            priority_picker(dialog.priority, Message::ProjectDialogPriorityChanged),
        ),
    ]
    .spacing(10);

    if let Err(hint) = &dialog.start_parsed {
        form = form.push(text(hint.as_str()).size(12).color(ERROR_COLOR));
    }
    if let Err(hint) = &dialog.end_parsed {
        form = form.push(text(hint.as_str()).size(12).color(ERROR_COLOR));
    }
    if name_conflict {
        form = form.push(text("项目名已存在").size(12).color(ERROR_COLOR));
    }
    if range_invalid {
        form = form.push(text("开始时间必须早于结束时间").size(12).color(ERROR_COLOR));
    }

    // 按钮行：名称为空 / 重名 / 时间非法 / 开始≥结束 时"创建"禁用
    let can_submit = !name.is_empty()
        && !name_conflict
        && dialog.start_parsed.is_ok()
        && dialog.end_parsed.is_ok()
        && !range_invalid;
    let actions = row![
        Space::new().width(Length::Fill),
        button(text("取消").size(14))
            .on_press(Message::CloseProjectDialog)
            .padding([8, 18]),
        Space::new().width(8),
        button(text("创建").size(14))
            .on_press_maybe(can_submit.then_some(Message::SubmitProjectDialog))
            .style(button::primary)
            .padding([8, 18]),
    ]
    .align_y(Alignment::Center);

    container(
        column![form, Space::new().height(2), actions]
            .spacing(10)
            .width(Length::Fixed(460.0)),
    )
    .padding(20)
    .style(card_style)
    .into()
}

// ---------- 已完成归档弹窗 ----------

/// 已完成任务归档弹窗：按完成时间降序（最近完成在前）展示紧凑行，可删除。
fn completed_dialog_card<'a>(app: &'a App) -> Element<'a, Message> {
    let mut done: Vec<&Todo> = app
        .todos
        .iter()
        .filter(|todo| todo.status() == TodoStatus::Done)
        .collect();
    // 最近完成在前
    done.sort_by_key(|todo| std::cmp::Reverse(todo.finished_at));

    let list: Element<'_, Message> = if done.is_empty() {
        text("暂无已完成任务").size(13).color(MUTED).into()
    } else {
        column(done.into_iter().map(|todo| done_row(todo, app)))
            .spacing(6)
            .padding(2)
            .into()
    };

    container(
        column![
            text("已完成任务").size(20).font(BOLD),
            Space::new().height(4),
            scrollable(list).height(Length::Fill),
            Space::new().height(2),
            row![
                Space::new().width(Length::Fill),
                button(text("关闭").size(14))
                    .on_press(Message::CloseCompletedDialog)
                    .padding([8, 18]),
            ],
        ]
        .spacing(10)
        .width(Length::Fixed(520.0))
        .height(Length::Fixed(480.0)),
    )
    .padding(20)
    .style(card_style)
    .into()
}

/// 归档弹窗中的紧凑行：标题 + 项目 / 完成时间 / 总耗时 + 删除。
fn done_row<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let project = todo
        .project_id
        .and_then(|id| app.projects.iter().find(|p| p.id == id))
        .map(|p| p.name.as_str())
        .unwrap_or("无项目");
    let finished = todo
        .finished_at
        .map(format_time)
        .unwrap_or_else(|| "—".into());
    let total = todo
        .duration(app.now)
        .map(format_duration)
        .unwrap_or_else(|| "—".into());

    row![
        column![
            text(todo.title.as_str()).size(14).font(BOLD),
            text(format!("{project} · 完成于 {finished} · 总耗时 {total}"))
                .size(11)
                .color(MUTED),
        ]
        .spacing(2)
        .width(Length::Fill),
        button(text("删除").size(12))
            .on_press(Message::DeleteTodo(todo.id))
            .style(button::danger)
            .padding([4, 10]),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

/// 弹窗表单里带小标签的一行（标签在上、输入框在下）。
fn form_field<'a>(label: &'a str, input: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(label).size(13).color(MUTED), input.into()]
        .spacing(4)
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

/// 任务区排序选择行（右对齐）：控制未开始 / 进行中两列的组内排序。
fn sort_picker_row(app: &App) -> Element<'_, Message> {
    row![
        Space::new().width(Length::Fill),
        text("排序").size(13).color(MUTED),
        Space::new().width(6),
        PickList::new(
            &SORT_MODES[..],
            Some(app.sort_mode),
            Message::SortModeChanged
        )
        .text_size(13)
        .padding([4, 8]),
    ]
    .align_y(Alignment::Center)
    .spacing(6)
    .into()
}

// ---------- 项目单行栏 ----------

/// 项目单行栏：项目标签 + 横向滚动芯片（「全部」+ 各项目，按项目排序偏好排序），
/// 选中具体项目时右端出现「编辑 / 删除」上下文按钮，最右端是项目排序下拉（滚动区外恒可见）。
/// 点击芯片筛选该项目的任务（「全部」恒在最前，显示全部任务）。
fn project_bar(app: &App) -> Element<'_, Message> {
    // 芯片区：「全部」恒在最前；无项目时灰字提示
    let mut chips = row![project_chip(app, None, None, "全部", app.todos.len())].spacing(8);

    if app.projects.is_empty() {
        chips = chips.push(
            container(text("暂无项目").size(12).color(MUTED))
                .padding([6, 2])
                .align_y(Alignment::Center),
        );
    } else {
        // 项目芯片按 project_sort_mode 排序（"全部"恒在最前）
        let mut projects: Vec<&Project> = app.projects.iter().collect();
        match app.project_sort_mode {
            SortMode::Priority => projects.sort_by_key(|p| p.priority_order_key()),
            SortMode::Due => projects.sort_by_key(|p| p.end_order_key()),
            SortMode::Combined => projects.sort_by_key(|p| p.combined_order_key()),
        }
        for project in projects {
            let count = app
                .todos
                .iter()
                .filter(|todo| todo.project_id == Some(project.id))
                .count();
            chips = chips.push(project_chip(
                app,
                Some(project.id),
                Some(project),
                &project.name,
                count,
            ));
        }
    }

    // 选中具体项目时：右端显示「编辑 / 删除」上下文按钮（「全部」无）
    let mut actions = row![].align_y(Alignment::Center).spacing(4);
    if let Some(project_id) = app.selected_project {
        actions = actions
            .push(
                button(text("编辑").size(12))
                    .on_press(Message::StartEditProject(project_id))
                    .style(button::text)
                    .padding([2, 8]),
            )
            .push(
                button(text("删除").size(12))
                    .on_press(Message::DeleteProject(project_id))
                    .style(button::text)
                    .padding([2, 8]),
            );
    }

    row![
        text("项目").size(13).font(BOLD),
        Space::new().width(10),
        scrollable(chips)
            .direction(scrollable::Direction::Horizontal(Default::default()))
            .width(Length::Fill)
            .height(Length::Shrink),
        actions,
        Space::new().width(6),
        text("排序").size(11).color(MUTED),
        PickList::new(
            &SORT_MODES[..],
            Some(app.project_sort_mode),
            Message::ProjectSortModeChanged,
        )
        .text_size(12)
        .padding([2, 6]),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into()
}

/// 单个项目芯片：优先级圆点（可选）+ 名称 + 计数；点击筛选，选中主色描边高亮；
/// 设置了起止时间的项目悬停显示时间段 tooltip。
fn project_chip<'a>(
    app: &'a App,
    id: Option<Uuid>,
    project: Option<&'a Project>,
    name: &'a str,
    count: usize,
) -> Element<'a, Message> {
    let selected = app.selected_project == id;

    let mut content = row![].align_y(Alignment::Center).spacing(6);
    // 优先级圆点：仅项目芯片（「全部」无归属项目）且设置了优先级时显示
    if let Some(priority) = project.and_then(|p| p.priority) {
        content = content.push(text("●").size(10).color(priority_color(priority)));
    }
    content = content
        .push(text(name).size(13))
        .push(text(count.to_string()).size(12).color(MUTED));

    let chip = button(content)
        .on_press(Message::SelectProject(id))
        .style(move |theme, status| project_chip_style(theme, status, selected))
        .padding([6, 12]);

    // 起止时间悬停提示（仅设置了时间的项目；「全部」无）
    match project_period(app, id) {
        Some(period) => tooltip(chip, text(period), tooltip::Position::Bottom).into(),
        None => chip.into(),
    }
}

/// 项目芯片样式：选中时主题主色描边 + 弱色底，否则卡片风格底；悬停时底色加深。
fn project_chip_style(
    theme: &iced::Theme,
    status: button::Status,
    selected: bool,
) -> button::Style {
    let palette = theme.extended_palette();
    let base = button::Style {
        background: Some(Background::Color(if selected {
            palette.primary.weak.color
        } else {
            palette.background.weak.color
        })),
        text_color: palette.background.base.text,
        border: Border {
            color: if selected {
                palette.primary.base.color
            } else {
                palette.background.strong.color
            },
            width: 1.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    };

    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(if selected {
                palette.primary.weak.color
            } else {
                palette.background.strong.color
            })),
            ..base
        },
        _ => base,
    }
}

// ---------- 项目编辑面板 ----------

/// 项目编辑面板：项目单行栏下方展开的全宽内联表单（名称 + 优先级 + 起止时间 + 保存 / 取消）。
/// 校验规则同弹窗（重名校验排除自身）；失败红字提示并保持面板打开（update 层防御）。
fn project_edit_panel(app: &App) -> Element<'_, Message> {
    let edit = app
        .project_edit
        .as_ref()
        .expect("编辑面板仅在 project_edit 命中时渲染");

    // 派生校验（视图实时反馈，update 层保存时再防御一次）
    let name = edit.name.trim();
    let name_conflict = !name.is_empty()
        && app
            .projects
            .iter()
            .any(|p| p.id != edit.project_id && p.name == name);
    let range_invalid = match (&edit.start_parsed, &edit.end_parsed) {
        (Ok(Some(start)), Ok(Some(finish))) => start >= finish,
        _ => false,
    };

    let mut form = column![
        row![
            column![
                text("名称").size(11).color(MUTED),
                text_input("项目名称", &edit.name)
                    .id(PROJECT_EDIT_NAME_ID)
                    .on_input(Message::ProjectEditNameChanged)
                    .on_submit(Message::SaveEditProject)
                    .padding(6),
            ]
            .spacing(2)
            .width(Length::Fill),
            Space::new().width(12),
            column![
                text("优先级").size(11).color(MUTED),
                priority_picker(edit.priority, Message::ProjectEditPriorityChanged),
            ]
            .spacing(2)
            .width(Length::Fixed(140.0)),
        ]
        .align_y(Alignment::End),
        row![
            labeled_input(
                "开始时间",
                "2026-01-31",
                &edit.start_input,
                Message::ProjectEditStartChanged,
            ),
            labeled_input(
                "结束时间",
                "2026-01-31",
                &edit.end_input,
                Message::ProjectEditEndChanged,
            ),
        ]
        .spacing(12),
    ]
    .spacing(8);

    if let Err(hint) = &edit.start_parsed {
        form = form.push(text(hint.as_str()).size(12).color(ERROR_COLOR));
    }
    if let Err(hint) = &edit.end_parsed {
        form = form.push(text(hint.as_str()).size(12).color(ERROR_COLOR));
    }
    if name_conflict {
        form = form.push(text("项目名已存在").size(12).color(ERROR_COLOR));
    }
    if range_invalid {
        form = form.push(text("开始须早于结束").size(12).color(ERROR_COLOR));
    }

    let can_submit = !name.is_empty()
        && !name_conflict
        && edit.start_parsed.is_ok()
        && edit.end_parsed.is_ok()
        && !range_invalid;

    let actions = row![
        Space::new().width(Length::Fill),
        button(text("保存").size(13))
            .on_press_maybe(can_submit.then_some(Message::SaveEditProject))
            .style(button::primary)
            .padding([6, 14]),
        Space::new().width(6),
        button(text("取消").size(13))
            .on_press(Message::CancelEditProject)
            .padding([6, 12]),
    ]
    .align_y(Alignment::Center);

    container(column![form, actions].spacing(8))
        .width(Length::Fill)
        .padding(12)
        .style(card_style)
        .into()
}

// ---------- 右下角归档入口 ----------

/// 右下角悬浮「已完成 (N)」按钮：叠加在内容区右下角（弹窗遮罩之下）。
fn completed_fab(app: &App) -> Element<'_, Message> {
    let count = app
        .todos
        .iter()
        .filter(|todo| todo.status() == TodoStatus::Done)
        .count();
    container(
        button(text(format!("已完成 ({count})")).size(13))
            .on_press(Message::OpenCompletedDialog)
            .style(button::secondary)
            .padding([8, 14]),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::End)
    .align_y(Alignment::End)
    .padding(16)
    .into()
}

/// 带小标签的窄输入框（项目编辑面板用），回车即保存。
fn labeled_input<'a>(
    label: &'a str,
    placeholder: &'a str,
    value: &'a str,
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    column![
        text(label).size(11).color(MUTED),
        text_input(placeholder, value)
            .on_input(on_input)
            .on_submit(Message::SaveEditProject)
            .padding(6),
    ]
    .spacing(2)
    .into()
}

/// 项目起止时间的短文本（芯片悬停 tooltip 用，仅显示已设置的一端）：
/// `01-01 ~ 01-31` / `开始 01-01` / `结束 01-31`；未设置返回 `None`。
fn project_period(app: &App, id: Option<Uuid>) -> Option<String> {
    let project = app.projects.iter().find(|p| Some(p.id) == id)?;
    match (project.started_at, project.finished_at) {
        (Some(start), Some(finish)) => {
            Some(format!("{} ~ {}", short_date(start), short_date(finish)))
        }
        (Some(start), None) => Some(format!("开始 {}", short_date(start))),
        (None, Some(finish)) => Some(format!("结束 {}", short_date(finish))),
        (None, None) => None,
    }
}

/// 短日期（月-日），本地时区展示。
fn short_date(dt: DateTime<Utc>) -> String {
    dt.with_timezone(&chrono::Local).format("%m-%d").to_string()
}

// ---------- 任务列表 ----------

/// 双列分组显示：左列=未开始、右列=进行中，各自独立滚动；
/// 组内按 `sort_mode` 排序（优先级 / 截止日期 / 综合，未设置均排最后）；已完成任务不在此显示（见归档弹窗）。
fn grouped_columns<'a>(app: &'a App, todos: Vec<&'a Todo>) -> Element<'a, Message> {
    if todos.is_empty() {
        return empty_hint(if app.selected_project.is_some() {
            "该项目暂无任务"
        } else {
            "暂无任务，先添加一个吧"
        });
    }

    let mut pending: Vec<&Todo> = todos
        .iter()
        .copied()
        .filter(|todo| todo.status() == TodoStatus::Pending)
        .collect();
    let mut in_progress: Vec<&Todo> = todos
        .iter()
        .copied()
        .filter(|todo| todo.status() == TodoStatus::InProgress)
        .collect();

    // 组内排序：按当前选择的排序方式（稳定排序，未设置均排最后）
    match app.sort_mode {
        SortMode::Priority => {
            pending.sort_by_key(|todo| todo.priority_order_key());
            in_progress.sort_by_key(|todo| todo.priority_order_key());
        }
        SortMode::Due => {
            pending.sort_by_key(|todo| todo.due_order_key());
            in_progress.sort_by_key(|todo| todo.due_order_key());
        }
        SortMode::Combined => {
            pending.sort_by_key(|todo| todo.combined_order_key());
            in_progress.sort_by_key(|todo| todo.combined_order_key());
        }
    }

    row![
        group_column("未开始", "暂无未开始任务", pending, app),
        Space::new().width(12),
        group_column("进行中", "暂无进行中任务", in_progress, app),
    ]
    .height(Length::Fill)
    .into()
}

/// 单个分组列：标题 + 计数 + 可滚动卡片列表；空组显示提示。
fn group_column<'a>(
    title: &'static str,
    empty: &'static str,
    todos: Vec<&'a Todo>,
    app: &'a App,
) -> Element<'a, Message> {
    let count = todos.len();
    let list: Element<'_, Message> = if todos.is_empty() {
        empty_hint(empty)
    } else {
        column(todos.into_iter().map(|todo| todo_card(todo, app)))
            .spacing(8)
            .padding(4)
            .into()
    };

    column![
        text(format!("{title} ({count})")).size(14).font(BOLD),
        Space::new().height(4),
        scrollable(list).height(Length::Fill),
    ]
    .spacing(4)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// 单个任务的卡片：标题 + 可选描述 + 状态徽章 + 操作按钮 + 时间元信息。
/// 默认全部属性只读展示；该卡片处于编辑模式时（"当前任务"）渲染可编辑表单。
fn todo_card<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    if app
        .todo_edit
        .as_ref()
        .is_some_and(|edit| edit.todo_id == todo.id)
    {
        return todo_card_editor(todo, app);
    }

    let mut head = row![
        text(todo.title.as_str()).size(16).font(BOLD),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .spacing(8);
    // 优先级徽章：未设置不显示
    if let Some(priority) = todo.priority {
        head = head.push(
            text(priority.label())
                .size(11)
                .color(priority_color(priority)),
        );
    }
    head = head
        .push(
            text(todo.status().label())
                .size(12)
                .color(status_color(todo.status())),
        )
        .push(actions(todo));

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
    // 编辑按钮：卡片右下角（进入编辑模式，即"当前任务"）
    content = content.push(
        row![
            Space::new().width(Length::Fill),
            button(text("编辑").size(13))
                .on_press(Message::EditTodo(todo.id))
                .style(button::text)
                .padding([4, 10]),
        ]
        .align_y(Alignment::Center),
    );

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

/// 操作按钮：按状态显示"开始 / 完成"，始终有"删除"（「编辑」在卡片右下角）。
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

// ---------- 卡片编辑模式 ----------

/// 可编辑模式的卡片（即"当前任务"）：标题 / 描述 / 项目 / 截止时间可修改，
/// 保存校验同弹窗；时间字段保持只读展示；主题主色描边与普通卡片区分。
fn todo_card_editor<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let edit = app
        .todo_edit
        .as_ref()
        .expect("编辑模式仅在 todo_edit 命中该卡片时渲染");

    // 标题（必填）：回车保存
    let title_input = text_input("任务标题（必填）", &edit.title)
        .on_input(Message::EditTitleChanged)
        .on_submit(Message::SaveEditTodo)
        .padding(8)
        .width(Length::Fill);

    // 描述（可选）：回车保存
    let description_input = text_input("任务描述（可选）", &edit.description)
        .on_input(Message::EditDescriptionChanged)
        .on_submit(Message::SaveEditTodo)
        .padding(8);

    // 所属项目：复用卡片上的选项包装（"无项目" + 全部项目）
    let project_picker = row![
        text("项目")
            .size(13)
            .color(MUTED)
            .width(Length::Fixed(72.0)),
        PickList::new(
            project_choices(app),
            Some(ProjectChoice::of_id(edit.project_id, &app.projects)),
            move |choice| Message::EditProjectChanged(choice.id),
        )
        .placeholder("无项目")
        .text_size(13)
        .padding([4, 8])
        .width(Length::Fill),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // 优先级（可选）：下拉
    let priority_row = row![
        text("优先级")
            .size(13)
            .color(MUTED)
            .width(Length::Fixed(72.0)),
        priority_picker(edit.priority, Message::EditPriorityChanged),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // 截止时间：文本输入（实时校验）+ 快捷下拉（回填后仍可手动修改）
    let due_row = row![
        text("截止时间")
            .size(13)
            .color(MUTED)
            .width(Length::Fixed(72.0)),
        text_input("2026-01-31 或 2026-01-31 18:30", &edit.due_input)
            .on_input(Message::EditDueChanged)
            .on_submit(Message::SaveEditTodo)
            .padding(8)
            .width(Length::Fill),
        Space::new().width(6),
        PickList::new(
            &QUICK_DUE_OPTIONS[..],
            Option::<QuickDue>::None,
            Message::EditQuickDue,
        )
        .placeholder("快捷时间")
        .text_size(13)
        .padding([4, 8]),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // 头部：标题输入 + 状态徽章（保存/取消在卡片底部右下角）
    let head = row![
        title_input,
        Space::new().width(8),
        text(todo.status().label())
            .size(12)
            .color(status_color(todo.status())),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    // 表单主体；截止时间格式错误时追加红字提示
    let mut form = column![
        head,
        description_input,
        project_picker,
        priority_row,
        due_row
    ]
    .spacing(8);
    if let Err(hint) = &edit.due_parsed {
        form = form.push(text(hint.as_str()).size(12).color(ERROR_COLOR));
    }

    // 底部操作行：保存 / 取消（与只读卡片的「编辑」按钮位置对称）
    let can_submit = !edit.title.trim().is_empty() && edit.due_parsed.is_ok();
    let actions = row![
        Space::new().width(Length::Fill),
        button(text("保存").size(13))
            .on_press_maybe(can_submit.then_some(Message::SaveEditTodo))
            .style(button::primary)
            .padding([6, 14]),
        Space::new().width(6),
        button(text("取消").size(13))
            .on_press(Message::CancelEditTodo)
            .padding([6, 10]),
    ]
    .align_y(Alignment::Center);

    container(column![form, actions, time_meta_rows(todo, app)].spacing(8))
        .width(Length::Fill)
        .padding(12)
        .style(editor_card_style)
        .into()
}

/// 编辑模式卡片样式：主题主色描边，与普通卡片区分。
fn editor_card_style(theme: &iced::Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.primary.base.color,
            width: 1.5,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

// ---------- 任务元信息 ----------

/// 项目下拉的选项（"无项目" + 全部项目），弹窗与编辑模式共用。
fn project_choices(app: &App) -> Vec<ProjectChoice> {
    std::iter::once(ProjectChoice::none())
        .chain(app.projects.iter().map(ProjectChoice::of))
        .collect()
}

/// 任务归属的只读展示行：项目名（未归属显示"无项目"）。
/// 项目归属只能在编辑模式下修改（R15）。
fn project_row_readonly<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let (name, color) = match todo
        .project_id
        .and_then(|id| app.projects.iter().find(|p| p.id == id))
    {
        Some(project) => (project.name.as_str(), MUTED),
        None => ("无项目", MUTED),
    };
    time_row("项目", name.into(), color)
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

    /// 按项目 id 构造选项（项目已被删除时回落为"无项目"）。
    fn of_id(id: Option<Uuid>, projects: &[Project]) -> Self {
        match id.and_then(|id| projects.iter().find(|p| p.id == id)) {
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

impl std::fmt::Display for QuickDue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// 时间元信息：截止时间 + 创建 / 开始 / 结束；进行中附实时耗时，已完成附总耗时。
/// （不含项目行：普通模式由 `project_row_readonly` 展示，编辑模式用下拉。）
fn time_meta_rows<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let mut meta = column![].spacing(3);

    // 截止时间：已逾期且未完成的任务标红提示
    if let Some(due) = todo.due_at {
        let overdue = todo.status() != TodoStatus::Done && due < app.now;
        meta = meta.push(time_row(
            "截止时间",
            format_time(due),
            if overdue { ERROR_COLOR } else { MUTED },
        ));
    }

    meta = meta.push(time_row("创建时间", format_time(todo.created_at), MUTED));

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

/// 普通模式的任务元信息：归属只读行 + 时间行。
fn meta_rows<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    column![project_row_readonly(todo, app), time_meta_rows(todo, app)]
        .spacing(3)
        .into()
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
