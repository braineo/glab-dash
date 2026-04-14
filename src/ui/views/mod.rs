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

