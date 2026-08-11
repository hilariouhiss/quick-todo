//! Quick Todo 桌面应用 —— 程序入口。
//!
//! 使用 iced 0.14 的函数式 API（`iced::application`）装配整个应用：
//! boot 初始化状态并异步加载数据，update 处理消息，view 渲染界面，
//! subscription 提供每秒时钟以刷新"进行中"任务的实时耗时。

mod model;
mod stats;
mod storage;
mod update;
mod validate;
mod view;

use iced::futures::{SinkExt, channel::mpsc};
use iced::{Size, Subscription, Task, Theme};

use model::App;
use update::{Message, update};
use view::theme::{DARK_PALETTE, LIGHT_PALETTE};
use view::view;

pub fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("待办清单 · Quick Todo")
        // 主题：System → 跟随系统主题显式映射（system_dark 经订阅实时更新）；
        // Light / Dark → 固定模式。**恒返回 Some**——iced 的 None 跟随（`Theme::default`）
        // 在“手动模式 → Auto”切换时用旧模式解析默认主题，且原生窗口边框先跟随系统，
        // 造成“边框白 / 内容深”分裂；显式映射保证窗口背景与内容始终一致。
        // 两套自定义调色板（view::LIGHT_PALETTE / DARK_PALETTE，浅「晴空」/ 深「夜航」），
        // 经 `Theme::custom` 自动派生 `extended_palette()`，样式层零改动。
        .theme(|app: &App| -> Option<Theme> {
            Some(if app.is_dark() {
                Theme::custom("QuickTodo Dark", DARK_PALETTE)
            } else {
                Theme::custom("QuickTodo Light", LIGHT_PALETTE)
            })
        })
        .window(iced::window::Settings {
            min_size: Some(Size::new(480.0, 360.0)),
            ..Default::default()
        })
        .window_size(Size::new(880.0, 640.0))
        .subscription(subscription)
        .run()
}

/// 应用启动：初始化状态，异步加载持久化的任务列表，并获取当前系统主题。
fn boot() -> (App, Task<Message>) {
    (
        App::default(),
        Task::batch([
            Task::perform(storage::load(), Message::Loaded),
            // 初始系统主题（深色？）：供「跟随系统」模式显式映射
            iced::system::theme()
                .map(|mode| Message::SystemThemeChanged(mode == iced::theme::Mode::Dark)),
            // 图标字体（Material Symbols Outlined，编译期嵌入 assets/fonts/）：
            // iced 0.14 的 font::Error 为空枚举、加载失败不可观测（损坏字节静默表现为豆腐块），
            // 故仅作完成信号；失败缓解 = dev-time 校验（体积 / 家族名）+ 手动视觉验证
            iced::font::load(
                include_bytes!("../assets/fonts/MaterialSymbolsOutlined.ttf").as_slice(),
            )
            .map(|_| Message::FontLoaded),
        ]),
    )
}

/// 每秒产生一个时钟消息，驱动"进行中"任务的实时耗时显示；
/// 订阅系统主题变化（Auto 模式实时跟随）；
/// 任一弹窗（任务 / 项目添加）打开时，额外监听 Esc 键关闭对应弹窗。
fn subscription(app: &App) -> Subscription<Message> {
    let clock = Subscription::run(|| {
        iced::stream::channel(
            1,
            |mut sender: mpsc::Sender<chrono::DateTime<chrono::Utc>>| async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
                loop {
                    interval.tick().await;
                    // 订阅被取消（应用退出）时发送失败，结束循环
                    if sender.send(chrono::Utc::now()).await.is_err() {
                        break;
                    }
                }
            },
        )
    })
    .map(Message::Tick);

    // 系统主题实时跟随（仅 Auto 模式消费；事件由 iced_winit 从 winit ThemeChanged 转发）
    let system_theme = iced::system::theme_changes()
        .map(|mode| Message::SystemThemeChanged(mode == iced::theme::Mode::Dark));

    // 任一弹窗或下拉菜单打开时：Esc 关闭当前弹窗 / 菜单（与点击遮罩等效）
    // 注意：listen_with 只接受无捕获的 fn 指针，因此固定发出 CloseActiveDialog
    if app.add_dialog.is_some()
        || app.project_dialog.is_some()
        || app.type_dialog.is_some()
        || app.show_completed
        || app.show_stats
        || app.add_menu_open
    {
        let esc = iced::event::listen_with(|event, _status, _window| {
            if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event
                && key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
            {
                Some(Message::CloseActiveDialog)
            } else {
                None
            }
        });
        Subscription::batch([clock, system_theme, esc])
    } else {
        Subscription::batch([clock, system_theme])
    }
}
