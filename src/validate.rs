//! 表单校验（单一来源）：任务表单（弹窗 / 卡片编辑共用）与项目表单（弹窗 / 编辑面板共用）。
//!
//! view 层据此派生按钮禁用与红字提示，update 层据此做防御性拒绝——同一规则只有一份实现，
//! 杜绝"改一处漏一处"的双份校验漂移。本模块为纯函数，不持有状态、不产生副作用。
//!
//! **语义决策（行为零回归，见计划 B 节）**：
//! - `can_submit_todo` **不含** `missing_project`——与现状 view 的 can_submit 逐位一致
//!   （项目被删后按钮仍可点，提交被 update 防御性拒绝）；`missing_project` 仅作 update
//!   提交检查（`todo_form_values`）；
//! - 消息级守卫（拒绝设置已删项目）与提交校验分离：`DialogProjectChanged` /
//!   `EditProjectChanged` 用 `project_exists`，提交用 `todo_form_values`；
//! - hint 触发条件与现状一致：重名仅非空名提示、范围错误仅双端解析成功时提示、
//!   空白名仅禁用不提示。

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::model::{Project, TodoType};

/// 任务表单校验结果（弹窗添加与卡片编辑保存共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoFormIssues {
    /// 标题 trim 后为空（仅禁用按钮，不提示）
    pub blank_title: bool,
    /// 截止时间解析失败（红字提示）
    pub invalid_due: bool,
    /// 所属项目已不存在（仅 update 提交检查用，不参与按钮禁用——见模块文档语义决策）
    pub missing_project: bool,
    /// 任务类型已不存在（同上，与 `missing_project` 同一语义约定）
    pub missing_type: bool,
}

/// 任务表单校验（纯函数）：title / 截止时间解析结果 / 归属项目 / 任务类型。
pub fn todo_form_issues(
    title: &str,
    due_parsed: &Result<Option<DateTime<Utc>>, String>,
    project_id: Option<Uuid>,
    type_id: Option<Uuid>,
    projects: &[Project],
    types: &[TodoType],
) -> TodoFormIssues {
    TodoFormIssues {
        blank_title: title.trim().is_empty(),
        invalid_due: due_parsed.is_err(),
        missing_project: !project_exists(projects, project_id),
        missing_type: !type_exists(types, type_id),
    }
}

/// 任务表单按钮是否可提交：**不含** `missing_project` / `missing_type`（见模块文档语义决策）。
pub fn can_submit_todo(issues: &TodoFormIssues) -> bool {
    !issues.blank_title && !issues.invalid_due
}

/// 项目表单校验结果（弹窗添加与编辑面板保存共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFormIssues {
    /// 名称 trim 后为空（仅禁用按钮，不提示）
    pub blank_name: bool,
    /// 名称与既有项目重名（仅非空名时判定；红字提示）
    pub name_conflict: bool,
    /// 开始时间解析失败（红字提示）
    pub invalid_start: bool,
    /// 结束时间解析失败（红字提示）
    pub invalid_end: bool,
    /// 起止时间同时设置且开始 ≥ 结束（仅双端解析成功时判定；红字提示）
    pub range_invalid: bool,
}

/// 项目表单校验（纯函数）：名称 / 起止时间解析结果；`exclude_id` = 重名校验排除自身的项目 id
/// （弹窗传 `None`，编辑面板传被编辑项目 id）。
pub fn project_form_issues(
    name: &str,
    exclude_id: Option<Uuid>,
    start_parsed: &Result<Option<DateTime<Utc>>, String>,
    end_parsed: &Result<Option<DateTime<Utc>>, String>,
    projects: &[Project],
) -> ProjectFormIssues {
    let name = name.trim();
    let name_conflict = !name.is_empty()
        && projects
            .iter()
            .any(|p| p.name == name && exclude_id.is_none_or(|id| p.id != id));
    let range_invalid = match (start_parsed, end_parsed) {
        (Ok(Some(start)), Ok(Some(finish))) => start >= finish,
        _ => false,
    };
    ProjectFormIssues {
        blank_name: name.is_empty(),
        name_conflict,
        invalid_start: start_parsed.is_err(),
        invalid_end: end_parsed.is_err(),
        range_invalid,
    }
}

