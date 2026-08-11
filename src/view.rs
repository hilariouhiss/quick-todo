//! 视图：把应用状态渲染成 iced 元素树。
//!
//! 布局：标题栏（分体按钮「＋ 添加任务 ▾」，下拉菜单含「＋ 添加项目」）+ 项目单行栏
//! （左侧排序下拉 + 横向滚动芯片）+ 任务区（统一标题行：两列计数 + 右上角排序下拉 + 双列），
//! 项目栏与任务区以卡片容器分组；底部 footer（footer）：左下角主题指示器
//! （Theme: Auto/Light/Dark，胶囊外壳，点击循环切换）+ 右下角统计胶囊
//! （共 x 项 | 进行中 x | 已完成 x，「已完成 x」为主色粗体链接，点击打开归档弹窗）。
//! 视觉规范统一由 `view/tokens.rs`「设计令牌」常量控制（字号 / 间距 / 圆角 / 按钮规格），
//! 颜色来自 `view/theme.rs` 调色板 + `extended_palette()` 主题自适应（浅 / 深主题均可读）。
//! 每个任务一张卡片：标题、状态徽章、操作按钮、只读属性展示（含项目归属），
//! 以及创建 / 开始 / 结束三个时间点与耗时（两级粒度，不逐秒跳动）；「编辑」按钮在卡片右下角。

pub(crate) mod theme;
pub(crate) mod tokens;

use chrono::{DateTime, Duration, Utc};
use iced::alignment::Vertical as AlignVertical;
use iced::font::Weight;
use iced::mouse;
use iced::widget::canvas::{self, Cache, Canvas, Geometry, Path, Stroke, Text as CanvasText};
use iced::widget::{
    PickList, Space, button, column, container, mouse_area, opaque, pick_list, row, scrollable,
    stack, text, text_input, tooltip,
};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Point, Rectangle, Size};
use uuid::Uuid;

use crate::model::{
    App, Priority, Project, QuickDue, SortMode, StatsDimension, Todo, TodoStatus, TodoType,
};
use crate::stats::{self, Bucket};
use crate::update::Message;
use crate::validate;
use theme::{SemColors, extended, sem, sem_colors};
use tokens::*;

// ---------- 选项常量 ----------

/// 弹窗截止时间的快捷选项（选中后回填到文本输入框，仍可手动修改）
const QUICK_DUE_OPTIONS: [QuickDue; 3] = [QuickDue::Today, QuickDue::Tomorrow, QuickDue::Sunday];

/// 排序方式下拉的固定选项（任务区与项目栏共用）。
const SORT_MODES: [SortMode; 3] = [SortMode::Priority, SortMode::Due, SortMode::Combined];

/// 统计维度下拉的固定选项（周 / 月 / 年 / 项目）。
const STATS_DIMENSIONS: [StatsDimension; 4] = [
    StatsDimension::Week,
    StatsDimension::Month,
    StatsDimension::Year,
    StatsDimension::Project,
];

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
    .text_size(FONT_HEADER)
    .padding([SPACE_XS, SPACE_M])
    .style(pick_list_style)
    .width(Length::Fill)
    .into()
}

/// 所属项目下拉行（任务弹窗 / 卡片编辑模式共用；`with_quick_new` = 弹窗带「＋ 新建」
/// 快速新建项目入口，点击弹出与标题栏相同的新建项目弹窗，创建成功自动选中）。
fn project_picker_row<'a>(
    app: &'a App,
    label: &'static str,
    selected: Option<Uuid>,
    on_select: fn(Option<Uuid>) -> Message,
    with_quick_new: bool,
) -> Element<'a, Message> {
    let mut row = row![
        text(label)
            .size(FONT_BODY)
            .color(sem(app).muted)
            .width(Length::Fixed(LABEL_WIDTH)),
        PickList::new(
            project_choices(app),
            Some(ProjectChoice::of_id(selected, &app.projects)),
            move |choice| on_select(choice.id),
        )
        .placeholder("无项目")
        .text_size(FONT_HEADER)
        .padding([SPACE_XS, SPACE_M])
        .style(pick_list_style)
        .width(Length::Fill),
    ]
    .spacing(SPACE_S)
    .align_y(Alignment::Center);
    if with_quick_new {
        row = row.push(
            button(text("＋ 新建").size(FONT_BODY))
                .on_press(Message::OpenQuickProjectDialog)
                .padding([4, 8]),
        );
    }
    row.into()
}

/// 任务类型下拉行（任务弹窗 / 卡片编辑模式共用；`with_quick_new` = 弹窗带「＋ 新建」
/// 快速新建类型入口，点击弹出与标题栏相同的新建类型弹窗，创建成功自动选中）。
fn type_picker_row<'a>(
    app: &'a App,
    label: &'static str,
    selected: Option<Uuid>,
    on_select: fn(Option<Uuid>) -> Message,
    with_quick_new: bool,
) -> Element<'a, Message> {
    let mut row = row![
        text(label)
            .size(FONT_BODY)
            .color(sem(app).muted)
            .width(Length::Fixed(LABEL_WIDTH)),
        PickList::new(
            type_choices(app),
            Some(TypeChoice::of_id(selected, &app.types)),
            move |choice| on_select(choice.id),
        )
        .placeholder("无类型")
        .text_size(FONT_HEADER)
        .padding([SPACE_XS, SPACE_M])
        .style(pick_list_style)
        .width(Length::Fill),
    ]
    .spacing(SPACE_S)
    .align_y(Alignment::Center);
    if with_quick_new {
        row = row.push(
            button(text("＋ 新建").size(FONT_BODY))
                .on_press(Message::OpenQuickTypeDialog)
                .padding([4, 8]),
        );
    }
    row.into()
}

/// 优先级下拉行（任务弹窗 / 卡片编辑模式共用）。
fn priority_row<'a>(
    app: &'a App,
    selected: Option<Priority>,
    on_select: fn(Option<Priority>) -> Message,
) -> Element<'a, Message> {
    row![
        text("优先级")
            .size(FONT_BODY)
            .color(sem(app).muted)
            .width(Length::Fixed(LABEL_WIDTH)),
        priority_picker(selected, on_select),
    ]
    .spacing(SPACE_S)
    .align_y(Alignment::Center)
    .into()
}

/// 截止时间行：文本输入（实时校验）+ 快捷下拉（回填后仍可手动修改）。
/// 任务弹窗 / 卡片编辑模式共用；`on_submit` 为回车提交消息，`padding` 为输入框内边距。
fn due_row<'a>(
    app: &'a App,
    input: &'a str,
    on_input: fn(String) -> Message,
    on_quick: fn(QuickDue) -> Message,
    on_submit: Message,
    padding: impl Into<iced::Padding>,
) -> Element<'a, Message> {
    row![
        text("截止时间")
            .size(FONT_BODY)
            .color(sem(app).muted)
            .width(Length::Fixed(LABEL_WIDTH)),
        text_input("2026-01-31 或 2026-01-31 18:30", input)
            .style(text_input_style)
            .on_input(on_input)
            .on_submit(on_submit)
            .padding(padding)
            .width(Length::Fill),
        Space::new().width(SPACE_S),
        PickList::new(&QUICK_DUE_OPTIONS[..], Option::<QuickDue>::None, on_quick)
            .placeholder("快捷时间")
            .text_size(FONT_HEADER)
            .padding([SPACE_XS, SPACE_M])
            .style(pick_list_style),
    ]
    .spacing(SPACE_S)
    .align_y(Alignment::Center)
    .into()
}

/// 优先级徽章 / 圆点颜色：高=红、中=橙、低=灰（取自当前主题语义色）。
fn priority_color(sem: SemColors, priority: Priority) -> Color {
    match priority {
        Priority::High => sem.error,
        Priority::Medium => sem.accent,
        Priority::Low => sem.muted,
    }
}

/// 下拉选择器样式：与设计令牌统一——弱色底 + 卡片圆角 + 主题描边；悬停 / 展开时底色加深。
/// 深浅主题均取 `extended_palette()` 自适应。
fn pick_list_style(theme: &iced::Theme, status: pick_list::Status) -> pick_list::Style {
    let background = theme.extended_palette().background;
    let muted = sem_colors(theme.extended_palette().is_dark).muted;
    let base = pick_list::Style {
        text_color: background.base.text,
        placeholder_color: background.base.text,
        handle_color: muted,
        background: background.weak.color.into(),
        border: Border {
            color: background.strong.color,
            width: 1.0,
            radius: RADIUS_CARD.into(),
        },
    };
    match status {
        pick_list::Status::Active => base,
        pick_list::Status::Hovered | pick_list::Status::Opened { .. } => pick_list::Style {
            background: background.strong.color.into(),
            ..base
        },
    }
}

/// 文本输入框样式：与下拉选择器同款——弱色底 + 卡片圆角 + 主题描边；聚焦时主色描边（1.5px）。
/// placeholder / 正文 / 选中文本色均取自主题派生，深浅主题自适应。
fn text_input_style(theme: &iced::Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    let muted = sem_colors(palette.is_dark).muted;
    let base = text_input::Style {
        background: palette.background.weak.color.into(),
        border: Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: RADIUS_CARD.into(),
        },
        icon: muted,
        placeholder: muted,
        value: palette.background.base.text,
        selection: palette.primary.weak.color,
    };
    match status {
        text_input::Status::Focused { .. } => text_input::Style {
            border: Border {
                color: palette.primary.base.color,
                width: 1.5,
                radius: RADIUS_CARD.into(),
            },
            ..base
        },
        _ => base,
    }
}

const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};

