//! 视图：把应用状态渲染成 iced 元素树。
//!
//! 布局：标题栏 + 输入行 + 可滚动任务列表。
//! 每个任务一张卡片：标题、状态徽章、操作按钮，
//! 以及创建 / 开始 / 结束三个时间点和（实时）耗时。

use chrono::{DateTime, Duration, Utc};
use iced::font::Weight;
use iced::widget::{Space, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Background, Border, Color, Element, Font, Length};

use crate::model::{App, Todo, TodoStatus};
use crate::update::Message;

/// 次要文本（标签、提示）颜色：中性灰
const MUTED: Color = Color::from_rgb(0.55, 0.58, 0.62);
/// 错误提示：红
const ERROR_COLOR: Color = Color::from_rgb(0.92, 0.45, 0.45);
/// 进行中（橙）
const ACCENT: Color = Color::from_rgb(0.98, 0.70, 0.25);
/// 已完成（绿）
const DONE: Color = Color::from_rgb(0.36, 0.78, 0.50);

const BOLD: Font = Font {
    weight: Weight::Bold,
    ..Font::DEFAULT
};

/// 应用主视图。
pub fn view(app: &App) -> Element<'_, Message> {
    let header = row![
        text("待办清单").size(26).font(BOLD),
        Space::new().width(Length::Fill),
        text(summary(app)).size(13).color(MUTED),
    ]
    .align_y(Alignment::Center);

    let input_row = row![
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
    .align_y(Alignment::Center);

    let mut body = column![header, input_row].spacing(12).height(Length::Fill);

    if let Some(error) = &app.error {
        body = body.push(text(error.as_str()).size(12).color(ERROR_COLOR));
    }

    body = body.push(if app.todos.is_empty() {
        empty_hint()
    } else {
        todo_list(app)
    });

    container(body)
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
fn empty_hint() -> Element<'static, Message> {
    container(text("暂无任务，先添加一个吧").size(14).color(MUTED))
        .width(Length::Fill)
        .padding(32)
        .center_x(Length::Fill)
        .into()
}

/// 可滚动的任务列表。
fn todo_list(app: &App) -> Element<'_, Message> {
    scrollable(
        column(app.todos.iter().map(|todo| todo_card(todo, app.now)))
            .spacing(8)
            .padding(4),
    )
    .height(Length::Fill)
    .into()
}

/// 单个任务的卡片。
fn todo_card(todo: &Todo, now: DateTime<Utc>) -> Element<'_, Message> {
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

    container(column![head, meta_rows(todo, now)].spacing(8))
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

/// 时间元信息：创建 / 开始 / 结束；进行中附实时耗时，已完成附总耗时。
fn meta_rows(todo: &Todo, now: DateTime<Utc>) -> Element<'_, Message> {
    let mut meta = column![
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
            .duration(now)
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
            .duration(now)
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
