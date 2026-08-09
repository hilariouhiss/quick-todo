---
project: iced-demo
description: Iced Todos 桌面待办应用 —— Rust + iced 0.14（Elm 架构），带时间追踪与 JSON 持久化
---

# AGENTS.md

本文件为 AI 编码代理（Vibe Coding）提供项目上下文、架构约定与开发约束。
**改动任何行为之前，请先阅读 `docs/方案.md`（设计文档）与本文档。**

## 1. 项目概览

一个使用 [Iced](https://iced.rs) 0.14 开发的待办清单（Todos）桌面应用，核心特性：

- 添加 / 开始 / 完成 / 删除任务，每个任务记录**创建 / 开始 / 结束**三个时间点，可带可选**描述**
- 项目侧边栏可**收起 / 展开**（纯 UI 状态，不持久化，启动默认收起）
- 进行中任务显示**每秒实时**刷新耗时
- 任务列表 JSON 持久化，重启不丢失
- 深色主题，中文界面

技术栈：

| 项            | 选择                                                                          |
| ------------- | ----------------------------------------------------------------------------- |
| 语言 / 工具链 | Rust edition 2024（rustc / cargo 1.97.x）                                     |
| UI 框架       | iced 0.14，函数式 API（`iced::application`），特性：tokio、debug、time-travel |
| 时间          | chrono 0.4（serde 特性）                                                      |
| ID            | uuid v7（时间有序）                                                           |
| 持久化        | serde_json + tokio::fs + directories                                          |
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
├── view.rs     视图：项目侧边栏（可收放）+ 任务区（输入行、任务卡片、时间元信息）
├── storage.rs  持久化：异步 JSON 读写（Store = 任务 + 项目，兼容旧格式）
docs/
└── 方案.md     设计文档 —— 需求 R1-R10、架构图、验收标准，改行为前必读
```

数据流（Elm 架构）：**视图产生 Message → update 更新状态 → 视图重新渲染**；
一次性副作用经 `Task`、持续流经 `Subscription` 注入，状态本身保持纯数据。

## 4. 核心设计约定（不可破坏）

这些是该项目的架构决策，改动或扩展功能时必须遵守：

1. **状态由时间字段推导，不存储状态枚举。** `Todo` 只有 `created_at / started_at / finished_at` 三个时间字段，`TodoStatus`（Pending / InProgress / Done）由 `status()` 推导。时间字段是唯一事实来源——**严禁**为 `Todo` 增加持久化的状态字段，否则会破坏"状态与时间不可能不一致"的结构保证。
2. **update 是纯函数。** 所有时间戳一律取自 `app.now`（由每秒 `Tick` 消息刷新），**不要**在 update 里直接调用 `Utc::now()`。副作用只通过返回的 `Task` 表达。
3. **时间统一 UTC 存储**（`DateTime<Utc>`），仅在 view 层用 `chrono::Local` 格式化展示。
4. **持久化 fire-and-forget**：每次状态变更后把整个 `Store`（todos + projects）`serde_json::to_string_pretty` 序列化，经 `Task::perform(storage::save, Message::Saved)` 异步写盘；`Saved` 成功消息静默，失败写入 `app.error`。不引入增量同步。
5. **数据向后兼容**：`Todo.project_id` 与 `Todo.description` 带 `#[serde(default)]`；`storage::load` 对旧版纯数组格式自动迁移为 `Store`，不得破坏旧数据。
6. **项目语义**：项目名 trim 后非空且不重名；删除项目时其下任务 `project_id` 置 `None`（不级联删任务）；被删项目处于筛选/编辑态时同步复位。
7. **新任务插在最前**（`todos.insert(0, ...)`）；标题与描述输入均自动 `trim()`，空白标题静默忽略且保留输入框内容；空白描述存为空字符串（`Todo.description` 恒为 `String`，空串 = 无描述，卡片不显示空描述行）。
8. **侧边栏收放是纯 UI 状态**（`App.sidebar_visible`）：只存内存、**不参与持久化**，启动默认收起；`ToggleSidebar` 不触发落盘。
9. **非法状态流转静默拒绝**：仅 Pending 可开始、仅 InProgress 可完成，其他情况不产生任何副作用。
10. **错误不崩溃**：数据文件缺失视为空数据；损坏 JSON 返回错误并显示在 UI（`app.error`），绝不 panic。
11. **UI 文案与代码注释使用中文**；模块级 `//!` + 公开项 `///` 文档注释是标配。

## 5. 代码风格

- 模块职责单一：model = 纯数据、update = 纯逻辑、view = 渲染、storage = IO；新增功能先判断归属，不跨层写代码。
- view 层不持有业务逻辑；派生展示（状态徽章、摘要、耗时格式化、项目计数、筛选）放在 view 内部私有函数。
- 颜色、字体等视觉常量用模块级 `const`，如 `MUTED / ACCENT / DONE`。
- 新依赖直接在 Cargo.toml 的 `[dependencies]` 中声明（仅原生目标，无 wasm 分块）。
- 提交前运行 `cargo fmt` 与 `cargo clippy`，保持零警告。

## 6. 测试约定

- 测试写在各模块内 `#[cfg(test)] mod tests`，与被测代码同文件。
- 纯逻辑用 `#[test]`；异步（storage IO）用 `#[tokio::test]`。
- storage 测试使用临时文件（`std::env::temp_dir()` + 进程 id 命名），用后删除。
- 时间相关测试用固定时间戳（`Utc.timestamp_opt(...)`）构造，不用 `Utc::now()` 断言。
- **任何新的 Message / 状态流转 / 序列化变更必须补对应测试**，`cargo test` 全绿是交付前提。

## 7. iced 0.14 要点与陷阱

- **函数式 API 类型推断**：`iced::application(boot, update, view)` 的 `App` / `Message` 泛型由闭包推断，入口与各函数签名**不要**显式标注泛型参数，否则编译失败。
- `view` 返回 `Element<'_, Message>`；生命周期由 iced 管理，注意 `Element<'static, Message>` 与借用版本的区别。
- 订阅用 `Subscription::run(|| iced::stream::channel(1, ...))`；**发送失败（`send().await.is_err()`）必须 break**，否则应用退出后流不会结束。
- 窗口配置是独立的 builder 方法：`.window(...)`（含 `min_size`）与 `.window_size(...)` 分开设置。
- 主题跟随：样式函数签名 `fn style(theme: &iced::Theme, status: ...)`，颜色取自 `theme.extended_palette()`（背景弱化色、成功色等），保证深浅主题自适应。
- 中文渲染依赖系统字体回退（Windows 下微软雅黑），无需配置字体资源。
- 调试特性：`debug` 特性提供内置调试面板，`time-travel` 提供时间旅行调试，开发期保持启用。

## 8. 开发工作流（Vibe Coding 指引）

1. **先读文档**：涉及行为/数据模型改动时，先读 `docs/方案.md`，设计与文档冲突时先更新文档再改代码；新功能先编写计划文档到 `.pi/plan/` 再实现。
2. **善用代码索引**：本仓库已配置 CodeGraph（`codegraph_search` / `codegraph_explore` / `codegraph_callers` 等），查符号与调用关系优先用它们，其次才 grep/read。
3. **小步提交**：仓库当前尚无 commit（git 未初始化历史），功能完成即可做首次提交；提交信息用中文或英文均可，使用 Conventional Commits 风格。
4. **变更闭环**：改代码 → 补测试 → `cargo test` → `cargo clippy` → `cargo run` 手动验证 UI 行为。
5. **验收对照**：功能完成时对照 `docs/方案.md` 第 8 节验收标准逐条核对。