/// 应用主视图：标题栏（分体按钮 + 下拉菜单）+ 项目单行栏 + 双列任务区；底部角落条（左下主题指示器 + 右下统计胶囊）。
/// 任一弹窗（任务添加 / 项目添加编辑 / 已完成归档）打开时，叠加模态遮罩与弹窗卡片。
pub fn view(app: &App) -> Element<'_, Message> {
    // 标题栏：左侧标题；右端分体按钮（「＋ 添加任务」主按钮 + 「▾」下拉箭头，
    // 下拉菜单含「＋ 添加项目」入口）；主题指示器与任务统计位于底部 footer
    let header = row![
        text("待办清单").size(FONT_TITLE).font(BOLD),
        Space::new().width(Length::Fill),
        row![
            button(text("＋ 添加任务").size(FONT_BODY))
                .on_press(Message::OpenAddDialog)
                .style(button::primary)
                .padding(BTN_MEDIUM)
                .width(Length::Fixed(ADD_MAIN_WIDTH)),
            button(text("▾").size(FONT_BODY))
                .on_press(Message::ToggleAddMenu)
                .style(button::primary)
                .padding(BTN_ARROW)
                .width(Length::Fixed(ADD_ARROW_WIDTH)),
        ]
        .spacing(1)
        .align_y(Alignment::Center),
    ]
    .align_y(Alignment::Center);

    let mut body = column![header].spacing(SPACE_L).height(Length::Fill);

    // 加载 / 保存错误：弱底红字横幅（底 = background.weak，文字 = danger 色，可读性 ≥ 4.5:1）
    if let Some(error) = &app.error {
        let s = sem(app);
        body = body.push(
            container(text(error.as_str()).size(FONT_SMALL).color(s.error))
                .width(Length::Fill)
                .padding([SPACE_S, SPACE_M])
                .style(error_banner_style),
        );
    }

    // 项目单行栏（任务列表上方；点击芯片筛选该项目任务）——卡片容器分组
    body = body.push(
        container(project_bar(app))
            .width(Length::Fill)
            .padding(PADDING_PANEL)
            .style(card_style),
    );

    // 项目编辑面板：选中项目点「编辑」后在项目栏下方展开（防御：项目已删除则不渲染）
    if app
        .project_edit
        .as_ref()
        .is_some_and(|edit| app.projects.iter().any(|p| p.id == edit.project_id))
    {
        body = body.push(project_edit_panel(app));
    }

    // 类型单行栏（项目栏下方；点击芯片筛选该类型任务）——卡片容器分组
    body = body.push(
        container(type_bar(app))
            .width(Length::Fill)
            .padding(PADDING_PANEL)
            .style(card_style),
    );

    // 类型编辑面板：选中类型点「编辑」后在类型栏下方展开（防御：类型已删除则不渲染）
    if app
        .type_edit
        .as_ref()
        .is_some_and(|edit| app.types.iter().any(|t| t.id == edit.type_id))
    {
        body = body.push(type_edit_panel(app));
    }

    // 任务区（统一标题行 + 双列）——卡片容器分组（整区空也有容器框，无视觉跳变）；
    // 项目筛选与类型筛选 AND 叠加（「全部」仅清除对应维度）
    let visible: Vec<&Todo> = app
        .todos
        .iter()
        .filter(|todo| app.selected_project.is_none() || todo.project_id == app.selected_project)
        .filter(|todo| app.selected_type.is_none() || todo.type_id == app.selected_type)
        .collect();
    body = body.push(
        container(grouped_columns(app, visible))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(PADDING_PANEL)
            .style(card_style),
    );

    // 底部 footer：主题指示器 + 统计胶囊（常规流布局，位于任务列表卡片容器下方，不悬浮）
    body = body.push(footer(app));

    let base = container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(PADDING_PAGE)
        .center_x(Length::Fill);

    // 下拉菜单展开时：叠加 透明捕获层（点击外部关闭，无压暗）+ 右上角菜单卡片（最顶层）
    let content: Element<'_, Message> = if app.add_menu_open {
        stack![
            base,
            mouse_area(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::CloseActiveDialog),
            container(opaque(add_menu_card()))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(Alignment::Start)
                .padding(iced::Padding {
                    top: ADD_MENU_TOP,
                    right: PADDING_PAGE,
                    bottom: 0.0,
                    left: 0.0,
                }),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    } else {
        base.into()
    };

    // 弹窗打开时：内容之上叠加 遮罩（点击关闭）+ 弹窗卡片（不透明，防穿透）
    // 任务 / 项目 / 类型 / 归档 / 统计弹窗互斥；任务弹窗内「＋ 新建」会叠加打开
    // 类型 / 项目弹窗（顶层）——因此类型 / 项目弹窗优先渲染，Esc / 遮罩关闭后返回
    // 任务弹窗（update 层保证）
    let dialog = if app.type_dialog.is_some() {
        Some(type_dialog_card(app))
    } else if app.project_dialog.is_some() {
        Some(project_dialog_card(app))
    } else if app.add_dialog.is_some() {
        Some(add_dialog_card(app))
    } else if app.show_completed {
        Some(completed_dialog_card(app))
    } else if app.show_stats {
        Some(stats_dialog_card(app))
    } else {
        None
    };
    match dialog {
        Some(card) => modal_overlay(content, card),
        None => content,
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
        background: Some(Background::Color(SCRIM_COLOR)),
        ..Default::default()
    }
}

// ---------- 弹窗添加任务 ----------

/// 弹窗卡片：标题 / 描述 / 所属项目 / 截止时间（输入 + 快捷下拉）+ 操作按钮。
fn add_dialog_card<'a>(app: &'a App) -> Element<'a, Message> {
    let dialog = app.add_dialog.as_ref().expect("弹窗卡片仅在弹窗打开时渲染");
    let s = sem(app);

    // 标题（必填）：回车提交
    let title_input = text_input("任务标题（必填）", &dialog.title)
        .id(DIALOG_TITLE_ID)
        .style(text_input_style)
        .on_input(Message::DialogTitleChanged)
        .on_submit(Message::SubmitAddDialog)
        .padding(10);

    // 描述（可选）：回车提交
    let description_input = text_input("任务描述（可选）", &dialog.description)
        .style(text_input_style)
        .on_input(Message::DialogDescriptionChanged)
        .on_submit(Message::SubmitAddDialog)
        .padding(10);

    // 所属项目 / 类型 / 优先级 / 截止时间：与卡片编辑模式共用的表单行组件
    // （弹窗带「＋ 新建」快速入口：点击弹出与标题栏相同的新建弹窗，创建成功自动选中）
    let project_picker = project_picker_row(
        app,
        "所属项目",
        dialog.project_id,
        Message::DialogProjectChanged,
        true,
    );
    let type_picker = type_picker_row(
        app,
        "类型",
        dialog.type_id,
        Message::DialogTypeChanged,
        true,
    );
    let priority_row = priority_row(app, dialog.priority, Message::DialogPriorityChanged);
    let due_row = due_row(
        app,
        &dialog.due.input,
        Message::DialogDueChanged,
        Message::DialogQuickDue,
        Message::SubmitAddDialog,
        10,
    );

    // 表单主体；截止时间格式错误时追加红字提示
    let mut form = column![
        text("添加任务").size(FONT_DIALOG_TITLE).font(BOLD),
        Space::new().height(SPACE_XS),
        form_field(app, "标题", title_input),
        form_field(app, "描述", description_input),
        project_picker,
        type_picker,
        priority_row,
        due_row,
    ]
    .spacing(SPACE_L);

    if let Err(hint) = &dialog.due.parsed {
        form = form.push(text(hint.as_str()).size(FONT_SMALL).color(s.error));
    }

    // 按钮行：标题为空或截止时间非法时"创建"禁用（单一来源 validate 模块）
    let issues = validate::todo_form_issues(
        &dialog.title,
        &dialog.due.parsed,
        dialog.project_id,
        dialog.type_id,
        &app.projects,
        &app.types,
    );
    let can_submit = validate::can_submit_todo(&issues);
    let actions = row![
        Space::new().width(Length::Fill),
        button(text("取消").size(FONT_HEADER))
            .on_press(Message::CloseAddDialog)
            .padding(BTN_LARGE),
        Space::new().width(SPACE_M),
        button(text("创建").size(FONT_HEADER))
            .on_press_maybe(can_submit.then_some(Message::SubmitAddDialog))
            .style(button::primary)
            .padding(BTN_LARGE),
    ]
    .align_y(Alignment::Center);

    container(
        column![form, Space::new().height(SPACE_XXS), actions]
            .spacing(SPACE_L)
            .width(Length::Fixed(DIALOG_WIDTH)),
    )
    .padding(PADDING_DIALOG)
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
        .style(text_input_style)
        .on_input(Message::ProjectNameChanged)
        .on_submit(Message::SubmitProjectDialog)
        .padding(10);

    // 开始 / 结束时间（可选）：回车提交，实时解析校验
    let start_input = text_input("2026-01-31 或 2026-01-31 18:30", &dialog.start.input)
        .style(text_input_style)
        .on_input(Message::ProjectStartChanged)
        .on_submit(Message::SubmitProjectDialog)
        .padding(10);
    let end_input = text_input("2026-01-31 或 2026-01-31 18:30", &dialog.end.input)
        .style(text_input_style)
        .on_input(Message::ProjectEndChanged)
        .on_submit(Message::SubmitProjectDialog)
        .padding(10);

    // 派生校验（单一来源 validate 模块；视图层实时反馈，update 层提交时再防御一次）
    let issues = validate::project_form_issues(
        &dialog.name,
        None,
        &dialog.start.parsed,
        &dialog.end.parsed,
        &app.projects,
    );

    // 表单主体；按需追加红字提示
    let mut form = column![
        text("添加项目").size(FONT_DIALOG_TITLE).font(BOLD),
        Space::new().height(SPACE_XS),
        form_field(app, "名称", name_input),
        form_field(app, "开始时间", start_input),
        form_field(app, "结束时间", end_input),
        form_field(
            app,
            "优先级",
            priority_picker(dialog.priority, Message::ProjectDialogPriorityChanged),
        ),
    ]
    .spacing(SPACE_L);

    // 校验错误红字提示（弹窗 / 编辑面板共用的 helper）
    form = push_project_form_errors(
        form,
        app,
        &dialog.start.parsed,
        &dialog.end.parsed,
        &issues,
        "开始时间必须早于结束时间",
    );

    // 按钮行：名称为空 / 重名 / 时间非法 / 开始≥结束 时"创建"禁用
    let can_submit = validate::can_submit_project(&issues);
    let actions = row![
        Space::new().width(Length::Fill),
        button(text("取消").size(FONT_HEADER))
            .on_press(Message::CloseProjectDialog)
            .padding(BTN_LARGE),
        Space::new().width(SPACE_M),
        button(text("创建").size(FONT_HEADER))
            .on_press_maybe(can_submit.then_some(Message::SubmitProjectDialog))
            .style(button::primary)
            .padding(BTN_LARGE),
    ]
    .align_y(Alignment::Center);

    container(
        column![form, Space::new().height(SPACE_XXS), actions]
            .spacing(SPACE_L)
            .width(Length::Fixed(DIALOG_WIDTH)),
    )
    .padding(PADDING_DIALOG)
    .style(card_style)
    .into()
}

// ---------- 弹窗添加类型 ----------

