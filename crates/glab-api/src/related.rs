use std::collections::HashSet;

use anyhow::{Context, Result};
use serde::Deserialize;

use glab_core::domain::{ItemKind, ItemRef, RelatedItem, Relation};

use crate::client::{GitLabClient, get_mutation_payload};
use crate::wire::{Nodes, normalize_state};

impl GitLabClient {
    pub async fn list_related(&self, kind: ItemKind, gid: &str) -> Result<Vec<RelatedItem>> {
        match kind {
            ItemKind::Issue => self.list_issue_related(gid).await,
            ItemKind::MergeRequest => self.list_mr_related(gid).await,
        }
    }

    pub async fn add_link(
        &self,
        item_gid: &str,
        target_gid: &str,
        relation: Relation,
    ) -> Result<()> {
        let json = self
            .graphql_once(
                "workItemAddLinkedItems",
                LINK_MUTATION,
                serde_json::json!({
                    "id": item_gid,
                    "targets": [target_gid],
                    "linkType": encode_link_type(relation)?,
                }),
            )
            .await?;
        get_mutation_payload(&json, "workItemAddLinkedItems").map(|_| ())
    }

    /// GitLab reads this from the description rather than storing a link, so
    /// it is read-modify-write; a line already present is left alone.
    pub async fn mention_in_mr(
        &self,
        gid: &str,
        target_gid: &str,
        relation: Relation,
    ) -> Result<()> {
        let json = self
            .graphql(
                "mrMention",
                MENTION_QUERY,
                serde_json::json!({ "id": gid, "target": target_gid }),
            )
            .await?;
        let mention: Mention =
            serde_json::from_value(json.pointer("/data").cloned().unwrap_or_default())
                .context("failed to deserialize mrMention response")?;
        let (Some(mr), Some(target)) = (mention.merge_request, mention.work_item) else {
            anyhow::bail!("no merge request {gid} or no item {target_gid}");
        };

        let line = mention_line(&target.reference, relation);
        let description = mr.description.unwrap_or_default();
        if description.contains(&line) {
            return Ok(());
        }
        let description = if description.trim().is_empty() {
            line
        } else {
            format!("{}\n\n{line}", description.trim_end())
        };
        self.mr_mutation(
            "mergeRequestUpdate",
            "MergeRequestUpdateInput",
            &mr.project.full_path,
            &mr.iid,
            serde_json::json!({ "description": description }),
        )
        .await
        .map(|_| ())
    }

    pub async fn unlink(&self, item_gid: &str, target_gid: &str) -> Result<()> {
        let json = self
            .graphql_once(
                "workItemRemoveLinkedItems",
                UNLINK_MUTATION,
                serde_json::json!({ "id": item_gid, "targets": [target_gid] }),
            )
            .await?;
        get_mutation_payload(&json, "workItemRemoveLinkedItems").map(|_| ())
    }

    async fn list_issue_related(&self, gid: &str) -> Result<Vec<RelatedItem>> {
        let json = self
            .graphql(
                "issueRelated",
                ISSUE_RELATED_QUERY,
                serde_json::json!({ "id": gid }),
            )
            .await?;
        let Some(widgets) = json.pointer("/data/workItem/widgets") else {
            return Ok(Vec::new());
        };
        let widgets: Vec<RelationWidget> = serde_json::from_value(widgets.clone())
            .context("failed to deserialize issueRelated widgets")?;
        Ok(widgets
            .into_iter()
            .flat_map(RelationWidget::into_related)
            .collect())
    }

    async fn list_mr_related(&self, gid: &str) -> Result<Vec<RelatedItem>> {
        let json = self
            .graphql(
                "mrRelated",
                MR_RELATED_QUERY,
                serde_json::json!({ "id": gid }),
            )
            .await?;
        let Some(linked) = json.pointer("/data/mergeRequest/linkedWorkItems") else {
            return Ok(Vec::new());
        };
        let linked: Vec<MrLinkedItem> = serde_json::from_value(linked.clone())
            .context("failed to deserialize mrRelated linkedWorkItems")?;
        Ok(linked
            .into_iter()
            .filter_map(MrLinkedItem::into_related)
            .collect())
    }
}

/// `closing` wins the overlap: an item that settles the other also shows up in
/// the wider collection as merely related.
fn fold(closing: Vec<RelatedItem>, mentioning: Vec<RelatedItem>) -> Vec<RelatedItem> {
    let closed: HashSet<ItemRef> = closing.iter().map(|r| r.item.clone()).collect();
    closing
        .into_iter()
        .chain(mentioning.into_iter().filter(|r| !closed.contains(&r.item)))
        .collect()
}

/// GitLab acts on `Closes`; the wording of the rest is for the reader alone.
fn mention_line(reference: &str, relation: Relation) -> String {
    let keyword = match relation {
        Relation::Closes => "Closes",
        _ => "Related to",
    };
    format!("{keyword} {reference}")
}

fn encode_link_type(relation: Relation) -> Result<&'static str> {
    Ok(match relation {
        Relation::BlockedBy => "BLOCKED_BY",
        Relation::Blocks => "BLOCKS",
        Relation::RelatesTo => "RELATED",
        derived => anyhow::bail!("GitLab derives \"{}\" — it is not a link", derived.label()),
    })
}

