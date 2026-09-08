use crate::app::View;
use crate::binding_group;
use crate::keybindings::BindingGroup;

binding_group! {
    /// Walking a detail view's conversation and acting on the thread under the
    /// cursor.  Both detail views answer to this identically, so they share one
    /// group.  It is innermost in the chain, so `c` here means "reply to this
    /// thread" rather than the focused item's "add comment" — and `x` is left
    /// alone, so it still closes the issue or merge request.
    pub DETAIL_NAV_GROUP: "Conversation" {
        ('j') => MoveDown | "j/k" "Move down/up",
        (key Down) => MoveDown,
        (ctrl 'n') => MoveDown,
        ('k') => MoveUp,
        (key Up) => MoveUp,
        (ctrl 'p') => MoveUp,
        ('g') => Top | "g/G" "First / last row",
        ('G') => Bottom,
        (ctrl 'd') => PageDown | "^d/^u" "Page down/up",
        (ctrl 'u') => PageUp,
        ('c') => ReplyThread | "c" "Reply to this thread",
        ('C') => NewThread | "C" "Start a new thread",
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

/// Owns all per-view component state.  Lives on `App` as a single field
/// so it can be borrowed independently from shared state (issues, mrs,
/// config, …), enabling recursive event dispatch where views handle
/// their own keys.
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

/// A list view: the list itself, then the filtering wrapped around it.
static LIST_CHAIN: &[&BindingGroup] = &[&list_model::LIST_NAV_GROUP, &list_model::FILTER_GROUP];

/// The board puts its own focus and column keys ahead of the list's, so its
/// `Tab` wins over the filter bar's.
static BOARD_CHAIN: &[&BindingGroup] = &[
    &dashboard::BOARD_NAV_GROUP,
    &list_model::LIST_NAV_GROUP,
    &list_model::FILTER_GROUP,
];

/// Planning puts its column keys ahead of the focused column's list.
static PLANNING_CHAIN: &[&BindingGroup] = &[
    &planning::PLANNING_NAV_GROUP,
    &list_model::LIST_NAV_GROUP,
    &list_model::FILTER_GROUP,
];

/// A detail view scrolls and replies; it has no list and nothing to filter.
static DETAIL_CHAIN: &[&BindingGroup] = &[&DETAIL_NAV_GROUP];

impl Views {
    /// The groups `view` composes, innermost first.  The view→groups map lives
    /// here, with the container that already knows every view state, so the
    /// dispatcher never learns the list a second time.
    pub fn binding_groups(view: View) -> &'static [&'static BindingGroup] {
        match view {
            View::Dashboard => BOARD_CHAIN,
            View::IssueList | View::MrList => LIST_CHAIN,
            View::IssueDetail | View::MrDetail => DETAIL_CHAIN,
            View::Planning => PLANNING_CHAIN,
        }
    }
}