/// 类型弹窗卡片：名称（必填）+ 操作按钮（无优先级 / 无起止时间）。
fn type_dialog_card<'a>(app: &'a App) -> Element<'a, Message> {
    let dialog = app
        .type_dialog
        .as_ref()
        .expect("类型弹窗仅在弹窗打开时渲染");

    // 名称（必填）：回车提交
    let name_input = text_input("类型名称（必填）", &dialog.name)
        .id(TYPE_DIALOG_NAME_ID)
        .style(text_input_style)
        .on_input(Message::TypeNameChanged)
        .on_submit(Message::SubmitTypeDialog)
        .padding(10);

    // 派生校验（单一来源 validate 模块；视图层实时反馈，update 层提交时再防御一次）
    let issues = validate::type_form_issues(&dialog.name, None, &app.types);

    let mut form = column![
        text("添加类型").size(FONT_DIALOG_TITLE).font(BOLD),
        Space::new().height(SPACE_XS),
        form_field(app, "名称", name_input),
    ]
    .spacing(SPACE_L);

    // 重名红字提示（空白名仅禁用按钮，不提示）
    if issues.name_conflict {
        form = form.push(text("类型名已存在").size(FONT_SMALL).color(sem(app).error));
    }

    // 按钮行：名称为空 / 重名时"创建"禁用
    let can_submit = validate::can_submit_type(&issues);
    let actions = row![
        Space::new().width(Length::Fill),
        button(text("取消").size(FONT_HEADER))
            .on_press(Message::CloseTypeDialog)
            .padding(BTN_LARGE),
        Space::new().width(SPACE_M),
        button(text("创建").size(FONT_HEADER))
            .on_press_maybe(can_submit.then_some(Message::SubmitTypeDialog))
            .style(button::primary)
            .padding(BTN_LARGE),
    ]
    .align_y(Alignment::Center);

    container(
        column![form, Space::new().height(SPACE_XXS), actions]
            .spacing(SPACE_L)
            .width(Length::Fixed(DIALOG_WIDTH)),
    )
    .padding(PADDING_DIALOG)
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
        text("暂无已完成任务")
            .size(FONT_BODY)
            .color(sem(app).muted)
            .into()
    } else {
        column(done.into_iter().map(|todo| done_row(todo, app)))
            .spacing(SPACE_S)
            .padding(2)
            .into()
    };

    container(
        column![
            text("已完成任务").size(FONT_DIALOG_TITLE).font(BOLD),
            Space::new().height(SPACE_XS),
            scrollable(list).height(Length::Fill),
            Space::new().height(SPACE_XXS),
            row![
                Space::new().width(Length::Fill),
                button(text("关闭").size(FONT_HEADER))
                    .on_press(Message::CloseCompletedDialog)
                    .padding(BTN_LARGE),
            ],
        ]
        .spacing(SPACE_L)
        .width(Length::Fixed(DIALOG_WIDTH))
        .height(Length::Fixed(DIALOG_HEIGHT)),
    )
    .padding(PADDING_DIALOG)
    .style(card_style)
    .into()
}

// ---------- 统计弹窗 ----------

/// 统计弹窗卡片：标题行（完成统计 + 维度下拉 + 关闭）+ 汇总数字行 + 图表区
/// （scrollable 兜底；两张 canvas 图表——周 / 月 / 年纵柱状图、项目横条形图）。
fn stats_dialog_card<'a>(app: &'a App) -> Element<'a, Message> {
    let s = sem(app);
    let now = app.now;

    // 汇总（全局，不随维度变化）
    let totals = stats::totals(&app.todos, now);

    // 图表桶（按维度生成一次，两张图共用）
    let buckets: Vec<Bucket> = match app.stats_dimension {
        StatsDimension::Week => stats::week_buckets(&app.todos, now, 12),
        StatsDimension::Month => stats::month_buckets(&app.todos, now, 12),
        StatsDimension::Year => stats::year_buckets(&app.todos, now),
        StatsDimension::Project => stats::project_buckets(&app.todos, &app.projects, now),
    };

    // 空态：完成数全 0（与耗时无关——缺 started_at 的任务仍计入数量）
    let empty = buckets.iter().all(|b| b.count == 0);

    // 图表区：两张图（周期维度 = 纵柱状图；项目维度 = 横条形图）
    let charts: Element<'_, Message> = if empty {
        container(text("暂无已完成任务").size(FONT_BODY).color(s.muted))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    } else {
        let (kind, count_title, duration_title) = match app.stats_dimension {
            StatsDimension::Project => (ChartKind::Horizontal, "各项目完成数", "各项目总耗时"),
            _ => (ChartKind::Vertical, "完成数量", "总耗时"),
        };
        column![
            stats_chart(kind, count_title, buckets.clone(), count_value, false, app),
            stats_chart(kind, duration_title, buckets, duration_value, true, app),
        ]
        .spacing(SPACE_M)
        .into()
    };

    // 汇总数字行：第一行 总完成 · 总耗时 · 平均耗时；第二行 最长（含任务名截断）
    let (longest_duration, longest_title) = match &totals.longest {
        Some((_, title, d)) => (format_duration(*d), truncate(title, 12)),
        None => ("—".into(), "—".into()),
    };
    let summary = column![
        row![
            text("总完成").size(FONT_SMALL).color(s.muted),
            text(totals.done_count.to_string())
                .size(FONT_SMALL)
                .font(BOLD),
            text("·").size(FONT_SMALL).color(s.muted),
            text("总耗时").size(FONT_SMALL).color(s.muted),
            text(format_duration(totals.total))
                .size(FONT_SMALL)
                .font(BOLD),
            text("·").size(FONT_SMALL).color(s.muted),
            text("平均耗时").size(FONT_SMALL).color(s.muted),
            text(format_duration(totals.avg))
                .size(FONT_SMALL)
                .font(BOLD),
        ]
        .spacing(SPACE_XS)
        .align_y(Alignment::Center),
        row![
            text("最长").size(FONT_SMALL).color(s.muted),
            text(format!("{longest_duration} · {longest_title}"))
                .size(FONT_SMALL)
                .font(BOLD),
        ]
        .spacing(SPACE_XS)
        .align_y(Alignment::Center)
    ]
    .spacing(SPACE_XS);

    container(
        column![
            // 标题行：完成统计 + 维度下拉 + 关闭
            row![
                text("完成统计").size(FONT_DIALOG_TITLE).font(BOLD),
                Space::new().width(Length::Fill),
                dimension_picker(app),
                Space::new().width(SPACE_S),
                button(text("关闭").size(FONT_HEADER))
                    .on_press(Message::CloseStatsDialog)
                    .padding(BTN_LARGE),
            ]
            .align_y(Alignment::Center),
            summary,
            // 图表区：scrollable 兜底（内容超高时可滚动）
            scrollable(charts).height(Length::Fill),
        ]
        .spacing(SPACE_L)
        .width(Length::Fixed(STATS_DIALOG_WIDTH))
        .height(Length::Fixed(STATS_DIALOG_HEIGHT)),
    )
    .padding(PADDING_DIALOG)
    .style(card_style)
    .into()
}

/// 统计维度下拉（周 / 月 / 年 / 项目，同款 pick_list 样式）。
fn dimension_picker<'a>(app: &'a App) -> Element<'a, Message> {
    PickList::new(
        &STATS_DIMENSIONS[..],
        Some(app.stats_dimension),
        Message::StatsDimensionChanged,
    )
    .style(pick_list_style)
    .text_size(FONT_HEADER)
    .padding([SPACE_XS, SPACE_M])
    .into()
}

/// 图表方向：纵柱状图（周期维度：周 / 月 / 年）或横条形图（项目维度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChartKind {
    /// 纵柱状图：每周期一根竖柱
    Vertical,
    /// 横条形图：每项目一行横条
    Horizontal,
}

/// 统计图表 canvas 程序：静态柱状图 / 条形图（手绘，零依赖）。
///
/// 每次 view 重建即构造新实例（含新 Cache，恒空、draw 闭包全量执行）——
/// 数据量小（≤12 桶 / ≤项目数行）无性能问题；不把实例提升为 App 字段。
struct StatsChart {
    cache: Cache,
    kind: ChartKind,
    buckets: Vec<Bucket>,
    /// 值提取：count（完成数）或 total（耗时秒数）
    value: fn(&Bucket) -> f64,
    /// 值语义：耗时（秒）→ 短格式标注；否则整数标注
    is_duration: bool,
    dark: bool,
}

/// 图表组件：小标题 + 画布（固定高 `CHART_HEIGHT`）。
fn stats_chart<'a>(
    kind: ChartKind,
    title: &'a str,
    buckets: Vec<Bucket>,
    value: fn(&Bucket) -> f64,
    is_duration: bool,
    app: &'a App,
) -> Element<'a, Message> {
    column![
        text(title).size(FONT_TINY).color(sem(app).muted),
        Canvas::new(StatsChart {
            cache: Cache::new(),
            kind,
            buckets,
            value,
            is_duration,
            dark: app.is_dark(),
        })
        .width(Length::Fill)
        .height(Length::Fixed(CHART_HEIGHT)),
    ]
    .spacing(SPACE_XXS)
    .into()
}

/// 桶值：完成数量。
fn count_value(b: &Bucket) -> f64 {
    b.count as f64
}

/// 桶值：总耗时（秒）。
fn duration_value(b: &Bucket) -> f64 {
    b.total.num_seconds().max(0) as f64
}

impl canvas::Program<Message> for StatsChart {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geometry = self
            .cache
            .draw(renderer, bounds.size(), |frame| match self.kind {
                ChartKind::Vertical => self.draw_vertical(frame, theme, bounds),
                ChartKind::Horizontal => self.draw_horizontal(frame, theme, bounds),
            });
        vec![geometry]
    }
}