#[derive(Deserialize)]
struct Mention {
    #[serde(rename = "mergeRequest")]
    merge_request: Option<MergeRequestDescription>,
    #[serde(rename = "workItem")]
    work_item: Option<WorkItemReference>,
}

#[derive(Deserialize)]
struct MergeRequestDescription {
    iid: String,
    description: Option<String>,
    project: Project,
}

#[derive(Deserialize)]
struct Project {
    #[serde(rename = "fullPath")]
    full_path: String,
}

#[derive(Deserialize)]
struct WorkItemReference {
    reference: String,
}

const MENTION_QUERY: &str = r"
    query mrMention($id: MergeRequestID!, $target: WorkItemID!) {
        mergeRequest(id: $id) {
            iid
            description
            project { fullPath }
        }
        workItem(id: $target) {
            reference(full: true)
        }
    }
";

const ISSUE_RELATED_QUERY: &str = r"
    query issueRelated($id: WorkItemID!) {
        workItem(id: $id) {
            widgets(onlyTypes: [LINKED_ITEMS, DEVELOPMENT]) {
                ... on WorkItemWidgetLinkedItems {
                    linkedItems(first: 100) {
                        nodes {
                            linkType
                            workItem { id reference(full: true) title state webUrl }
                        }
                    }
                }
                ... on WorkItemWidgetDevelopment {
                    closingMergeRequests(first: 100) {
                        nodes {
                            mergeRequest { id reference(full: true) title state webUrl }
                        }
                    }
                    relatedMergeRequests(first: 100) {
                        nodes { id reference(full: true) title state webUrl }
                    }
                }
            }
        }
    }
";

const MR_RELATED_QUERY: &str = r"
    query mrRelated($id: MergeRequestID!) {
        mergeRequest(id: $id) {
            linkedWorkItems {
                linkType
                workItem { id reference(full: true) title state webUrl }
            }
        }
    }
";

#[derive(Deserialize)]
struct MrLinkedItem {
    #[serde(rename = "linkType")]
    link_type: MrLinkType,
    #[serde(rename = "workItem")]
    work_item: Option<RelatedFields>,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum MrLinkType {
    Closes,
    Mentioned,
}

impl MrLinkedItem {
    /// An issue on an external tracker arrives with `workItem` null.
    fn into_related(self) -> Option<RelatedItem> {
        let relation = match self.link_type {
            MrLinkType::Closes => Relation::Closes,
            MrLinkType::Mentioned => Relation::RelatesTo,
        };
        self.work_item?.into_related(relation)
    }
}

const LINK_MUTATION: &str = r"
    mutation workItemAddLinkedItems($id: WorkItemID!, $targets: [WorkItemID!]!, $linkType: WorkItemRelatedLinkType!) {
        workItemAddLinkedItems(input: { id: $id, workItemsIds: $targets, linkType: $linkType }) {
            errors
        }
    }
";

const UNLINK_MUTATION: &str = r"
    mutation workItemRemoveLinkedItems($id: WorkItemID!, $targets: [WorkItemID!]!) {
        workItemRemoveLinkedItems(input: { id: $id, workItemsIds: $targets }) {
            errors
        }
    }
";

#[derive(Deserialize)]
struct RelationWidget {
    #[serde(rename = "linkedItems")]
    linked_items: Option<Nodes<LinkedItem>>,
    #[serde(rename = "closingMergeRequests")]
    closing_merge_requests: Option<Nodes<ClosingMergeRequest>>,
    #[serde(rename = "relatedMergeRequests")]
    related_merge_requests: Option<Nodes<RelatedFields>>,
}

#[derive(Deserialize)]
struct LinkedItem {
    #[serde(rename = "linkType")]
    link_type: Relation,
    #[serde(rename = "workItem")]
    work_item: Option<RelatedFields>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelatedFields {
    #[serde(rename = "id")]
    gid: String,
    reference: String,
    title: String,
    state: String,
    web_url: String,
}

impl RelatedFields {
    fn into_related(self, relation: Relation) -> Option<RelatedItem> {
        Some(RelatedItem {
            relation,
            gid: self.gid,
            item: ItemRef::parse(&self.reference)?,
            title: self.title,
            state: normalize_state(&self.state),
            web_url: self.web_url,
        })
    }
}

#[derive(Deserialize)]
struct ClosingMergeRequest {
    #[serde(rename = "mergeRequest")]
    merge_request: Option<RelatedFields>,
}

impl LinkedItem {
    fn into_related(self) -> Option<RelatedItem> {
        let link_type = self.link_type;
        self.work_item?.into_related(link_type)
    }
}

impl RelationWidget {
    fn into_related(self) -> Vec<RelatedItem> {
        if let Some(items) = self.linked_items {
            return items
                .nodes
                .into_iter()
                .filter_map(LinkedItem::into_related)
                .collect();
        }
        let closing = self.closing_merge_requests.map_or_default(|mrs| {
            mrs.nodes
                .into_iter()
                .filter_map(|node| node.merge_request?.into_related(Relation::ClosedBy))
                .collect()
        });
        let mentioning = self.related_merge_requests.map_or_default(|mrs| {
            mrs.nodes
                .into_iter()
                .filter_map(|node| node.into_related(Relation::RelatesTo))
                .collect()
        });
        fold(closing, mentioning)
    }
}

#[cfg(test)]
mod tests {