/// 项目表单按钮是否可提交。
pub fn can_submit_project(issues: &ProjectFormIssues) -> bool {
    !issues.blank_name
        && !issues.name_conflict
        && !issues.invalid_start
        && !issues.invalid_end
        && !issues.range_invalid
}

/// 项目表单校验通过后的提交值：(trim 后名称, 开始时间, 结束时间)。
pub type ProjectFormOutput = (String, Option<DateTime<Utc>>, Option<DateTime<Utc>>);

/// 项目表单校验并提取提交值；`Err(issues)` = 校验失败（update 提交路径用）。
pub fn project_form_values(
    name: &str,
    exclude_id: Option<Uuid>,
    start_parsed: &Result<Option<DateTime<Utc>>, String>,
    end_parsed: &Result<Option<DateTime<Utc>>, String>,
    projects: &[Project],
) -> Result<ProjectFormOutput, ProjectFormIssues> {
    let issues = project_form_issues(name, exclude_id, start_parsed, end_parsed, projects);
    if can_submit_project(&issues) {
        // can_submit 已保证两端解析均为 Ok；Err 不可达，防御性回落 None
        Ok((
            name.trim().to_owned(),
            start_parsed.clone().unwrap_or(None),
            end_parsed.clone().unwrap_or(None),
        ))
    } else {
        Err(issues)
    }
}

/// 类型表单校验结果（弹窗添加与编辑面板保存共用；类型仅名称字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeFormIssues {
    /// 名称 trim 后为空（仅禁用按钮，不提示）
    pub blank_name: bool,
    /// 名称与既有类型重名（仅非空名时判定；红字提示）
    pub name_conflict: bool,
}

/// 类型表单校验（纯函数）：名称；`exclude_id` = 重名校验排除自身的类型 id
/// （弹窗传 `None`，编辑面板传被编辑类型 id）。
pub fn type_form_issues(
    name: &str,
    exclude_id: Option<Uuid>,
    types: &[TodoType],
) -> TypeFormIssues {
    let name = name.trim();
    let name_conflict = !name.is_empty()
        && types
            .iter()
            .any(|t| t.name == name && exclude_id.is_none_or(|id| t.id != id));
    TypeFormIssues {
        blank_name: name.is_empty(),
        name_conflict,
    }
}

/// 类型表单按钮是否可提交。
pub fn can_submit_type(issues: &TypeFormIssues) -> bool {
    !issues.blank_name && !issues.name_conflict
}

/// 任务表单校验通过后的提交值：(trim 后标题, 类型 id, 截止时间)。
pub type TodoFormOutput = (String, Option<Uuid>, Option<DateTime<Utc>>);

/// 任务表单校验并提取提交值；`Err(issues)` = 校验失败（update 提交路径用）。
/// 与 `can_submit_todo` 不同：**含** `missing_project` / `missing_type` 检查
/// （提交必须拒绝不存在的项目 / 类型）。
pub fn todo_form_values(
    title: &str,
    due_parsed: &Result<Option<DateTime<Utc>>, String>,
    project_id: Option<Uuid>,
    type_id: Option<Uuid>,
    projects: &[Project],
    types: &[TodoType],
) -> Result<TodoFormOutput, TodoFormIssues> {
    let issues = todo_form_issues(title, due_parsed, project_id, type_id, projects, types);
    if can_submit_todo(&issues) && !issues.missing_project && !issues.missing_type {
        // can_submit 已保证 due_parsed 为 Ok；Err 不可达，防御性回落 None
        Ok((
            title.trim().to_owned(),
            type_id,
            due_parsed.clone().unwrap_or(None),
        ))
    } else {
        Err(issues)
    }
}

/// 项目 id 是否命中既有项目（`None` = 无项目，恒合法；消息级守卫与提交校验共用）。
pub fn project_exists(projects: &[Project], id: Option<Uuid>) -> bool {
    id.is_none_or(|id| projects.iter().any(|p| p.id == id))
}