impl StatsChart {
    /// 纵柱状图：Y 轴刻度 + 水平网格线 + 柱（末桶 = 当前周期高亮）+ 柱顶外侧数值 + X 轴周期标签。
    ///
    /// 全部坐标用**画布本地坐标系**（原点 = canvas 左上角）：canvas widget 绘制时
    /// renderer 已按 `bounds.x/y` 平移到画布位置，frame 内若再叠加 `bounds.x/y`
    /// 会把整图偏移出画布——历史 bug「弹窗内图表区空白」的根因。
    fn draw_vertical(&self, frame: &mut canvas::Frame, theme: &iced::Theme, bounds: Rectangle) {
        let palette = theme.extended_palette();
        let text_color = sem_colors(self.dark).muted;
        let grid_color = palette.background.strong.color;
        let bar_color = palette.primary.base.color;
        let bar_current = palette.primary.strong.color;

        let n = self.buckets.len();
        if n == 0 {
            return;
        }
        let max = self
            .buckets
            .iter()
            .map(|b| (self.value)(b))
            .fold(0.0f64, f64::max)
            .max(1.0); // 全 0 时退化为 1，避免除零

        // 布局：左 Y 轴标签区 / 顶部数值留白 / 底部 X 轴标签区（画布本地坐标）
        let plot = vertical_plot(bounds);

        // 网格线 + Y 轴刻度（4 等分）
        for i in 0..=4 {
            let t = i as f32 / 4.0;
            let y = plot.y + plot.height * (1.0 - t);
            let value = max * f64::from(t);
            frame.stroke(
                &Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                Stroke::default().with_width(1.0).with_color(grid_color),
            );
            frame.fill_text(CanvasText {
                content: chart_value_label(value, self.is_duration),
                position: Point::new(plot.x - SPACE_XS, y - 6.0),
                color: text_color,
                size: FONT_TINY.into(),
                align_x: iced::widget::text::Alignment::Right,
                ..CanvasText::default()
            });
        }

        // 柱 + 柱顶数值 + X 轴标签
        let slot = plot.width / n as f32;
        let bar_w = (slot * 0.7).max(1.0);
        for (i, b) in self.buckets.iter().enumerate() {
            let v = (self.value)(b);
            let h = plot.height * (v / max) as f32;
            let x = plot.x + slot * i as f32 + (slot - bar_w) / 2.0;
            // 末桶 = 当前周期（窗口恒含当前周期，view 数据约定）
            let color = if i == n - 1 { bar_current } else { bar_color };
            frame.fill_rectangle(
                Point::new(x, plot.y + plot.height - h),
                Size::new(bar_w, h.max(0.0)),
                color,
            );
            // 柱顶外侧数值（柱上放文字与柱色对比度未锁定，故画在柱外背景底上）
            if v > 0.0 {
                frame.fill_text(CanvasText {
                    content: chart_value_label(v, self.is_duration),
                    position: Point::new(x + bar_w / 2.0, plot.y + plot.height - h - 2.0),
                    color: text_color,
                    size: FONT_TINY.into(),
                    align_x: iced::widget::text::Alignment::Center,
                    ..CanvasText::default()
                });
            }
            // X 轴周期标签
            frame.fill_text(CanvasText {
                content: b.label.clone(),
                position: Point::new(x + bar_w / 2.0, plot.y + plot.height + 2.0),
                color: text_color,
                size: FONT_TINY.into(),
                align_x: iced::widget::text::Alignment::Center,
                ..CanvasText::default()
            });
        }
    }

    /// 横条形图：每项目一行——行标签（截断）+ 横条（宽 = 值 / 最大值）+ 行尾数值。
    /// 坐标同纵柱状图：画布本地坐标系（原点 = canvas 左上角）。
    fn draw_horizontal(&self, frame: &mut canvas::Frame, theme: &iced::Theme, bounds: Rectangle) {
        let palette = theme.extended_palette();
        let text_color = sem_colors(self.dark).muted;
        let bar_color = palette.primary.base.color;

        let n = self.buckets.len();
        if n == 0 {
            return;
        }
        let max = self
            .buckets
            .iter()
            .map(|b| (self.value)(b))
            .fold(0.0f64, f64::max)
            .max(1.0);

        let label_w = 64.0;
        let pad = 4.0;
        let row_h = bounds.height / n as f32;
        let bar_area_w = bounds.width - label_w;
        for (i, b) in self.buckets.iter().enumerate() {
            let y = row_h * i as f32;
            // 行标签（截断）
            frame.fill_text(CanvasText {
                content: truncate(&b.label, 6),
                position: Point::new(0.0, y + row_h / 2.0),
                color: text_color,
                size: FONT_TINY.into(),
                align_y: AlignVertical::Center,
                ..CanvasText::default()
            });
            // 横条
            let v = (self.value)(b);
            let bar_w = bar_area_w * (v / max) as f32;
            frame.fill_rectangle(
                Point::new(label_w, y + pad),
                Size::new(bar_w, (row_h - 2.0 * pad).max(2.0)),
                bar_color,
            );
            // 行尾数值
            frame.fill_text(CanvasText {
                content: chart_value_label(v, self.is_duration),
                position: Point::new(label_w + bar_w + SPACE_XS, y + row_h / 2.0),
                color: text_color,
                size: FONT_TINY.into(),
                align_y: AlignVertical::Center,
                ..CanvasText::default()
            });
        }
    }
}

/// 纵柱状图布局常量：Y 轴标签区宽。
const AXIS_W: f32 = 44.0;
/// 纵柱状图布局常量：X 轴标签区高。
const X_LABEL_H: f32 = 16.0;
/// 纵柱状图布局常量：柱顶数值留白。
const CHART_TOP_PAD: f32 = 14.0;

/// 纵柱状图绘图区（**画布本地坐标**，原点 = canvas 左上角）：扣除左轴区 / 顶部留白 / 底部标签区。
///
/// 不可使用 `bounds.x/y`（绝对布局坐标）——canvas 绘制时 renderer 已按该偏移平移，
/// 叠加使用会把整图画出画布（历史 bug「弹窗内图表区空白」的根因，勿回归）。
fn vertical_plot(bounds: Rectangle) -> Rectangle {
    Rectangle {
        x: AXIS_W,
        y: CHART_TOP_PAD,
        width: bounds.width - AXIS_W,
        height: bounds.height - CHART_TOP_PAD - X_LABEL_H,
    }
}

/// 图表内数值标注：数量 = 整数；耗时 = 短格式（`3.2 小时` / `45 分` / `30 秒`，
/// 不用 `format_duration` 长文本——柱顶 / 行尾画不下）。
fn chart_value_label(v: f64, is_duration: bool) -> String {
    if is_duration {
        short_duration(Duration::seconds(v as i64))
    } else {
        format!("{}", v as i64)
    }
}

/// 耗时短格式（图表内标注用）。
fn short_duration(d: Duration) -> String {
    let secs = d.num_seconds().max(0);
    if secs >= 3600 {
        format!("{:.1} 小时", secs as f64 / 3600.0)
    } else if secs >= 60 {
        format!("{} 分", secs / 60)
    } else {
        format!("{secs} 秒")
    }
}

/// 截断文本（超长加 …；按字符计，中文安全）。
fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max_chars).collect();
        t.push('…');
        t
    }
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
        .map(format_date)
        .unwrap_or_else(|| "—".into());
    let total = todo
        .duration(app.now)
        .map(format_duration)
        .unwrap_or_else(|| "—".into());

    row![
        column![
            text(todo.title.as_str()).size(FONT_HEADER).font(BOLD),
            text(format!("{project} · 完成于 {finished} · 总耗时 {total}"))
                .size(FONT_TINY)
                .color(sem(app).muted),
        ]
        .spacing(SPACE_XXS)
        .width(Length::Fill),
        button(text("删除").size(FONT_SMALL))
            .on_press(Message::DeleteTodo(todo.id))
            .style(button::danger)
            .padding(BTN_SMALL),
    ]
    .align_y(Alignment::Center)
    .spacing(SPACE_M)
    .into()
}

/// 弹窗表单里带小标签的一行（标签在上、输入框在下）。
fn form_field<'a>(
    app: &'a App,
    label: &'a str,
    input: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        text(label).size(FONT_BODY).color(sem(app).muted),
        input.into()
    ]
    .spacing(SPACE_XS)
    .into()
}

/// 项目表单的校验错误红字提示（项目弹窗 / 编辑面板共用）：
/// 按解析结果与 validate issues 追加 时间格式 / 重名 / 范围 提示。
fn push_project_form_errors<'a>(
    mut form: iced::widget::Column<'a, Message>,
    app: &'a App,
    start_parsed: &'a Result<Option<DateTime<Utc>>, String>,
    end_parsed: &'a Result<Option<DateTime<Utc>>, String>,
    issues: &validate::ProjectFormIssues,
    range_text: &'static str,
) -> iced::widget::Column<'a, Message> {
    let s = sem(app);
    if let Err(hint) = start_parsed {
        form = form.push(text(hint.as_str()).size(FONT_SMALL).color(s.error));
    }
    if let Err(hint) = end_parsed {
        form = form.push(text(hint.as_str()).size(FONT_SMALL).color(s.error));
    }
    if issues.name_conflict {
        form = form.push(text("项目名已存在").size(FONT_SMALL).color(s.error));
    }
    if issues.range_invalid {
        form = form.push(text(range_text).size(FONT_SMALL).color(s.error));
    }
    form
}

/// 空列表提示（整区空 / 组内空均垂直居中）。
fn empty_hint<'a>(app: &'a App, message: &'static str) -> Element<'a, Message> {
    container(text(message).size(FONT_HEADER).color(sem(app).muted))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(SPACE_M * 4.0)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// 任务排序下拉（任务区统一标题行右上角，无文字标签）：控制未开始 / 进行中两列的组内排序。
fn task_sort_picker(app: &App) -> Element<'_, Message> {
    PickList::new(
        &SORT_MODES[..],
        Some(app.sort_mode),
        Message::SortModeChanged,
    )
    .text_size(FONT_HEADER)
    .padding([SPACE_XS, SPACE_M])
    .style(pick_list_style)
    .into()
}

// ---------- 项目单行栏 ----------