    use super::{MrLinkedItem, RelationWidget};
    use glab_core::domain::{RelatedItem, Relation};

    #[test]
    fn an_issues_links_and_merge_requests_fold_into_one_list() {
        let widgets: Vec<RelationWidget> = serde_json::from_str(
            r#"[
                {
                  "closingMergeRequests": { "nodes": [
                    { "mergeRequest": {
                        "id": "gid://gitlab/MergeRequest/5",
                        "reference": "team/infra!5",
                        "title": "Fix the fetch loop",
                        "state": "merged",
                        "webUrl": "https://gitlab.example.com/team/infra/-/merge_requests/5" } } ] },
                  "relatedMergeRequests": { "nodes": [
                    { "id": "gid://gitlab/MergeRequest/5",
                      "reference": "team/infra!5",
                      "title": "Fix the fetch loop",
                      "state": "merged",
                      "webUrl": "https://gitlab.example.com/team/infra/-/merge_requests/5" },
                    { "id": "gid://gitlab/MergeRequest/9",
                      "reference": "other/team!9",
                      "title": "Mention the fetch loop",
                      "state": "opened",
                      "webUrl": "https://gitlab.example.com/other/team/-/merge_requests/9" } ] }
                },
                {
                  "linkedItems": { "nodes": [
                    { "linkType": "is_blocked_by",
                      "workItem": {
                        "id": "gid://gitlab/WorkItem/43954",
                        "reference": "team/infra#42",
                        "title": "Rework the fetch loop",
                        "state": "OPEN",
                        "webUrl": "https://gitlab.example.com/team/infra/-/work_items/42" } } ] }
                }
            ]"#,
        )
        .expect("the documented shape deserializes");

        let mut related: Vec<RelatedItem> = widgets
            .into_iter()
            .flat_map(RelationWidget::into_related)
            .collect();
        related.sort_by_key(RelatedItem::rank);
        let related: Vec<(Relation, String, String, String)> = related
            .into_iter()
            .map(|r| (r.relation, r.item.reference(), r.gid, r.state))
            .collect();

        assert_eq!(
            related,
            vec![
                (
                    Relation::BlockedBy,
                    "team/infra#42".to_string(),
                    "gid://gitlab/WorkItem/43954".to_string(),
                    "opened".to_string(),
                ),
                (
                    Relation::ClosedBy,
                    "team/infra!5".to_string(),
                    "gid://gitlab/MergeRequest/5".to_string(),
                    "merged".to_string(),
                ),
                (
                    Relation::RelatesTo,
                    "other/team!9".to_string(),
                    "gid://gitlab/MergeRequest/9".to_string(),
                    "opened".to_string(),
                ),
            ],
            "a stored link keeps the gid an unlink names, a closing merge \
             request outranks the same one merely related, and OPEN is \
             normalized to the spelling the rest of glab-dash uses"
        );
    }

    #[test]
    fn a_merge_requests_issues_come_with_their_reference_and_skip_external_ones() {
        let linked: Vec<MrLinkedItem> = serde_json::from_str(
            r#"[
                { "linkType": "CLOSES",
                  "externalIssue": { "reference": "UIUX-1" },
                  "workItem": null },
                { "linkType": "CLOSES",
                  "workItem": {
                    "id": "gid://gitlab/WorkItem/1",
                    "reference": "team/infra#1",
                    "title": "Add comments for code",
                    "state": "CLOSED",
                    "webUrl": "https://gitlab.example.com/team/infra/-/issues/1" } },
                { "linkType": "MENTIONED",
                  "workItem": {
                    "id": "gid://gitlab/WorkItem/9",
                    "reference": "other/team#9",
                    "title": "Mentioned only",
                    "state": "OPEN",
                    "webUrl": "https://gitlab.example.com/other/team/-/issues/9" } }
            ]"#,
        )
        .expect("the documented shape deserializes");

        let linked: Vec<(Relation, String, String)> = linked
            .into_iter()
            .filter_map(MrLinkedItem::into_related)
            .map(|r| (r.relation, r.item.reference(), r.state))
            .collect();
        assert_eq!(
            linked,
            vec![
                (
                    Relation::Closes,
                    "team/infra#1".to_string(),
                    "closed".to_string()
                ),
                (
                    Relation::RelatesTo,
                    "other/team#9".to_string(),
                    "opened".to_string()
                ),
            ]
        );
    }

    #[test]
    fn only_a_closing_relation_gets_the_keyword_gitlab_acts_on() {
        assert_eq!(
            super::mention_line("team/infra#42", Relation::Closes),
            "Closes team/infra#42"
        );
        assert_eq!(
            super::mention_line("team/infra#42", Relation::RelatesTo),
            "Related to team/infra#42"
        );
    }
}
