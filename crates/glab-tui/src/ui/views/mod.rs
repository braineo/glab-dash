use glab_core::domain::{Item, ItemKind, RelatedItem};

use crate::app::View;
use crate::binding_group;
use crate::keybindings::BindingGroup;

pub struct DetailCtx<'a> {
    pub kind: ItemKind,
    pub gid: String,
    pub related: &'a [RelatedItem],
}

impl<'a> DetailCtx<'a> {
    pub fn of(item: &impl Item, related: &'a [RelatedItem]) -> Self {
        Self {
            kind: item.kind(),
            gid: item.gid().to_string(),
            related,
        }
    }
}

binding_group! {
    /// Innermost in the chain, so `c` here means "reply to this thread" rather
    /// than the focused item's "add comment".
    pub DETAIL_NAV_GROUP: "Conversation" {
        ('j') => MoveDown | "j/k" "Move down/up",
        (key Down) => MoveDown,
        (ctrl 'n') => MoveDown,
        ('k') => MoveUp,
        (key Up) => MoveUp,
        (ctrl 'p') => MoveUp,
        ('J') => NextUnresolved | "J/K" "Next / prev unresolved",
        ('K') => PrevUnresolved,
        ('g') => Top | "g/G" "First / last row",
        ('G') => Bottom,
        (ctrl 'v') => PageDown | "^v/M-v" "Page down/up",
        (alt 'v') => PageUp,
        ('c') => ReplyThread | "c" "Reply to this thread",
        ('C') => NewThread | "C" "Start a new thread",
        ('e') => EditComment | "e" "Edit this comment",
        (' ') => ResolveThread | "Space" "Resolve / unresolve",
        (key Tab) => ToggleThread | "Tab" "Fold thread",
    }
}

pub mod dashboard;
pub mod filter_editor;
pub mod issue_detail;
pub mod issue_list;
pub mod list_model;
pub mod mr_detail;
pub mod mr_list;
pub mod planning;

/// One `App` field, so it borrows separately from the shared data and a view
/// can handle its own keys.
#[derive(Default)]
pub struct Views {
    pub issue_list: issue_list::IssueListState,
    pub mr_list: mr_list::MrListState,
    pub issue_detail: issue_detail::IssueDetailState,
    pub mr_detail: mr_detail::MrDetailState,
    pub planning: planning::PlanningViewState,
    pub board: dashboard::IterationBoardState,
    pub health: Option<dashboard::IterationHealth>,
}

static LIST_CHAIN: &[&BindingGroup] = &[&list_model::LIST_NAV_GROUP, &list_model::FILTER_GROUP];

/// The board's keys go ahead of the list's, so its `Tab` wins over the filter
/// bar's.
static BOARD_CHAIN: &[&BindingGroup] = &[
    &dashboard::BOARD_NAV_GROUP,
    &list_model::LIST_NAV_GROUP,
    &list_model::FILTER_GROUP,
];

static PLANNING_CHAIN: &[&BindingGroup] = &[
    &planning::PLANNING_NAV_GROUP,
    &list_model::LIST_NAV_GROUP,
    &list_model::FILTER_GROUP,
];

/// A detail view has no list and nothing to filter, so it composes neither.
static ISSUE_DETAIL_CHAIN: &[&BindingGroup] = &[&issue_detail::ISSUE_LINK_GROUP, &DETAIL_NAV_GROUP];

static MR_DETAIL_CHAIN: &[&BindingGroup] = &[&mr_detail::MR_LINK_GROUP, &DETAIL_NAV_GROUP];

impl Views {
    /// Innermost first.
    pub fn binding_groups(view: View) -> &'static [&'static BindingGroup] {
        match view {
            View::Dashboard => BOARD_CHAIN,
            View::IssueList | View::MrList => LIST_CHAIN,
            View::IssueDetail => ISSUE_DETAIL_CHAIN,
            View::MrDetail => MR_DETAIL_CHAIN,
            View::Planning => PLANNING_CHAIN,
        }
    }
}