/// 项目单行栏：排序下拉（行首，无文字标签）+ 项目标签 + 横向滚动芯片（「全部」+ 各项目，按项目排序偏好排序），
/// 选中具体项目时右端出现「编辑 / 删除」上下文按钮。
/// 点击芯片筛选该项目的任务（「全部」恒在最前，显示全部任务）。
fn project_bar(app: &App) -> Element<'_, Message> {
    // 芯片区：「全部」恒在最前；无项目时灰字提示
    let mut chips = row![project_chip(app, None, None, "全部", app.todos.len())].spacing(SPACE_M);

    if app.projects.is_empty() {
        chips = chips.push(
            container(text("暂无项目").size(FONT_SMALL).color(sem(app).muted))
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
    let mut actions = row![].align_y(Alignment::Center).spacing(SPACE_XS);
    if let Some(project_id) = app.selected_project {
        actions = actions
            .push(
                button(text("编辑").size(FONT_SMALL))
                    .on_press(Message::StartEditProject(project_id))
                    .style(button::text)
                    .padding(BTN_TINY),
            )
            .push(
                button(text("删除").size(FONT_SMALL))
                    .on_press(Message::DeleteProject(project_id))
                    .style(button::text)
                    .padding(BTN_TINY),
            );
    }

    row![
        PickList::new(
            &SORT_MODES[..],
            Some(app.project_sort_mode),
            Message::ProjectSortModeChanged,
        )
        .text_size(FONT_HEADER)
        .padding([SPACE_XS, SPACE_M])
        .style(pick_list_style),
        text("项目").size(FONT_BODY).font(BOLD),
        Space::new().width(SPACE_M),
        scrollable(chips)
            .direction(scrollable::Direction::Horizontal(Default::default()))
            .width(Length::Fill)
            .height(Length::Shrink),
        actions,
    ]
    .align_y(Alignment::Center)
    .spacing(SPACE_M)
    .into()
}

/// 类型单行栏：行首「类型」标签 + 横向滚动芯片区 + 选中类型时的「编辑 / 删除」上下文按钮。
/// 与项目栏同款布局（芯片 = 名称 + 计数，点击筛选，选中主色描边高亮，
/// 「全部」恒在最前）；无排序下拉（类型无优先级 / 时间等排序属性，按创建顺序展示）。
fn type_bar(app: &App) -> Element<'_, Message> {
    // 芯片区：「全部」恒在最前；无类型时灰字提示
    let mut chips = row![type_chip(app, None, "全部", app.todos.len())].spacing(SPACE_M);

    if app.types.is_empty() {
        chips = chips.push(
            container(text("暂无类型").size(FONT_SMALL).color(sem(app).muted))
                .padding([6, 2])
                .align_y(Alignment::Center),
        );
    } else {
        for r#type in &app.types {
            let count = app
                .todos
                .iter()
                .filter(|todo| todo.type_id == Some(r#type.id))
                .count();
            chips = chips.push(type_chip(app, Some(r#type.id), &r#type.name, count));
        }
    }

    // 选中具体类型时：右端显示「编辑 / 删除」上下文按钮（「全部」无）
    let mut actions = row![].align_y(Alignment::Center).spacing(SPACE_XS);
    if let Some(type_id) = app.selected_type {
        actions = actions
            .push(
                button(text("编辑").size(FONT_SMALL))
                    .on_press(Message::StartEditType(type_id))
                    .style(button::text)
                    .padding(BTN_TINY),
            )
            .push(
                button(text("删除").size(FONT_SMALL))
                    .on_press(Message::DeleteType(type_id))
                    .style(button::text)
                    .padding(BTN_TINY),
            );
    }

    row![
        text("类型").size(FONT_BODY).font(BOLD),
        Space::new().width(SPACE_M),
        scrollable(chips)
            .direction(scrollable::Direction::Horizontal(Default::default()))
            .width(Length::Fill)
            .height(Length::Shrink),
        actions,
    ]
    .align_y(Alignment::Center)
    .spacing(SPACE_M)
    .into()
}

/// 单个类型芯片：名称 + 计数；点击筛选，选中主色描边高亮（与项目芯片同款样式）。
/// 类型无优先级圆点 / 无 tooltip（类型无优先级 / 时间属性）。
fn type_chip<'a>(
    app: &'a App,
    id: Option<Uuid>,
    name: &'a str,
    count: usize,
) -> Element<'a, Message> {
    let selected = app.selected_type == id;
    let s = sem(app);

    let content = row![text(name).size(FONT_BODY)]
        .align_y(Alignment::Center)
        .spacing(SPACE_S)
        .push(text(count.to_string()).size(FONT_SMALL).color(if selected {
            // 选中态：芯片文字用 primary.weak 的可读配对色（与名称同色系，弱化字号区分）
            extended(app).primary.weak.text
        } else {
            s.muted
        }));

    button(content)
        .on_press(Message::SelectType(id))
        .style(move |theme, status| project_chip_style(theme, status, selected))
        .padding(BTN_MEDIUM)
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
    let s = sem(app);

    let mut content = row![].align_y(Alignment::Center).spacing(SPACE_S);
    // 优先级圆点：仅项目芯片（「全部」无归属项目）且设置了优先级时显示
    if let Some(priority) = project.and_then(|p| p.priority) {
        content = content.push(
            text("●")
                .size(FONT_MICRO)
                .color(priority_color(s, priority)),
        );
    }
    content = content.push(text(name).size(FONT_BODY)).push(
        text(count.to_string()).size(FONT_SMALL).color(if selected {
            // 选中态：芯片文字用 primary.weak 的可读配对色（与名称同色系，弱化字号区分）
            extended(app).primary.weak.text
        } else {
            s.muted
        }),
    );

    let chip = button(content)
        .on_press(Message::SelectProject(id))
        .style(move |theme, status| project_chip_style(theme, status, selected))
        .padding(BTN_MEDIUM);

    // 起止时间悬停提示（仅设置了时间的项目；「全部」无）
    match project_period(app, id) {
        Some(period) => tooltip(
            chip,
            text(period).size(FONT_SMALL),
            tooltip::Position::Bottom,
        )
        .into(),
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
        // 选中态文字用 primary.weak 的可读配对色（iced readable 保证），未选中用主文字色——
        // 深色下 primary.weak 为中调蓝底，主文字叠其上仅 ~2.3:1，必须随选中态切换
        text_color: if selected {
            palette.primary.weak.text
        } else {
            palette.background.base.text
        },
        border: Border {
            color: if selected {
                palette.primary.base.color
            } else {
                palette.background.strong.color
            },
            width: 1.0,
            radius: RADIUS_PILL.into(),
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

    // 派生校验（单一来源 validate 模块；视图实时反馈，update 层保存时再防御一次）
    let issues = validate::project_form_issues(
        &edit.name,
        Some(edit.project_id),
        &edit.start.parsed,
        &edit.end.parsed,
        &app.projects,
    );

    let mut form = column![
        row![
            column![
                text("名称").size(FONT_TINY).color(sem(app).muted),
                text_input("项目名称", &edit.name)
                    .id(PROJECT_EDIT_NAME_ID)
                    .style(text_input_style)
                    .on_input(Message::ProjectEditNameChanged)
                    .on_submit(Message::SaveEditProject)
                    .padding(6),
            ]
            .spacing(SPACE_XXS)
            .width(Length::Fill),
            Space::new().width(SPACE_L),
            column![
                text("优先级").size(FONT_TINY).color(sem(app).muted),
                priority_picker(edit.priority, Message::ProjectEditPriorityChanged),
            ]
            .spacing(SPACE_XXS)
            .width(Length::Fixed(PRIORITY_COLUMN_WIDTH)),
        ]
        .align_y(Alignment::End),
        row![
            labeled_input(
                app,
                "开始时间",
                "2026-01-31",
                &edit.start.input,
                Message::ProjectEditStartChanged,
            ),
            labeled_input(
                app,
                "结束时间",
                "2026-01-31",
                &edit.end.input,
                Message::ProjectEditEndChanged,
            ),
        ]
        .spacing(SPACE_L),
    ]
    .spacing(SPACE_M);

    // 校验错误红字提示（弹窗 / 编辑面板共用的 helper）
    form = push_project_form_errors(
        form,
        app,
        &edit.start.parsed,
        &edit.end.parsed,
        &issues,
        "开始须早于结束",
    );

    let can_submit = validate::can_submit_project(&issues);

    let actions = row![
        Space::new().width(Length::Fill),
        button(text("保存").size(FONT_BODY))
            .on_press_maybe(can_submit.then_some(Message::SaveEditProject))
            .style(button::primary)
            .padding(BTN_CARD),
        Space::new().width(SPACE_S),
        button(text("取消").size(FONT_BODY))
            .on_press(Message::CancelEditProject)
            .padding(BTN_MEDIUM),
    ]
    .align_y(Alignment::Center);

    container(column![form, actions].spacing(SPACE_M))
        .width(Length::Fill)
        .padding(12)
        .style(card_style)
        .into()
}

// ---------- 类型栏编辑面板 ----------

/// 类型栏下方展开的全宽编辑面板：名称（必填，重名校验排除自身）+ 保存 / 取消。
fn type_edit_panel(app: &App) -> Element<'_, Message> {
    let edit = app
        .type_edit
        .as_ref()
        .expect("编辑面板仅在 type_edit 命中时渲染");

    // 派生校验（单一来源 validate 模块；视图实时反馈，update 层保存时再防御一次）
    let issues = validate::type_form_issues(&edit.name, Some(edit.type_id), &app.types);

    let mut form = column![
        column![
            text("名称").size(FONT_TINY).color(sem(app).muted),
            text_input("类型名称", &edit.name)
                .id(TYPE_EDIT_NAME_ID)
                .style(text_input_style)
                .on_input(Message::TypeEditNameChanged)
                .on_submit(Message::SaveEditType)
                .padding(6),
        ]
        .spacing(SPACE_XXS)
        .width(Length::Fill),
    ]
    .spacing(SPACE_M);

    // 重名红字提示
    if issues.name_conflict {
        form = form.push(text("类型名已存在").size(FONT_SMALL).color(sem(app).error));
    }

    let can_submit = validate::can_submit_type(&issues);
    let actions = row![
        Space::new().width(Length::Fill),
        button(text("保存").size(FONT_BODY))
            .on_press_maybe(can_submit.then_some(Message::SaveEditType))
            .style(button::primary)
            .padding(BTN_CARD),
        Space::new().width(SPACE_S),
        button(text("取消").size(FONT_BODY))
            .on_press(Message::CancelEditType)
            .padding(BTN_MEDIUM),
    ]
    .align_y(Alignment::Center);

    container(column![form, actions].spacing(SPACE_M))
        .width(Length::Fill)
        .padding(12)
        .style(card_style)
        .into()
}

// ---------- 标题栏添加下拉菜单 ----------

/// 标题栏分体按钮的下拉菜单：右上角悬浮（无压暗、无卡片包裹），
/// 「＋ 添加项目」/「＋ 添加类型」与主按钮「＋ 添加任务」同款 primary 样式；
/// 宽度与分体按钮总宽一致（`ADD_BUTTONS_WIDTH`），左右边缘与文字起点三方对齐。
/// 点击菜单项打开对应弹窗（弹窗互斥逻辑不变，update 层自动收起菜单）。
fn add_menu_card() -> Element<'static, Message> {
    container(
        column![
            button(text("＋ 添加项目").size(FONT_BODY))
                .on_press(Message::OpenProjectDialog)
                .style(button::primary)
                .padding(BTN_MEDIUM)
                .width(Length::Fill),
            Space::new().height(SPACE_XS),
            button(text("＋ 添加类型").size(FONT_BODY))
                .on_press(Message::OpenTypeDialog)
                .style(button::primary)
                .padding(BTN_MEDIUM)
                .width(Length::Fill),
        ]
        .spacing(SPACE_XXS),
    )
    .width(Length::Fixed(ADD_BUTTONS_WIDTH))
    .into()
}

// ---------- 底部角落条 ----------

/// 底部 footer：主题指示器 + 统计；横条样式（弱色底 + 四边框、无圆角）常规流布局固定于窗口内容区
/// 底部（任务列表卡片容器下方，不悬浮、不与滚动内容重叠；仍位于弹窗遮罩之下）。
fn footer(app: &App) -> Element<'_, Message> {
    container(
        row![
            theme_indicator(app),
            Space::new().width(Length::Fill),
            stats_group(app),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([SPACE_S, SPACE_L])
    .style(footer_style)
    .into()
}

/// footer 横条样式：弱色底 + 主题描边（无圆角），与卡片容器区分。
fn footer_style(theme: &iced::Theme) -> container::Style {
    let background = theme.extended_palette().background;
    container::Style {
        background: Some(Background::Color(background.weak.color)),
        border: Border {
            color: background.strong.color,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

/// 左下角主题指示器：`Theme: Auto/Light/Dark`（无 tooltip），点击循环切换主题模式；
/// 文本样式按钮（无胶囊外壳，与统计同属 footer 横条内的文本）。
fn theme_indicator(app: &App) -> Element<'_, Message> {
    button(text(format!("Theme: {}", app.theme_mode.label())).size(FONT_BODY))
        .on_press(Message::CycleThemeMode)
        .style(button::text)
        .padding([SPACE_XS, SPACE_M])
        .into()
}

/// 右下角统计：`共 x 项 | 未开始 x | 进行中 x | 已完成 x | 统计` 单行文本（无外壳，footer 横条内）；
/// 「已完成 x」与「统计」为可点击链接（主色 + 粗体，悬停加深），分别打开归档 / 统计弹窗；其余段纯展示。
fn stats_group(app: &App) -> Element<'_, Message> {
    let total = app.todos.len();
    let pending = app
        .todos
        .iter()
        .filter(|todo| todo.status() == TodoStatus::Pending)
        .count();
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
    row![
        text(format!("共 {total} 项")).size(FONT_BODY),
        text(" | ").size(FONT_BODY).color(sem(app).muted),
        text(format!("未开始 {pending}")).size(FONT_BODY),
        text(" | ").size(FONT_BODY).color(sem(app).muted),
        text(format!("进行中 {in_progress}")).size(FONT_BODY),
        text(" | ").size(FONT_BODY).color(sem(app).muted),
        button(text(format!("已完成 {done}")).size(FONT_BODY).font(BOLD))
            .on_press(Message::OpenCompletedDialog)
            .style(link_button_style),
        text(" | ").size(FONT_BODY).color(sem(app).muted),
        button(text("统计").size(FONT_BODY).font(BOLD))
            .on_press(Message::OpenStatsDialog)
            .style(link_button_style),
    ]
    .align_y(Alignment::Center)
    .spacing(SPACE_XS)
    .into()
}

/// 链接按钮样式：无背景，主色文字（悬停加深）——用于「已完成 x」可点击入口。
fn link_button_style(theme: &iced::Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let base = button::Style {
        background: None,
        text_color: palette.primary.base.color,
        ..Default::default()
    };
    match status {
        button::Status::Hovered => button::Style {
            text_color: palette.primary.strong.color,
            ..base
        },
        _ => base,
    }
}

/// 错误横幅样式：弱色底 + 危险色描边 + 卡片圆角（文字用 danger 色，弱底上可读性 ≥ 4.5:1）。
fn error_banner_style(theme: &iced::Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.danger.strong.color,
            width: 1.0,
            radius: RADIUS_CARD.into(),
        },
        ..Default::default()
    }
}

/// 带小标签的窄输入框（项目编辑面板用），回车即保存。
fn labeled_input<'a>(
    app: &'a App,
    label: &'a str,
    placeholder: &'a str,
    value: &'a str,
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    column![
        text(label).size(FONT_TINY).color(sem(app).muted),
        text_input(placeholder, value)
            .style(text_input_style)
            .on_input(on_input)
            .on_submit(Message::SaveEditProject)
            .padding(6),
    ]
    .spacing(SPACE_XXS)
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

/// 双列分组显示：任务区统一标题行（左=未开始计数、中=进行中计数、右上角=任务排序下拉），
/// 下方双列独立滚动（左=未开始、右=进行中）；组内按 `sort_mode` 排序（优先级 / 截止日期 /
/// 综合，未设置均排最后）；已完成任务不在此显示（见归档弹窗）。
fn grouped_columns<'a>(app: &'a App, todos: Vec<&'a Todo>) -> Element<'a, Message> {
    if todos.is_empty() {
        return empty_hint(
            app,
            if app.selected_project.is_some() || app.selected_type.is_some() {
                // 项目 / 类型筛选（或叠加）下无匹配任务：中性文案
                "暂无匹配任务"
            } else {
                "暂无任务，先添加一个吧"
            },
        );
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

    column![
        // 统一标题行：未开始贴左、进行中居中、排序下拉在右上角（两个 Fill 间隔均分剩余宽度）
        row![
            text(format!("未开始 ({})", pending.len()))
                .size(FONT_HEADER)
                .font(BOLD),
            Space::new().width(Length::Fill),
            text(format!("进行中 ({})", in_progress.len()))
                .size(FONT_HEADER)
                .font(BOLD),
            Space::new().width(Length::Fill),
            task_sort_picker(app),
        ]
        .align_y(Alignment::Center),
        Space::new().height(SPACE_S),
        row![
            group_scroll("暂无未开始任务", pending, app),
            Space::new().width(SPACE_L),
            group_scroll("暂无进行中任务", in_progress, app),
        ]
        .height(Length::Fill),
    ]
    .spacing(SPACE_XS)
    .height(Length::Fill)
    .into()
}

/// 单个分组列的可滚动卡片列表（标题与计数已合并到任务区统一标题行）；空组显示提示。
fn group_scroll<'a>(
    empty: &'static str,
    todos: Vec<&'a Todo>,
    app: &'a App,
) -> Element<'a, Message> {
    let list: Element<'_, Message> = if todos.is_empty() {
        empty_hint(app, empty)
    } else {
        column(todos.into_iter().map(|todo| todo_card(todo, app)))
            .spacing(SPACE_M)
            .padding(4)
            .into()
    };

    scrollable(list)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

/// 单个任务的卡片：标题 + 可选描述 + 状态徽章 + 操作按钮 + 时间元信息。
/// 默认全部属性只读展示；该卡片处于编辑模式时（"当前任务"）渲染可编辑表单。
fn todo_card<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let s = sem(app);
    if app
        .todo_edit
        .as_ref()
        .is_some_and(|edit| edit.todo_id == todo.id)
    {
        return todo_card_editor(todo, app);
    }

    let mut head = row![
        text(todo.title.as_str()).size(FONT_CARD_TITLE).font(BOLD),
        Space::new().width(Length::Fill),
    ]
    .align_y(Alignment::Center)
    .spacing(SPACE_M);
    // 优先级徽章：未设置不显示
    if let Some(priority) = todo.priority {
        head = head.push(
            text(priority.label())
                .size(FONT_TINY)
                .color(priority_color(s, priority)),
        );
    }
    head = head
        .push(
            text(todo.status().label())
                .size(FONT_SMALL)
                .color(status_color(s, todo.status())),
        )
        .push(actions(todo));

    // 描述：非空时在标题下方以灰色小字显示（自动换行）
    let mut content = column![head].spacing(SPACE_M);
    if !todo.description.is_empty() {
        content = content.push(
            text(todo.description.as_str())
                .size(FONT_BODY)
                .color(s.muted)
                .width(Length::Fill),
        );
    }
    content = content.push(meta_rows(todo, app));
    // 编辑按钮：卡片右下角（进入编辑模式，即"当前任务"）
    content = content.push(
        row![
            Space::new().width(Length::Fill),
            button(text("编辑").size(FONT_BODY))
                .on_press(Message::EditTodo(todo.id))
                .style(button::text)
                .padding(BTN_SMALL),
        ]
        .align_y(Alignment::Center),
    );

    container(content)
        .width(Length::Fill)
        .padding(12)
        .style(card_style)
        .into()
}

