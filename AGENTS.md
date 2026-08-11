---
project: quick-todo
description: Quick Todo 桌面待办应用 —— Rust + iced 0.14（Elm 架构），带时间追踪与 SQLite 持久化
---

# AGENTS.md

本文件为 AI 编码代理（Vibe Coding）提供项目上下文、架构约定与开发约束。
**改动任何行为之前，请先阅读 `docs/需求与概要设计.md`（设计文档）与本文档。**

## 1. 项目概览

一个使用 [Iced](https://iced.rs) 0.14 开发的待办清单（Todos）桌面应用，核心特性：

- 添加 / 开始 / 完成 / 删除任务，每个任务记录**创建 / 开始 / 结束**三个时间点，可带可选**描述**
- 任务卡片默认**只读**展示全部属性；点击「编辑」进入编辑模式（即"当前任务"），可修改标题 / 描述 / 项目 / 截止时间
- 项目以**单行横向滚动芯片**展示在任务列表上方（圆点 + 名称 + 计数，点击筛选，「全部」恒最前，选中主色高亮）；选中后右端出现「编辑 / 删除」，编辑经项目栏下方展开的**全宽编辑面板**（纯 UI 状态，不持久化）
- 项目通过标题栏**分体按钮**「＋ 添加任务 ▾」下拉菜单中的「＋ 添加项目」**弹窗**创建（名称必填），可带可选**起止时间**（芯片悬停 tooltip 展示，编辑面板可改）；**任务弹窗内可快速新建项目**（复用新建项目弹窗，创建后自动选中）
- 任务区**双列分组**：左=未开始、右=进行中（组内按截止时间排序）；已完成经底部 footer 统计「已完成 x」链接弹窗归档；任务添加统一走「＋ 添加任务」弹窗
- 任务 / 项目可带可选**优先级**（无/低/中/高）；任务与项目列表可切换排序（优先级 / 截止日期 / 综合），偏好**持久化**到数据文件
- 进行中任务显示**每秒实时**刷新耗时
- 任务 / 项目存 SQLite（quick-todo.db），排序偏好与主题模式存 settings.json，重启不丢失
- 主题跟随系统（浅 / 深自动切换），可手动循环切换（跟随系统 / 浅色 / 深色，偏好持久化）；视觉规范由 view.rs 顶部「设计令牌」常量统一（字号 / 间距 / 圆角 / 按钮规格）

技术栈：

| 项            | 选择                                                                          |
| ------------- | ----------------------------------------------------------------------------- |
| 语言 / 工具链 | Rust edition 2024（rustc / cargo 1.97.x）                                     |
| UI 框架       | iced 0.14，函数式 API（`iced::application`），特性：tokio、debug、time-travel |
| 时间          | chrono 0.4                                                                    |
| ID            | uuid v7（时间有序）                                                           |
| 持久化        | rusqlite 0.40（bundled）+ serde_json（仅 settings.json）                      |
| 测试          | 内置单元测试 + iced_test（dev-dependency）                                    |

## 2. 常用命令

```bash
cargo run          # 运行桌面应用
cargo test         # 运行全部测试（含 #[tokio::test]）
cargo build        # 构建（debug）
cargo build --release
cargo clippy       # 静态检查（保持零警告）
cargo fmt          # 代码格式化
```

## 3. 项目结构

```
src/
├── main.rs     入口：iced::application 装配（boot / update / view / subscription）
├── model.rs    数据模型：Todo、Project、TodoStatus、App（纯数据，无 IO）
├── update.rs   Message 枚举 + update 纯函数（状态流转、副作用派发）
├── view.rs     视图：设计令牌常量 + 标题栏（分体按钮 + 下拉菜单）+ 项目单行栏（横向滚动芯片）+ 编辑面板 + 任务区（任务卡片/编辑模式、时间元信息）+ 弹窗（任务/项目添加）+ 底部 footer（主题指示器 + 统计文本横条）
├── storage.rs  持久化：SQLite（quick-todo.db）+ settings.json，按 Op 增量写盘
docs/
└── 需求与概要设计.md     需求与概要设计文档 —— 需求 R1-R27 + 非功能 N1-N7、架构图、验收标准，改行为前必读
```

数据流（Elm 架构）：**视图产生 Message → update 更新状态 → 视图重新渲染**；
一次性副作用经 `Task`、持续流经 `Subscription` 注入，状态本身保持纯数据。

## 4. 核心设计约定（不可破坏）

这些是该项目的架构决策，改动或扩展功能时必须遵守：

1. **状态由时间字段推导，不存储状态枚举。** `Todo` 只有 `created_at / started_at / finished_at` 三个时间字段，`TodoStatus`（Pending / InProgress / Done）由 `status()` 推导。时间字段是唯一事实来源——**严禁**为 `Todo` 增加持久化的状态字段，否则会破坏"状态与时间不可能不一致"的结构保证。
2. **update 是纯函数。** 所有时间戳一律取自 `app.now`（由每秒 `Tick` 消息刷新），**不要**在 update 里直接调用 `Utc::now()`。副作用只通过返回的 `Task` 表达。
3. **时间统一 UTC 存储**（`DateTime<Utc>`），仅在 view 层用 `chrono::Local` 格式化展示。
4. **持久化 fire-and-forget**：每次数据变更派发一个 `storage::Op`（携带完整行状态），经 `Task::perform(storage::apply, Message::Saved)` 在**单事务**内执行对应 SQL；排序偏好经 `storage::save_settings` 整文件覆写 `settings.json`（无读-改-写）。`Saved` 成功消息静默，失败写入 `app.error`。增量写、无写队列；rusqlite 同步 API 一律经 `spawn_blocking` 包裹，不阻塞 UI 线程。
5. **数据不兼容旧版本**（开发阶段破坏性更新）：任务 / 项目存 SQLite 单文件（`quick-todo.db`，可执行文件同目录），schema 变更即破坏性更新——旧库不兼容直接报错，不做自动迁移；排序偏好与主题模式存独立 `settings.json`（缺**文件**取默认「综合」/「跟随系统」；`theme_mode` 为**必填键**——旧文件缺键解析失败红字提示，不迁移）。两个文件缺失视为空数据。
6. **项目语义**：项目名 trim 后非空且不重名；项目添加走弹窗（标题栏分体按钮「▾」下拉菜单中的「＋ 添加项目」`OpenProjectDialog` / `SubmitProjectDialog`，完整属性），也可在**任务弹窗内快速新建**（`OpenQuickProjectDialog`：弹出与标题栏相同的新建项目弹窗，**保留任务弹窗**，校验与弹窗一致——重名红字提示、保持打开、输入保留；创建成功自动选中新项目、焦点回落标题框，创建成功才落盘 `Op::InsertProject`）；可带可选起止时间（`Project.started_at` / `finished_at`；**开始必须早于结束**）；编辑走项目栏下方展开的**全宽编辑面板**（`StartEditProject` / `SaveEditProject`，名称 + 起止时间可改，**重名校验排除自身**）；删除项目时其下任务 `project_id` 置 `None`（不级联删任务）；被删项目处于筛选/编辑态时同步复位。
7. **新任务插在最前**（`todos.insert(0, ...)`）；**任务添加统一走「＋ 添加任务」弹窗**（`SubmitAddDialog`，`App.input` / 快捷输入行已移除）；标题 / 描述 / 项目名输入均自动 `trim()`，空白标题静默忽略且保留输入框内容；空白描述存为空字符串（`Todo.description` 恒为 `String`，空串 = 无描述，卡片不显示空描述行）；优先级（`Option<Priority>`）在弹窗 / 编辑表单设置，未设置不显示徽章、排序排最后；弹窗校验不过（空白标题 / 截止时间格式非法 / 项目不存在）时弹窗保持打开、输入保留；时间取自 `app.now`。**任务弹窗不持有快速新建项目状态**：点「＋ 新建」经 `OpenQuickProjectDialog` 复用新建项目弹窗（`App.project_dialog`），任务弹窗内容保留，创建成功后 `AddDialog.project_id` 自动选中新项目。
8. **项目筛选、弹窗表单、下拉菜单、归档开关与编辑表单是纯 UI 状态**（`App.selected_project` / `App.add_dialog` / `App.project_dialog` / `App.show_completed` / `App.add_menu_open` / `App.project_edit` / `App.todo_edit`）：只存内存、**不参与持久化**，启动默认全部 / 关闭 / 无编辑；各弹窗与编辑表单的打开/关闭/输入变化均不触发落盘（`SubmitAddDialog` 创建成功、`SubmitProjectDialog` 创建成功、`SaveEditProject` / `SaveEditTodo` 保存成功才落盘）；任务 / 项目 / 归档三个弹窗**互斥**（打开一个关闭其余），**例外：任务弹窗内「＋ 新建」（`OpenQuickProjectDialog`）叠加打开项目弹窗**——`add_dialog` 保留，视图叠加层项目弹窗优先渲染，Esc / 遮罩 / 取消仅关闭项目弹窗并返回任务弹窗（输入保留）；标题栏下拉菜单（`ToggleAddMenu`）打开时打开任一弹窗即自动收起（`OpenAddDialog` / `OpenProjectDialog` / `OpenCompletedDialog` 清 `add_menu_open`），点击外部 / Esc / 再点「▾」关闭；`CloseActiveDialog` 按「项目弹窗 → 任务弹窗 → 下拉菜单 → 归档」顺序关闭。
    **例外：排序与主题偏好持久化**（`App.sort_mode` / `App.project_sort_mode` / `App.theme_mode`，R22/R24/R26）——存独立 `settings.json`（`storage::save_settings`，不入 SQLite）、启动经 `Loaded` 恢复，`SortModeChanged` / `ProjectSortModeChanged` / `CycleThemeMode` 切换即触发落盘。
9. **非法状态流转静默拒绝**：仅 Pending 可开始、仅 InProgress 可完成，其他情况不产生任何副作用。
10. **错误不崩溃**：数据文件缺失视为空数据；损坏数据库 / settings.json 返回错误并显示在 UI（`app.error`），绝不 panic。
11. **UI 文案与代码注释使用中文**；模块级 `//!` + 公开项 `///` 文档注释是标配。
12. **卡片默认只读，修改须进编辑模式**：主界面任务卡片全部属性只读展示（项目归属也是只读文字，无 `AssignProject` 消息——归属只能经编辑模式保存）；点击「编辑」进入该卡片的编辑模式（即"当前任务"，`App.todo_edit`），可改标题 / 描述 / 项目 / 截止时间，保存校验同弹窗；**时间字段（创建 / 开始 / 结束）永不直接编辑**（自动记录，状态由它们推导）；切换编辑其他卡片时未保存修改被丢弃。
13. **双列分组与归档**：任务区双列——左=未开始、右=进行中（各自独立滚动，组内按排序偏好排序、未设置均排最后、稳定排序，`Todo::due_order_key` / `priority_order_key` / `combined_order_key` 为排序键）；已完成任务不进双列，经底部 footer 统计「已完成 x」链接弹窗归档（按 `finished_at` 降序，**不受排序偏好影响**）；分组 / 排序属派生展示，放 view 内部私有函数，update 层不改列表顺序。
14. **排序偏好、主题模式与优先级**：任务区右上角（统一标题行右端）与项目单行栏最左侧各自独立排序下拉（均无文字标签）（`sort_mode` / `project_sort_mode`，值：优先级 / 截止日期 / 综合=优先级优先同级按截止）；「综合」的截止键：任务=`due_at`、项目=`finished_at`（项目结束时间即截止日期）；「全部」芯片恒在项目栏最前；优先级展示：卡片徽章「高/中/低」（高红/中橙/低灰）、项目芯片彩色圆点；`Priority`（低<中<高，不序列化）、`SortMode`（库 / settings.json 存英文变体名，缺省 `Combined`）与 `ThemeMode`（**System / Light / Dark，settings.json 必填键**——旧文件缺键解析失败红字提示不迁移，缺省 `System`）——`SortMode` / `ThemeMode` 派生 `Serialize/Deserialize/Default`；`.theme()` 闭包**恒返回 `Some(Theme::custom(…))`**（两套自定义调色板）：System → 按 `App.is_dark()`（`App.system_dark` 经 `iced::system::theme_changes` 订阅实时更新，不持久化）显式映射，Light / Dark → 固定板，view 层语义色取板共用同一 `is_dark` 判定（iced 原生 `None` 跟随在手动 → Auto 切换时窗口边框 / 内容主题分裂，故不用）。

## 5. 代码风格

- 模块职责单一：model = 纯数据、update = 纯逻辑、view = 渲染、storage = IO；新增功能先判断归属，不跨层写代码。
- view 层不持有业务逻辑；派生展示（状态徽章、摘要、耗时格式化、项目计数、筛选）放在 view 内部私有函数。
- 颜色、字体等视觉常量用模块级 `const`；主题颜色为两套自定义 `Palette`（`LIGHT_PALETTE`「晴空」/ `DARK_PALETTE`「夜航」）+ 语义色双板 `SemColors`（`muted / blue / done / accent / error`），对比度由 view 测试锁定（≥ 4.5:1）。
- 新依赖直接在 Cargo.toml 的 `[dependencies]` 中声明（仅原生目标，无 wasm 分块）。
- 提交前运行 `cargo fmt` 与 `cargo clippy`，保持零警告。

## 6. 测试约定

- 测试写在各模块内 `#[cfg(test)] mod tests`，与被测代码同文件。
- 纯逻辑用 `#[test]`；异步（storage IO）用 `#[tokio::test]`。
- storage 测试使用临时文件（`std::env::temp_dir()` + 进程 id 命名，临时 `.db` / `settings.json`），用后删除。
- 时间相关测试用固定时间戳（`Utc.timestamp_opt(...)`）构造，不用 `Utc::now()` 断言。
- **任何新的 Message / 状态流转 / 持久化变更必须补对应测试**，`cargo test` 全绿是交付前提。

## 7. iced 0.14 要点与陷阱

- **函数式 API 类型推断**：`iced::application(boot, update, view)` 的 `App` / `Message` 泛型由闭包推断，入口与各函数签名**不要**显式标注泛型参数，否则编译失败。
- `view` 返回 `Element<'_, Message>`；生命周期由 iced 管理，注意 `Element<'static, Message>` 与借用版本的区别。
- 订阅用 `Subscription::run(|| iced::stream::channel(1, ...))`；**发送失败（`send().await.is_err()`）必须 break**，否则应用退出后流不会结束。
- 窗口配置是独立的 builder 方法：`.window(...)`（含 `min_size`）与 `.window_size(...)` 分开设置。
- 主题跟随：样式函数签名 `fn style(theme: &iced::Theme, status: ...)`，颜色取自 `theme.extended_palette()`（背景弱化色、成功色等），保证深浅主题自适应。
- 中文渲染依赖系统字体回退（Windows 下微软雅黑），无需配置字体资源。
- 调试特性：`debug` 特性提供内置调试面板，`time-travel` 提供时间旅行调试，开发期保持启用。

## 8. 开发工作流（Vibe Coding 指引）

1. **先读文档**：涉及行为/数据模型改动时，先读 `docs/需求与概要设计.md`，设计与文档冲突时先更新文档再改代码；新功能先编写计划文档到 `.pi/plan/` 再实现。
2. **善用代码索引**：本仓库已配置 CodeGraph（`codegraph_search` / `codegraph_explore` / `codegraph_callers` 等），查符号与调用关系优先用它们，其次才 grep/read。
3. **小步提交（提交门禁）**：功能完成后**必须**依次通过 `cargo fmt`（无差异）、`cargo clippy --all-targets`（零警告）、`cargo test`（全绿）**才能提交**；未通过门禁不得 commit，也不得用 `--no-verify` 绕过。提交信息用中文或英文均可，使用 Conventional Commits 风格。
4. **变更闭环**：改代码 → 补测试 → `cargo fmt` → `cargo test` → `cargo clippy --all-targets` → `cargo run` 手动验证 UI 行为。
5. **验收对照**：功能完成时对照 `docs/需求与概要设计.md` 第 5 节验收标准逐条核对。
