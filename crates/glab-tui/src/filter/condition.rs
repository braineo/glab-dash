use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Field {
    Assignee,
    Author,
    Reviewer,
    Label,
    Milestone,
    State,
    Draft,
    ApprovedBy,
    Title,
    Project,
    Team,
    Iteration,
    Weight,
}

impl Field {
    pub fn all() -> &'static [Field] {
        &[
            Field::Assignee,
            Field::Author,
            Field::Reviewer,
            Field::Label,
            Field::Milestone,
            Field::State,
            Field::Draft,
            Field::ApprovedBy,
            Field::Title,
            Field::Project,
            Field::Team,
            Field::Iteration,
            Field::Weight,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Field::Assignee => "assignee",
            Field::Author => "author",
            Field::Reviewer => "reviewer",
            Field::Label => "label",
            Field::Milestone => "milestone",
            Field::State => "state",
            Field::Draft => "draft",
            Field::ApprovedBy => "approved_by",
            Field::Title => "title",
            Field::Project => "project",
            Field::Team => "team",
            Field::Iteration => "iteration",
            Field::Weight => "weight",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "assignee" => Some(Field::Assignee),
            "author" => Some(Field::Author),
            "reviewer" => Some(Field::Reviewer),
            "label" => Some(Field::Label),
            "milestone" => Some(Field::Milestone),
            "state" => Some(Field::State),
            "draft" => Some(Field::Draft),
            "approved_by" => Some(Field::ApprovedBy),
            "title" => Some(Field::Title),
            "project" => Some(Field::Project),
            "team" => Some(Field::Team),
            "iteration" => Some(Field::Iteration),
            "weight" => Some(Field::Weight),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Op {
    Eq,
    Neq,
    Contains,
    NotContains,
}

impl Op {
    pub fn all() -> &'static [Op] {
        &[Op::Eq, Op::Neq, Op::Contains, Op::NotContains]
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Op::Eq => "=",
            Op::Neq => "!=",
            Op::Contains => "~",
            Op::NotContains => "!~",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "eq" | "=" => Some(Op::Eq),
            "neq" | "!=" => Some(Op::Neq),
            "contains" | "~" => Some(Op::Contains),
            "not_contains" | "!~" => Some(Op::NotContains),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FilterCondition {
    pub field: Field,
    pub op: Op,
    pub value: String,
}

impl FilterCondition {
    pub fn display(&self) -> String {
        format!("{}{}{}", self.field.name(), self.op.symbol(), self.value)
    }
}

impl std::fmt::Display for FilterCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

pub fn matches_issue(
    item: &TrackedIssue,
    conditions: &[FilterCondition],
    me: &str,
    team_members: &[String],
) -> bool {
    conditions.iter().all(|c| {
        let value = resolve_value(&c.value, me);
        match c.field {
            Field::Assignee => match_string_list(
                &item
                    .issue
                    .assignees
                    .iter()
                    .map(|u| u.username.as_str())
                    .collect::<Vec<_>>(),
                &c.op,
                &value,
            ),
            Field::Author => match_optional_string(
                item.issue.author.as_ref().map(|u| u.username.as_str()),
                &c.op,
                &value,
            ),
            Field::Label => match_string_list(
                &item
                    .issue
                    .labels
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                &c.op,
                &value,
            ),
            Field::Milestone => match_optional_string(
                item.issue.milestone.as_ref().map(|m| m.title.as_str()),
                &c.op,
                &value,
            ),
            Field::State => match_string(&item.issue.state, &c.op, &value),
            Field::Title => match_string_contains(&item.issue.title, &c.op, &value),
            Field::Project => match_string(&item.project_path, &c.op, &value),
            Field::Team => match_team_membership(
                &item
                    .issue
                    .assignees
                    .iter()
                    .map(|u| u.username.clone())
                    .collect::<Vec<_>>(),
                &c.op,
                &value,
                team_members,
            ),
            Field::Iteration => match_optional_string(
                item.issue.iteration.as_ref().map(|i| i.title.as_str()),
                &c.op,
                &value,
            ),
            Field::Weight => {
                let w = item.issue.weight.unwrap_or(0).to_string();
                match_string(&w, &c.op, &value)
            }
            // Issue doesn't have these fields
            Field::Reviewer | Field::Draft | Field::ApprovedBy => true,
        }
    })
}

pub fn matches_mr(
    item: &TrackedMergeRequest,
    conditions: &[FilterCondition],
    me: &str,
    team_members: &[String],
) -> bool {
    conditions.iter().all(|c| {
        let value = resolve_value(&c.value, me);
        match c.field {
            Field::Assignee => match_string_list(
                &item
                    .mr
                    .assignees
                    .iter()
                    .map(|u| u.username.as_str())
                    .collect::<Vec<_>>(),
                &c.op,
                &value,
            ),
            Field::Author => match_optional_string(
                item.mr.author.as_ref().map(|u| u.username.as_str()),
                &c.op,
                &value,
            ),
            Field::Reviewer => match_string_list(
                &item
                    .mr
                    .reviewers
                    .iter()
                    .map(|u| u.username.as_str())
                    .collect::<Vec<_>>(),
                &c.op,
                &value,
            ),
            Field::Label => match_string_list(
                &item
                    .mr
                    .labels
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                &c.op,
                &value,
            ),
            Field::Milestone => match_optional_string(
                item.mr.milestone.as_ref().map(|m| m.title.as_str()),
                &c.op,
                &value,
            ),
            Field::State => match_string(&item.mr.state, &c.op, &value),
            Field::Draft => {
                let is_draft = item.mr.draft || item.mr.work_in_progress;
                match_bool(is_draft, &c.op, &value)
            }
            Field::ApprovedBy => match_string_list(
                &item
                    .mr
                    .approved_by
                    .iter()
                    .map(|a| a.user.username.as_str())
                    .collect::<Vec<_>>(),
                &c.op,
                &value,
            ),
            Field::Title => match_string_contains(&item.mr.title, &c.op, &value),
            Field::Project => match_string(&item.project_path, &c.op, &value),
            // MRs don't have iteration/weight
            Field::Iteration | Field::Weight => true,
            Field::Team => match_team_membership(
                &item
                    .mr
                    .assignees
                    .iter()
                    .map(|u| u.username.clone())
                    .collect::<Vec<_>>(),
                &c.op,
                &value,
                team_members,
            ),
        }
    })
}

fn resolve_value(value: &str, me: &str) -> String {
    if value == "$me" {
        me.to_string()
    } else {
        value.to_string()
    }
}

fn match_string(field_val: &str, op: &Op, value: &str) -> bool {
    match op {
        Op::Eq => field_val.eq_ignore_ascii_case(value),
        Op::Neq => !field_val.eq_ignore_ascii_case(value),
        Op::Contains => field_val.to_lowercase().contains(&value.to_lowercase()),
        Op::NotContains => !field_val.to_lowercase().contains(&value.to_lowercase()),
    }
}

fn match_string_contains(field_val: &str, op: &Op, value: &str) -> bool {
    let lower_field = field_val.to_lowercase();
    let lower_val = value.to_lowercase();
    match op {
        Op::Eq | Op::Contains => lower_field.contains(&lower_val),
        Op::Neq | Op::NotContains => !lower_field.contains(&lower_val),
    }
}

fn match_optional_string(field_val: Option<&str>, op: &Op, value: &str) -> bool {
    if value == "none" {
        return match op {
            Op::Eq => field_val.is_none(),
            Op::Neq => field_val.is_some(),
            _ => true,
        };
    }
    match field_val {
        Some(v) => match_string(v, op, value),
        None => matches!(op, Op::Neq | Op::NotContains),
    }
}

fn match_string_list(items: &[&str], op: &Op, value: &str) -> bool {
    if value == "none" {
        return match op {
            Op::Eq => items.is_empty(),
            Op::Neq => !items.is_empty(),
            _ => true,
        };
    }
    let has = items.iter().any(|s| s.eq_ignore_ascii_case(value));
    match op {
        Op::Eq | Op::Contains => has,
        Op::Neq | Op::NotContains => !has,
    }
}

fn match_bool(field_val: bool, op: &Op, value: &str) -> bool {
    let expected = matches!(value, "true" | "yes" | "1");
    match op {
        Op::Eq | Op::Contains => field_val == expected,
        Op::Neq | Op::NotContains => field_val != expected,
    }
}

fn match_team_membership(
    assignees: &[String],
    op: &Op,
    _value: &str,
    team_members: &[String],
) -> bool {
    let has_team_member = assignees
        .iter()
        .any(|a| team_members.iter().any(|m| m.eq_ignore_ascii_case(a)));
    match op {
        Op::Eq | Op::Contains => has_team_member,
        Op::Neq | Op::NotContains => !has_team_member,
    }
}
