//! 设计令牌（统一视觉规范）：字号 / 间距 / 圆角 / 按钮规格 / 容器宽度 / 遮罩色 /
//! 分体按钮宽度 / 输入框聚焦 widget Id。
//!
//! 全部视觉尺寸与颜色值集中于此，view 层渲染只引用令牌常量，不出现裸数值；
//! 修改视觉规范时只需改此文件。

/// 下拉菜单距窗口顶部的偏移：内容区 padding 24 + 标题行高（26px 字号 ≈34）
/// + 按钮垂直居中（按钮底 ≈55）+ 9px 间隙；字体 / DPI 变化时在此校准
pub(crate) const ADD_MENU_TOP: f32 = 64.0;

/// 分体按钮主按钮「＋ 添加任务」固定宽：文本（5 全角 + 1 半角空格 ≈ 5.5×字号 ≈71.5）
/// + BTN_MEDIUM 左右内边距 24 + 4px 裁剪余量（字形度量偏差时防裁剪；不影响对齐）
pub(crate) const ADD_MAIN_WIDTH: f32 = FONT_BODY * 5.5 + BTN_MEDIUM.left + BTN_MEDIUM.right + 4.0;
/// 分体按钮箭头「▾」固定宽：字形（≈ 字号）+ BTN_ARROW 左右内边距 16 + 4px 余量
pub(crate) const ADD_ARROW_WIDTH: f32 = FONT_BODY + BTN_ARROW.left + BTN_ARROW.right + 4.0;
/// 分体按钮总宽（主按钮 + 箭头 + 1px 间距）；下拉菜单「＋ 添加项目」与其等宽——
/// 菜单右缘与组合右缘均贴窗口右缘 − 24px，等宽即左右边缘与文字起点三方对齐
pub(crate) const ADD_BUTTONS_WIDTH: f32 = ADD_MAIN_WIDTH + ADD_ARROW_WIDTH + 1.0;

/// 字号：标题栏标题
pub(crate) const FONT_TITLE: f32 = 26.0;
/// 字号：弹窗标题
pub(crate) const FONT_DIALOG_TITLE: f32 = 20.0;
/// 字号：任务卡片标题
pub(crate) const FONT_CARD_TITLE: f32 = 16.0;
/// 字号：任务区标题行 / 分区标题 / 弹窗操作按钮
pub(crate) const FONT_HEADER: f32 = 14.0;
/// 字号：正文 / 按钮 / 芯片
pub(crate) const FONT_BODY: f32 = 13.0;
/// 字号：小字（计数 / 辅助按钮）
pub(crate) const FONT_SMALL: f32 = 12.0;
/// 字号：标签（表单字段名 / 状态小字）
pub(crate) const FONT_TINY: f32 = 11.0;
/// 字号：微字（优先级圆点 ●）
pub(crate) const FONT_MICRO: f32 = 10.0;

/// 间距：2px（卡片元信息行内）
pub(crate) const SPACE_XXS: f32 = 2.0;
/// 间距：4px
pub(crate) const SPACE_XS: f32 = 4.0;
/// 间距：6px
pub(crate) const SPACE_S: f32 = 6.0;
/// 间距：8px
pub(crate) const SPACE_M: f32 = 8.0;
/// 间距：12px
pub(crate) const SPACE_L: f32 = 12.0;

/// 圆角：卡片 / 弹窗 / 分组容器 / 错误横幅
pub(crate) const RADIUS_CARD: f32 = 10.0;
/// 圆角：胶囊（芯片 / 徽章 / 摘要状态条）
pub(crate) const RADIUS_PILL: f32 = 999.0;

/// 按钮规格：弹窗主操作 / 取消 [8, 18]
pub(crate) const BTN_LARGE: iced::Padding = iced::Padding {
    top: 8.0,
    right: 18.0,
    bottom: 8.0,
    left: 18.0,
};
/// 按钮规格：常规（分体主按钮 / 芯片）[6, 12]
pub(crate) const BTN_MEDIUM: iced::Padding = iced::Padding {
    top: 6.0,
    right: 12.0,
    bottom: 6.0,
    left: 12.0,
};
/// 按钮规格：分体下拉箭头 [6, 8]
pub(crate) const BTN_ARROW: iced::Padding = iced::Padding {
    top: 6.0,
    right: 8.0,
    bottom: 6.0,
    left: 8.0,
};
/// 按钮规格：行内 / 卡片辅助操作 [4, 10]
pub(crate) const BTN_SMALL: iced::Padding = iced::Padding {
    top: 4.0,
    right: 10.0,
    bottom: 4.0,
    left: 10.0,
};
/// 按钮规格：chip 上下文编辑 / 删除 [2, 8]
pub(crate) const BTN_TINY: iced::Padding = iced::Padding {
    top: 2.0,
    right: 8.0,
    bottom: 2.0,
    left: 8.0,
};
/// 按钮规格：卡片开始 / 完成 / 删除 / 保存 [6, 14]
pub(crate) const BTN_CARD: iced::Padding = iced::Padding {
    top: 6.0,
    right: 14.0,
    bottom: 6.0,
    left: 14.0,
};

/// 容器：窗口内容边距
pub(crate) const PADDING_PAGE: f32 = 24.0;
/// 容器：分组容器 / 卡片内边距
pub(crate) const PADDING_PANEL: f32 = 12.0;
/// 容器：弹窗卡片内边距
pub(crate) const PADDING_DIALOG: f32 = 20.0;
/// 容器：三弹窗统一宽度
pub(crate) const DIALOG_WIDTH: f32 = 480.0;
/// 容器：归档弹窗高度
pub(crate) const DIALOG_HEIGHT: f32 = 480.0;
/// 容器：统计弹窗宽度（含图表区）
pub(crate) const STATS_DIALOG_WIDTH: f32 = 560.0;
/// 容器：统计弹窗高度
pub(crate) const STATS_DIALOG_HEIGHT: f32 = 560.0;
/// 图表：统计图表画布固定高度
pub(crate) const CHART_HEIGHT: f32 = 150.0;
/// 表单：标签列宽
pub(crate) const LABEL_WIDTH: f32 = 72.0;
/// 表单：编辑面板优先级列宽
pub(crate) const PRIORITY_COLUMN_WIDTH: f32 = 140.0;
/// 遮罩：弹窗半透明黑
pub(crate) const SCRIM_COLOR: iced::Color = iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55);

/// 弹窗标题输入框的 widget Id（打开弹窗时聚焦用）
pub(crate) const DIALOG_TITLE_ID: iced::widget::Id = iced::widget::Id::new("add-dialog-title");

/// 项目弹窗名称输入框的 widget Id（打开弹窗时聚焦用）
pub(crate) const PROJECT_DIALOG_NAME_ID: iced::widget::Id =
    iced::widget::Id::new("add-project-dialog-name");

/// 项目内联编辑名称输入框的 widget Id（进入编辑态时聚焦用）
pub(crate) const PROJECT_EDIT_NAME_ID: iced::widget::Id =
    iced::widget::Id::new("edit-project-name");

/// 类型弹窗名称输入框的 widget Id（打开弹窗时聚焦用）
pub(crate) const TYPE_DIALOG_NAME_ID: iced::widget::Id =
    iced::widget::Id::new("add-type-dialog-name");

/// 类型栏内联编辑名称输入框的 widget Id（进入编辑态时聚焦用）
pub(crate) const TYPE_EDIT_NAME_ID: iced::widget::Id = iced::widget::Id::new("edit-type-name");