/// 状态徽章颜色：未开始＝灰、进行中＝蓝、已完成＝绿（取自当前主题语义色）。
fn status_color(sem: SemColors, status: TodoStatus) -> Color {
    match status {
        TodoStatus::Pending => sem.muted,
        TodoStatus::InProgress => sem.blue,
        TodoStatus::Done => sem.done,
    }
}

/// 操作按钮：按状态显示"开始 / 完成"，始终有"删除"（「编辑」在卡片右下角）。
fn actions(todo: &Todo) -> Element<'_, Message> {
    let mut actions = row![].spacing(SPACE_S);

    match todo.status() {
        TodoStatus::Pending => {
            actions = actions.push(
                button(text("开始").size(FONT_BODY))
                    .on_press(Message::StartTodo(todo.id))
                    .style(button::primary)
                    .padding(BTN_CARD),
            );
        }
        TodoStatus::InProgress => {
            actions = actions.push(
                button(text("完成").size(FONT_BODY))
                    .on_press(Message::FinishTodo(todo.id))
                    .style(success_button)
                    .padding(BTN_CARD),
            );
        }
        TodoStatus::Done => {}
    }

    actions
        .push(
            button(text("删除").size(FONT_BODY))
                .on_press(Message::DeleteTodo(todo.id))
                .style(button::danger)
                .padding(BTN_CARD),
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
            radius: RADIUS_CARD.into(),
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
    let s = sem(app);

    // 标题（必填）：回车保存
    let title_input = text_input("任务标题（必填）", &edit.title)
        .style(text_input_style)
        .on_input(Message::EditTitleChanged)
        .on_submit(Message::SaveEditTodo)
        .padding(8)
        .width(Length::Fill);

    // 描述（可选）：回车保存
    let description_input = text_input("任务描述（可选）", &edit.description)
        .style(text_input_style)
        .on_input(Message::EditDescriptionChanged)
        .on_submit(Message::SaveEditTodo)
        .padding(8);

    // 所属项目 / 类型 / 优先级 / 截止时间：与任务弹窗共用的表单行组件（无快速新建入口）
    let project_picker = project_picker_row(
        app,
        "项目",
        edit.project_id,
        Message::EditProjectChanged,
        false,
    );
    let type_picker = type_picker_row(app, "类型", edit.type_id, Message::EditTypeChanged, false);
    let priority_row = priority_row(app, edit.priority, Message::EditPriorityChanged);
    let due_row = due_row(
        app,
        &edit.due.input,
        Message::EditDueChanged,
        Message::EditQuickDue,
        Message::SaveEditTodo,
        8,
    );

    // 头部：标题输入 + 状态徽章（保存/取消在卡片底部右下角）
    let head = row![
        title_input,
        Space::new().width(SPACE_M),
        text(todo.status().label())
            .size(FONT_SMALL)
            .color(status_color(s, todo.status())),
    ]
    .align_y(Alignment::Center)
    .spacing(SPACE_M);

    // 表单主体；截止时间格式错误时追加红字提示
    let mut form = column![
        head,
        description_input,
        project_picker,
        type_picker,
        priority_row,
        due_row
    ]
    .spacing(SPACE_M);
    if let Err(hint) = &edit.due.parsed {
        form = form.push(text(hint.as_str()).size(FONT_SMALL).color(s.error));
    }

    // 底部操作行：保存 / 取消（与只读卡片的「编辑」按钮位置对称；单一来源 validate 模块）
    let issues = validate::todo_form_issues(
        &edit.title,
        &edit.due.parsed,
        edit.project_id,
        edit.type_id,
        &app.projects,
        &app.types,
    );
    let can_submit = validate::can_submit_todo(&issues);
    let actions = row![
        Space::new().width(Length::Fill),
        button(text("保存").size(FONT_BODY))
            .on_press_maybe(can_submit.then_some(Message::SaveEditTodo))
            .style(button::primary)
            .padding(BTN_CARD),
        Space::new().width(SPACE_S),
        button(text("取消").size(FONT_BODY))
            .on_press(Message::CancelEditTodo)
            .padding(BTN_SMALL),
    ]
    .align_y(Alignment::Center);

    container(column![form, actions, time_meta_rows(todo, app)].spacing(SPACE_M))
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
            radius: RADIUS_CARD.into(),
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

/// 类型下拉的选项（"无类型" + 全部类型），弹窗与编辑模式共用。
fn type_choices(app: &App) -> Vec<TypeChoice> {
    std::iter::once(TypeChoice::none())
        .chain(app.types.iter().map(TypeChoice::of))
        .collect()
}

/// 任务归属的只读展示行：项目名（未归属显示"无项目"）。
/// 项目归属只能在编辑模式下修改（R15）。
fn project_row_readonly<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let (name, color) = match todo
        .project_id
        .and_then(|id| app.projects.iter().find(|p| p.id == id))
    {
        Some(project) => (project.name.as_str(), sem(app).muted),
        None => ("无项目", sem(app).muted),
    };
    time_row(app, "项目", name.into(), color)
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

/// pick_list 的选项包装：`id = None` 表示"无类型"（不设置类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeChoice {
    id: Option<Uuid>,
    label: String,
}

