//! The GraphQL documents, kept together so a selection is written once and
//! every query and mutation that needs it spreads the same fragment.

/// Page size for merge-request list queries. Kept small to stay within GitLab's
/// default query complexity limit of 250 (each MR node with nested discussions
/// contributes ~5 points).
pub(crate) const MR_PAGE_SIZE: u32 = 25;

/// The user selection every other fragment spreads in turn.
const USER_FIELDS: &str = r"
    fragment UserFields on User {
        id username name webUrl
    }
";

/// The selection every merge-request query and mutation shares.
pub(crate) const MR_FIELDS: &str = r"
    fragment MrFields on MergeRequest {
        id iid title state draft
        author { ...UserFields }
        assignees { nodes { ...UserFields } }
        reviewers { nodes { ...UserFields } }
        labels { nodes { title } }
        milestone { title }
        createdAt updatedAt webUrl description
        userNotesCount
        sourceBranch targetBranch
        reference(full: true)
        diffStatsSummary { additions deletions fileCount }
        approved
        approvedBy { nodes { ...UserFields } }
        headPipeline { status }
        resolvableDiscussionsCount
        resolvedDiscussionsCount
    }
";

/// The selection the root `issues` query uses, deserialized straight into
/// [`glab_core::domain::Issue`].
pub(crate) const ISSUE_FIELDS: &str = r"
    fragment IssueFields on Issue {
        id iid title state
        author { ...UserFields }
        assignees { nodes { ...UserFields } }
        labels { nodes { title } }
        milestone { title }
        createdAt updatedAt closedAt webUrl description
        userNotesCount
        reference(full: true)
        status { name category }
        iteration { id title startDate dueDate state }
        weight
    }
";

/// The selection both work-item documents share — the `namespace.workItems`
/// list query and the `workItemUpdate` mutation. Widgets arrive as a
/// heterogeneous array, so this path still needs `GqlWorkItem` and
/// `From<GqlWorkItem> for Issue` to fold into the flat `Issue` shape.
pub(crate) const WORK_ITEM_FIELDS: &str = r"
    fragment WorkItemFields on WorkItem {
        id iid title state
        author { ...UserFields }
        createdAt updatedAt closedAt webUrl
        reference(full: true)
        widgets(onlyTypes: [STATUS, ASSIGNEES, LABELS, MILESTONE, DESCRIPTION, ITERATION, WEIGHT]) {
            ... on WorkItemWidgetAssignees {
                assignees { nodes { ...UserFields } }
            }
            ... on WorkItemWidgetLabels {
                labels { nodes { title } }
            }
            ... on WorkItemWidgetMilestone {
                milestone { title }
            }
            ... on WorkItemWidgetStatus {
                status { name category }
            }
            ... on WorkItemWidgetDescription {
                description
            }
            ... on WorkItemWidgetIteration {
                iteration { id title startDate dueDate state }
            }
            ... on WorkItemWidgetWeight {
                weight
            }
        }
    }
";

/// `doc` followed by the fragments it spreads: `fields` itself, and the
/// `UserFields` every one of them spreads in turn.
pub(crate) fn document(doc: &str, fields: &str) -> String {
    format!("{doc}{fields}{USER_FIELDS}")
}