/// 类型 id 是否命中既有类型（`None` = 无类型，恒合法；消息级守卫与提交校验共用）。
pub fn type_exists(types: &[TodoType], id: Option<Uuid>) -> bool {
    id.is_none_or(|id| types.iter().any(|t| t.id == id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn projects() -> Vec<Project> {
        vec![
            Project::new("工作".into(), dt(1_000)),
            Project::new("生活".into(), dt(2_000)),
        ]
    }

    fn types() -> Vec<TodoType> {
        vec![
            TodoType::new_full("学习".into(), dt(1_000)),
            TodoType::new_full("健康".into(), dt(2_000)),
        ]
    }

    fn ok_due(secs: i64) -> Result<Option<DateTime<Utc>>, String> {
        Ok(Some(dt(secs)))
    }

    fn err_due() -> Result<Option<DateTime<Utc>>, String> {
        Err("时间格式：2026-01-31 或 2026-01-31 18:30".into())
    }

    // ---------- 任务表单 ----------

    #[test]
    fn todo_issues_blank_title_whitespace_only() {
        for title in ["", "   ", "\t"] {
            let issues = todo_form_issues(title, &Ok(None), None, None, &[], &[]);
            assert!(issues.blank_title, "{title:?} 应为空白标题");
            assert!(!issues.invalid_due);
            assert!(!issues.missing_project);
            assert!(!issues.missing_type);
        }
    }

    #[test]
    fn todo_issues_invalid_due_and_missing_project() {
        let projects = projects();
        let missing = projects[0].id; // 存在 → 不缺失
        let issues = todo_form_issues("写方案", &err_due(), Some(missing), None, &projects, &[]);
        assert!(issues.invalid_due);
        assert!(!issues.missing_project);

        let issues = todo_form_issues(
            "写方案",
            &Ok(None),
            Some(Uuid::now_v7()),
            None,
            &projects,
            &[],
        );
        assert!(!issues.invalid_due);
        assert!(issues.missing_project);
    }

    #[test]
    fn todo_issues_missing_type() {
        let types = types();
        // 类型存在 → 不缺失
        let issues = todo_form_issues("写方案", &Ok(None), None, Some(types[0].id), &[], &types);
        assert!(!issues.missing_type);
        // 类型不存在 → 缺失（仅提交检查用）
        let issues = todo_form_issues("写方案", &Ok(None), None, Some(Uuid::now_v7()), &[], &types);
        assert!(issues.missing_type);
    }

    #[test]
    fn todo_issues_all_clear() {
        let projects = projects();
        let types = types();
        let issues = todo_form_issues(
            " 写方案 ",
            &ok_due(1_700_000_000),
            Some(projects[0].id),
            Some(types[0].id),
            &projects,
            &types,
        );
        assert!(!issues.blank_title);
        assert!(!issues.invalid_due);
        assert!(!issues.missing_project);
        assert!(!issues.missing_type);
    }

    /// 语义锁定：can_submit_todo 不含 missing_project / missing_type
    /// （与现状 view 按钮行为逐位一致）。
    #[test]
    fn can_submit_todo_ignores_missing_project_and_type() {
        let projects = projects();
        let types = types();
        // 仅 missing_project 为真：按钮仍可点（现状语义），提交由 todo_form_values 拒绝
        let issues = todo_form_issues(
            "写方案",
            &Ok(None),
            Some(Uuid::now_v7()),
            None,
            &projects,
            &types,
        );
        assert!(can_submit_todo(&issues));
        assert!(issues.missing_project);
        assert!(
            todo_form_values(
                "写方案",
                &Ok(None),
                Some(Uuid::now_v7()),
                None,
                &projects,
                &types
            )
            .is_err()
        );

        // 仅 missing_type 为真：按钮仍可点，提交被拒
        let issues = todo_form_issues(
            "写方案",
            &Ok(None),
            None,
            Some(Uuid::now_v7()),
            &projects,
            &types,
        );
        assert!(can_submit_todo(&issues));
        assert!(issues.missing_type);
        assert!(
            todo_form_values(
                "写方案",
                &Ok(None),
                None,
                Some(Uuid::now_v7()),
                &projects,
                &types,
            )
            .is_err()
        );
    }

    #[test]
    fn todo_form_values_extracts_trimmed_values() {
        let projects = projects();
        let types = types();
        let (title, type_id, due) = todo_form_values(
            "  写方案  ",
            &ok_due(1_700_000_000),
            Some(projects[0].id),
            Some(types[0].id),
            &projects,
            &types,
        )
        .unwrap();
        assert_eq!(title, "写方案");
        assert_eq!(type_id, Some(types[0].id));
        assert_eq!(due, Some(dt(1_700_000_000)));

        // 无截止 / 无类型
        let (_, type_id, due) =
            todo_form_values("写方案", &Ok(None), None, None, &projects, &types).unwrap();
        assert_eq!(type_id, None);
        assert_eq!(due, None);
    }

    #[test]
    fn todo_form_values_rejects_invalid() {
        let projects = projects();
        let types = types();
        // 空白标题
        assert!(todo_form_values("   ", &Ok(None), None, None, &projects, &types).is_err());
        // 时间非法
        assert!(todo_form_values("写方案", &err_due(), None, None, &projects, &types).is_err());
        // 项目不存在
        assert!(
            todo_form_values(
                "写方案",
                &Ok(None),
                Some(Uuid::now_v7()),
                None,
                &projects,
                &types
            )
            .is_err()
        );
        // 类型不存在
        assert!(
            todo_form_values(
                "写方案",
                &Ok(None),
                None,
                Some(Uuid::now_v7()),
                &projects,
                &types
            )
            .is_err()
        );
    }

    // ---------- 项目表单 ----------

    #[test]
    fn project_issues_blank_name() {
        let issues = project_form_issues("  ", None, &Ok(None), &Ok(None), &[]);
        assert!(issues.blank_name);
        // 空白名不触发重名提示（仅禁用）
        assert!(!issues.name_conflict);
        assert!(!can_submit_project(&issues));
    }

    #[test]
    fn project_issues_name_conflict_with_exclusion() {
        let projects = projects();
        let (work, life) = (projects[0].id, projects[1].id);

        // 弹窗（不排除）：与既有项目重名
        let issues = project_form_issues("工作", None, &Ok(None), &Ok(None), &projects);
        assert!(issues.name_conflict);
        assert!(!can_submit_project(&issues));

        // 编辑面板（排除自身）：与自身同名放行
        let issues = project_form_issues("工作", Some(work), &Ok(None), &Ok(None), &projects);
        assert!(!issues.name_conflict);
        assert!(can_submit_project(&issues));

        // 编辑面板：与其他项目重名仍拒绝
        let issues = project_form_issues("生活", Some(work), &Ok(None), &Ok(None), &projects);
        assert!(issues.name_conflict);
        assert!(!can_submit_project(&issues));

        // 排除 id 不存在于列表（防御）：视为不排除
        let issues = project_form_issues(
            "生活",
            Some(Uuid::now_v7()),
            &Ok(None),
            &Ok(None),
            &projects,
        );
        assert!(issues.name_conflict);
        let _ = life;
    }

    #[test]
    fn project_issues_time_parsing_and_range() {
        let start = ok_due(1_700_000_000);
        let end = ok_due(1_700_100_000);

        // 解析失败
        let issues = project_form_issues("项目", None, &err_due(), &Ok(None), &[]);
        assert!(issues.invalid_start);
        assert!(!issues.range_invalid); // 单端 Err 不触发范围提示
        let issues = project_form_issues("项目", None, &Ok(None), &err_due(), &[]);
        assert!(issues.invalid_end);

        // 开始 < 结束：合法
        let issues = project_form_issues("项目", None, &start, &end, &[]);
        assert!(!issues.range_invalid);
        assert!(can_submit_project(&issues));

        // 开始 ≥ 结束：范围错误
        let issues = project_form_issues("项目", None, &end, &start, &[]);
        assert!(issues.range_invalid);
        assert!(!can_submit_project(&issues));

        // 仅设置一端：不触发范围提示
        let issues = project_form_issues("项目", None, &start, &Ok(None), &[]);
        assert!(!issues.range_invalid);
        assert!(can_submit_project(&issues));
        let issues = project_form_issues("项目", None, &Ok(None), &end, &[]);
        assert!(!issues.range_invalid);
        assert!(can_submit_project(&issues));
    }

    #[test]
    fn project_form_values_extracts_and_rejects() {
        let projects = projects();
        let start = ok_due(1_700_000_000);
        let end = ok_due(1_700_100_000);

        // 提取 trim 名称与起止时间
        let (name, s, e) =
            project_form_values("  项目 A  ", None, &start, &end, &projects).unwrap();
        assert_eq!(name, "项目 A");
        assert_eq!(s, Some(dt(1_700_000_000)));
        assert_eq!(e, Some(dt(1_700_100_000)));

        // 仅设置一端
        let (_, s, e) = project_form_values("项目 A", None, &start, &Ok(None), &projects).unwrap();
        assert_eq!(s, Some(dt(1_700_000_000)));
        assert_eq!(e, None);

        // 全部拒绝路径
        assert!(project_form_values("   ", None, &Ok(None), &Ok(None), &projects).is_err()); // 空名
        assert!(project_form_values("工作", None, &Ok(None), &Ok(None), &projects).is_err()); // 重名
        assert!(project_form_values("项目", None, &err_due(), &Ok(None), &[]).is_err()); // 时间非法
        assert!(project_form_values("项目", None, &end, &start, &[]).is_err()); // 范围错误
    }

    // ---------- 类型存在性 ----------

    #[test]
    fn type_exists_semantics() {
        let types = types();
        assert!(type_exists(&types, None)); // 无类型恒合法
        assert!(type_exists(&types, Some(types[0].id)));
        assert!(!type_exists(&types, Some(Uuid::now_v7())));
        assert!(!type_exists(&[], Some(Uuid::now_v7())));
    }

    // ---------- 类型表单 ----------

    #[test]
    fn type_issues_blank_name() {
        let issues = type_form_issues("  ", None, &[]);
        assert!(issues.blank_name);
        // 空白名不触发重名提示（仅禁用）
        assert!(!issues.name_conflict);
        assert!(!can_submit_type(&issues));
    }

    #[test]
    fn type_issues_name_conflict_with_exclusion() {
        let types = types();
        let (study, health) = (types[0].id, types[1].id);

        // 弹窗（不排除）：与既有类型重名
        let issues = type_form_issues("学习", None, &types);
        assert!(issues.name_conflict);
        assert!(!can_submit_type(&issues));

        // 编辑面板（排除自身）：与自身同名放行
        let issues = type_form_issues("学习", Some(study), &types);
        assert!(!issues.name_conflict);
        assert!(can_submit_type(&issues));

        // 编辑面板：与其他类型重名仍拒绝
        let issues = type_form_issues("健康", Some(study), &types);
        assert!(issues.name_conflict);
        assert!(!can_submit_type(&issues));

        // 排除 id 不存在于列表（防御）：视为不排除
        let issues = type_form_issues("健康", Some(Uuid::now_v7()), &types);
        assert!(issues.name_conflict);
        let _ = health;
    }

    #[test]
    fn type_form_values_trimmed_and_unique() {
        // 名称 trim 后非空且不重名 → 可提交
        let types = types();
        let issues = type_form_issues("  阅读  ", None, &types);
        assert!(!issues.blank_name);
        assert!(!issues.name_conflict);
        assert!(can_submit_type(&issues));

        // 重名 → 不可提交（红字提示）
        let issues = type_form_issues("学习", None, &types);
        assert!(issues.name_conflict);
        assert!(!can_submit_type(&issues));
    }

    // ---------- 项目存在性 ----------

    #[test]
    fn project_exists_semantics() {
        let projects = projects();
        assert!(project_exists(&projects, None)); // 无项目恒合法
        assert!(project_exists(&projects, Some(projects[0].id)));
        assert!(!project_exists(&projects, Some(Uuid::now_v7())));
        assert!(!project_exists(&[], Some(Uuid::now_v7())));
    }
}