impl TypeChoice {
    /// "无类型"选项。
    fn none() -> Self {
        Self {
            id: None,
            label: "无类型".into(),
        }
    }

    /// 由类型构造选项。
    fn of(r#type: &TodoType) -> Self {
        Self {
            id: Some(r#type.id),
            label: r#type.name.clone(),
        }
    }

    /// 按类型 id 构造选项（类型已被删除时回落为"无类型"）。
    fn of_id(id: Option<Uuid>, types: &[TodoType]) -> Self {
        match id.and_then(|id| types.iter().find(|t| t.id == id)) {
            Some(r#type) => Self::of(r#type),
            None => Self::none(),
        }
    }
}

impl std::fmt::Display for TypeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// 时间元信息：截止时间 + 创建 / 开始 / 结束（年月日粒度）；进行中附已耗时，已完成附总耗时。
/// （不含项目行：普通模式由 `project_row_readonly` 展示，编辑模式用下拉。）
fn time_meta_rows<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let mut meta = column![].spacing(SPACE_XS);
    let s = sem(app);

    // 截止时间：已逾期且未完成的任务标红提示
    if let Some(due) = todo.due_at {
        let overdue = todo.status() != TodoStatus::Done && due < app.now;
        meta = meta.push(time_row(
            app,
            "截止时间",
            format_date(due),
            if overdue { s.error } else { s.muted },
        ));
    }

    meta = meta.push(time_row(
        app,
        "创建时间",
        format_date(todo.created_at),
        s.muted,
    ));

    if todo.status() == TodoStatus::InProgress {
        let elapsed = todo
            .duration(app.now)
            .map(format_duration)
            .unwrap_or_else(|| "—".into());
        meta = meta.push(time_row(app, "已耗时", elapsed, s.blue));
    }

    meta = meta.push(time_row(
        app,
        "结束时间",
        todo.finished_at
            .map(format_date)
            .unwrap_or_else(|| "—".into()),
        s.muted,
    ));

    if todo.status() == TodoStatus::Done {
        let total = todo
            .duration(app.now)
            .map(format_duration)
            .unwrap_or_else(|| "—".into());
        meta = meta.push(time_row(app, "总耗时", total, s.done));
    }

    meta.into()
}

/// 任务类型的只读展示行：类型名（灰字）。未设置类型时返回 `None`——
/// 类型被删除后任务"默认为普通任务，不对外显示"（区别于项目行恒显示"无项目"）。
fn type_row_readonly<'a>(todo: &'a Todo, app: &'a App) -> Option<Element<'a, Message>> {
    let name = todo
        .type_id
        .and_then(|id| app.types.iter().find(|t| t.id == id))
        .map(|r#type| r#type.name.as_str())?;
    Some(time_row(app, "类型", name.into(), sem(app).muted))
}

/// 普通模式的任务元信息：归属只读行 + 类型只读行（可选）+ 时间行。
fn meta_rows<'a>(todo: &'a Todo, app: &'a App) -> Element<'a, Message> {
    let mut meta = column![project_row_readonly(todo, app)].spacing(SPACE_XS);
    if let Some(type_row) = type_row_readonly(todo, app) {
        meta = meta.push(type_row);
    }
    meta = meta.push(time_meta_rows(todo, app));
    meta.into()
}

/// 带固定宽度标签的一行时间信息。
fn time_row<'a>(app: &'a App, label: &'a str, value: String, color: Color) -> Element<'a, Message> {
    row![
        text(label)
            .size(FONT_BODY)
            .color(sem(app).muted)
            .width(Length::Fixed(LABEL_WIDTH)),
        text(value).size(FONT_BODY).color(color),
    ]
    .spacing(SPACE_S)
    .into()
}

/// 日期格式化（卡片 / 归档时间字段）：UTC 存储、本地时区显示，**年月日粒度**（不显示时分秒）。
fn format_date(dt: DateTime<Utc>) -> String {
    dt.with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string()
}

/// 耗时格式化（两级粒度，无秒，不逐秒跳动）：
/// ≥1 天「X 天 Y 小时」（整小时省略）／<1 天「X 小时 Y 分」（整分省略）／<1 小时「X 分」／<1 分钟「不足 1 分」。
fn format_duration(d: Duration) -> String {
    let total = d.num_seconds().max(0);
    if total >= 86_400 {
        let days = total / 86_400;
        let hours = (total % 86_400) / 3_600;
        if hours > 0 {
            format!("{days} 天 {hours} 小时")
        } else {
            format!("{days} 天")
        }
    } else if total >= 3_600 {
        let hours = total / 3_600;
        let minutes = (total % 3_600) / 60;
        if minutes > 0 {
            format!("{hours} 小时 {minutes} 分")
        } else {
            format!("{hours} 小时")
        }
    } else if total >= 60 {
        format!("{} 分", total / 60)
    } else {
        "不足 1 分".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AddDialog, ParsedField, ProjectDialog, ProjectEdit, ThemeMode, TodoEdit, TypeDialog,
        TypeEdit,
    };
    use chrono::TimeZone;

    fn dt(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    /// 渲染冒烟断言：构造**状态一致**（弹窗开则表单在、编辑态指向存在的实体）的 App，
    /// `view(&app)` 构建元素树不 panic（渲染守卫 `expect` 依赖这些不变量）。
    fn renders(app: &App) {
        drop(view(app));
    }

    fn sample_app() -> App {
        App {
            now: dt(1_700_000_000),
            ..App::default()
        }
    }

    /// 全字段任务：进行中（带开始时间）。
    fn in_progress_todo(app: &App, title: &str) -> Todo {
        let mut todo = Todo::new_full(
            title.into(),
            "描述".into(),
            Some(Priority::High),
            None,
            None,
            Some(dt(1_700_100_000)),
            app.now,
        );
        todo.started_at = Some(dt(1_700_000_100));
        todo
    }

    /// 全字段已完成任务（带开始 / 结束时间）。
    fn done_todo(app: &App, title: &str) -> Todo {
        let mut todo = in_progress_todo(app, title);
        todo.finished_at = Some(dt(1_700_000_500));
        todo
    }

    #[test]
    fn view_renders_empty_app() {
        renders(&sample_app());
    }

    #[test]
    fn view_renders_todos_projects_and_error() {
        let mut app = sample_app();
        let project = Project::new_full(
            "工作".into(),
            Some(Priority::Medium),
            Some(dt(1_699_000_000)),
            Some(dt(1_700_200_000)),
            app.now,
        );
        let project_id = project.id;
        app.projects.push(project);
        let pending = Todo::new_full(
            "写方案".into(),
            "先读需求".into(),
            Some(Priority::High),
            Some(project_id),
            None,
            Some(dt(1_700_100_000)),
            app.now,
        );
        // 保持新任务在最前（生产语义：insert(0)）
        let _ = pending.id;
        let mut in_progress = in_progress_todo(&app, "编码");
        in_progress.project_id = Some(project_id);
        let done = done_todo(&app, "已读文档");
        app.todos = vec![pending, in_progress, done];
        app.selected_project = Some(project_id);
        app.sort_mode = SortMode::Priority;
        app.project_sort_mode = SortMode::Due;
        app.error = Some("测试错误横幅".into());
        renders(&app);
    }

    /// 测试场景常需"先默认后逐字段赋值"构造表单状态，比结构体字面量更贴近真实交互路径。
    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn view_renders_task_dialog() {
        let mut app = sample_app();
        app.projects
            .push(Project::new_full("工作".into(), None, None, None, app.now));
        let mut dialog = AddDialog::default();
        dialog.title = "写方案".into();
        dialog.description = "先读需求".into();
        dialog.project_id = Some(app.projects[0].id);
        dialog.priority = Some(Priority::Low);
        dialog.due = ParsedField::changed("2026-01-31 18:30".into());
        app.add_dialog = Some(dialog);
        renders(&app);
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn view_renders_task_dialog_with_due_error() {
        // 截止时间非法：红字提示路径
        let mut app = sample_app();
        let mut dialog = AddDialog::default();
        dialog.title = "写方案".into();
        dialog.due = ParsedField::changed("后天".into());
        app.add_dialog = Some(dialog);
        renders(&app);
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn view_renders_project_dialog() {
        let mut app = sample_app();
        app.projects
            .push(Project::new_full("工作".into(), None, None, None, app.now));
        let mut dialog = ProjectDialog::default();
        dialog.name = "  生活  ".into();
        dialog.priority = Some(Priority::High);
        dialog.start = ParsedField::changed("2026-01-01".into());
        dialog.end = ParsedField::changed("2026-01-31".into());
        app.project_dialog = Some(dialog);
        renders(&app);
    }

    #[test]
    fn view_renders_completed_dialog() {
        let mut app = sample_app();
        app.todos.push(done_todo(&app, "已完成任务"));
        app.show_completed = true;
        renders(&app);
    }

    #[test]
    fn view_renders_stats_dialog_week_with_data() {
        // 统计弹窗：周维度 + 有已完成任务（纵柱状图 / 汇总行路径）
        let mut app = sample_app();
        app.todos.push(done_todo(&app, "已完成任务"));
        app.show_stats = true;
        app.stats_dimension = StatsDimension::Week;
        renders(&app);
    }

    #[test]
    fn view_renders_stats_dialog_project_empty() {
        // 统计弹窗：项目维度 + 无已完成任务（空态路径）
        let mut app = sample_app();
        app.show_stats = true;
        app.stats_dimension = StatsDimension::Project;
        renders(&app);
    }

    /// 耗时两级粒度：各档位切换边界与整档位省略零次单位。
    #[test]
    fn format_duration_two_level_granularity() {
        use chrono::Duration as D;
        assert_eq!(format_duration(D::seconds(0)), "不足 1 分");
        assert_eq!(format_duration(D::seconds(59)), "不足 1 分");
        assert_eq!(format_duration(D::seconds(60)), "1 分");
        assert_eq!(format_duration(D::seconds(3599)), "59 分");
        // 整小时：省略零分
        assert_eq!(format_duration(D::seconds(3600)), "1 小时");
        assert_eq!(format_duration(D::seconds(3660)), "1 小时 1 分");
        assert_eq!(format_duration(D::seconds(86399)), "23 小时 59 分");
        // 整天：省略零小时
        assert_eq!(format_duration(D::seconds(86400)), "1 天");
        assert_eq!(format_duration(D::seconds(90000)), "1 天 1 小时");
        assert_eq!(
            format_duration(D::seconds(86_400 * 2 + 3_600 * 5)),
            "2 天 5 小时"
        );
        // 负值兜底为 0
        assert_eq!(format_duration(D::seconds(-30)), "不足 1 分");
    }

    /// 日期格式化：本地时区年月日，不含时分秒。
    #[test]
    fn format_date_local_ymd_without_time() {
        use chrono::TimeZone;
        // 选 UTC 正午（UTC-12 ~ +12 均同日，可移植）
        let noon = Utc.timestamp_opt(1_750_000_000, 0).unwrap();
        let expected = noon
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(format_date(noon), expected);
        // 长度固定（YYYY-MM-DD），不含空格（无时分秒）
        assert_eq!(format_date(noon).len(), 10);
        assert!(!format_date(noon).contains(' '));
    }

    /// 回归：纵柱状图绘图区必须位于画布本地坐标内（历史 bug——误用 `bounds.x/y`
    /// 绝对布局坐标，弹窗内图表整体偏移出画布、显示空白；此测试锁定不回归）。
    #[test]
    fn vertical_plot_stays_inside_canvas_bounds() {
        // 模拟弹窗内的真实布局坐标：画布位于窗口中部（x/y 均非 0）
        let bounds = Rectangle {
            x: 160.0,
            y: 118.0,
            width: 520.0,
            height: 150.0,
        };
        let plot = vertical_plot(bounds);
        // 本地坐标：原点在画布内、尺寸不超出画布
        assert!(plot.x >= 0.0 && plot.y >= 0.0);
        assert!(plot.x + plot.width <= bounds.width);
        assert!(plot.y + plot.height <= bounds.height);
        // 且不依赖 bounds 位置（偏移变化不影响绘图区）
        let moved = Rectangle {
            x: 320.0,
            y: 300.0,
            ..bounds
        };
        assert_eq!(vertical_plot(moved), plot);
    }

    #[test]
    fn view_renders_add_menu() {
        let mut app = sample_app();
        app.add_menu_open = true;
        renders(&app);
    }

    #[test]
    fn view_renders_project_edit_panel() {
        let mut app = sample_app();
        let project = Project::new_full(
            "工作".into(),
            None,
            Some(dt(1_699_000_000)),
            Some(dt(1_700_200_000)),
            app.now,
        );
        app.projects.push(project);
        app.project_edit = Some(ProjectEdit {
            project_id: app.projects[0].id,
            name: "工作".into(),
            priority: None,
            start: ParsedField::prefilled(Some(dt(1_699_000_000))),
            end: ParsedField::prefilled(Some(dt(1_700_200_000))),
        });
        renders(&app);
    }

    #[test]
    fn view_renders_todo_edit_card() {
        let mut app = sample_app();
        let todo = Todo::new_full(
            "写方案".into(),
            String::new(),
            None,
            None,
            None,
            None,
            app.now,
        );
        app.todos.push(todo);
        app.todo_edit = Some(TodoEdit {
            todo_id: app.todos[0].id,
            title: "写方案".into(),
            description: String::new(),
            priority: None,
            project_id: None,
            type_id: None,
            due: ParsedField::new(),
        });
        renders(&app);
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn view_renders_stacked_quick_project_dialog() {
        // 任务弹窗内「＋ 新建」：项目弹窗叠加于任务弹窗之上（顶层优先渲染）
        let mut app = sample_app();
        let mut add = AddDialog::default();
        add.title = "写方案".into();
        app.add_dialog = Some(add);
        app.project_dialog = Some(ProjectDialog::default());
        renders(&app);
    }

    #[test]
    fn view_renders_dark_theme_with_all_states() {
        // 深色主题（系统深色 + 手动 Dark）下语义色取板路径
        let mut app = sample_app();
        app.theme_mode = ThemeMode::Dark;
        app.system_dark = true;
        app.todos.push(in_progress_todo(&app, "进行中"));
        app.error = Some("深色错误横幅".into());
        renders(&app);
    }

    #[test]
    fn view_renders_filtered_empty_groups() {
        // 整区空（有筛选无任务）与组内空的提示路径
        let mut app = sample_app();
        app.projects
            .push(Project::new_full("工作".into(), None, None, None, app.now));
        app.selected_project = Some(app.projects[0].id);
        renders(&app);
    }

    #[test]
    fn view_renders_type_bar_with_selection() {
        // 类型单行栏：内建种子 + 选中态 + 计数
        let mut app = sample_app();
        let work = TodoType::new_full("工作".into(), app.now);
        let study = TodoType::new_full("学习".into(), app.now);
        app.types = vec![work.clone(), study.clone()];
        let mut todo = Todo::new_full(
            "写方案".into(),
            String::new(),
            None,
            None,
            None,
            None,
            app.now,
        );
        todo.type_id = Some(work.id);
        app.todos.push(todo);
        app.selected_type = Some(work.id);
        app.selected_project = None;
        renders(&app);
    }

    #[test]
    fn view_renders_type_bar_empty() {
        // 类型删光：仅「全部」芯片 + 「暂无类型」提示
        let mut app = sample_app();
        app.types.clear();
        renders(&app);
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn view_renders_type_dialog() {
        let mut app = sample_app();
        app.types.push(TodoType::new_full("阅读".into(), app.now));
        let mut dialog = TypeDialog::default();
        dialog.name = "  阅读  ".into();
        app.type_dialog = Some(dialog);
        renders(&app);
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn view_renders_type_dialog_with_conflict() {
        // 重名红字提示路径
        let mut app = sample_app();
        app.types.push(TodoType::new_full("阅读".into(), app.now));
        let mut dialog = TypeDialog::default();
        dialog.name = "阅读".into();
        app.type_dialog = Some(dialog);
        renders(&app);
    }

    #[test]
    fn view_renders_type_edit_panel() {
        let mut app = sample_app();
        let r#type = TodoType::new_full("阅读".into(), app.now);
        app.types.push(r#type);
        app.selected_type = Some(app.types[0].id);
        app.type_edit = Some(TypeEdit {
            type_id: app.types[0].id,
            name: "阅读".into(),
        });
        renders(&app);
    }

    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn view_renders_stacked_quick_type_dialog() {
        // 任务弹窗内「＋ 新建」：类型弹窗叠加于任务弹窗之上（顶层优先渲染）
        let mut app = sample_app();
        let mut add = AddDialog::default();
        add.title = "写方案".into();
        app.add_dialog = Some(add);
        app.type_dialog = Some(TypeDialog::default());
        renders(&app);
    }

    #[test]
    fn view_renders_type_filtered_empty() {
        // 仅类型筛选（无项目筛选）下整区空：中性空态文案路径
        let mut app = sample_app();
        let r#type = TodoType::new_full("阅读".into(), app.now);
        app.types.push(r#type);
        app.selected_type = Some(app.types[0].id);
        renders(&app);
    }

    #[test]
    fn view_renders_todo_card_with_type_row() {
        // 卡片类型只读行（类型存在时显示）
        let mut app = sample_app();
        let r#type = TodoType::new_full("阅读".into(), app.now);
        app.types.push(r#type);
        let mut todo = Todo::new_full(
            "读书".into(),
            String::new(),
            None,
            None,
            None,
            None,
            app.now,
        );
        todo.type_id = Some(app.types[0].id);
        app.todos.push(todo);
        renders(&app);
    }
}
