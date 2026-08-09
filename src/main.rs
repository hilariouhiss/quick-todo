//! Iced Todos 桌面应用 —— 程序入口。
//!
//! 使用 iced 0.14 的函数式 API（`iced::application`）装配整个应用：
//! boot 初始化状态并异步加载数据，update 处理消息，view 渲染界面，
//! subscription 提供每秒时钟以刷新"进行中"任务的实时耗时。

mod model;
mod storage;
mod update;
mod view;

use iced::futures::{SinkExt, channel::mpsc};
use iced::{Size, Subscription, Task, Theme};

use model::App;
use update::{Message, update};
use view::view;

pub fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("待办清单 · Iced Todos")
        .theme(Theme::Dark)
        .window(iced::window::Settings {
            min_size: Some(Size::new(480.0, 360.0)),
            ..Default::default()
        })
        .window_size(Size::new(880.0, 640.0))
        .subscription(subscription)
        .run()
}

/// 应用启动：初始化状态，并异步加载持久化的任务列表。
fn boot() -> (App, Task<Message>) {
    (
        App::default(),
        Task::perform(storage::load(), Message::Loaded),
    )
}

/// 每秒产生一个时钟消息，驱动"进行中"任务的实时耗时显示；
/// 弹窗添加任务打开时，额外监听 Esc 键关闭弹窗。
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

    if app.add_dialog.is_some() {
        // 弹窗打开时：Esc 关闭弹窗（与点击遮罩 / 取消按钮等效）
        let esc = iced::event::listen_with(|event, _status, _window| {
            if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event
                && key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
            {
                Some(Message::CloseAddDialog)
            } else {
                None
            }
        });
        Subscription::batch([clock, esc])
    } else {
        clock
    }
}
