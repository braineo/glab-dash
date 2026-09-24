//! What an item is related to, over REST.
//!
//! GitLab keeps each kind of relation on its own sub-collection — `links`,
//! `related_merge_requests`, `closed_by`, `closes_issues` — and reports each in
//! its own shape.  `list_related` walks the ones the item's kind has and folds
//! them into one list, so a new collection is one more leg here.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use glab_core::domain::{ItemKind, ItemRef, RelatedItem, Relation};

use crate::client::{GitLabClient, item_path};
use urlencoding::encode;

impl GitLabClient {
    /// Everything `item` is related to, unsorted.
    pub async fn list_related(&self, item: &ItemRef) -> Result<Vec<RelatedItem>> {
        match item.kind {
            ItemKind::Issue => {
                let (mut links, mrs) =
                    tokio::try_join!(self.issue_links(item), self.issue_merge_requests(item))?;
                links.extend(mrs);
                Ok(links)
            }
            ItemKind::MergeRequest => self.mr_issues(item).await,
        }
    }

    /// [`Relation::Blocks`] means `item` blocks `target`.  GitLab refuses a
    /// `relation` outside [`Relation::LINKABLE`], which it derives itself.
    pub async fn add_link(
        &self,
        item: &ItemRef,
        target: &ItemRef,
        relation: Relation,
    ) -> Result<()> {
        let request = self
            .rest(Method::POST, &links_path(item, ""))
            .json(&serde_json::json!({
                "target_project_id": target.project,
                "target_issue_iid": target.iid,
                "link_type": relation,
            }));
        Self::send::<Value>(request).await.map(|_| ())
    }

    /// Record `target` as related to — or closed by — the merge request `mr`,
    /// which GitLab reads from the description rather than storing as a link.
    /// Read-modify-write: there is no append, and a line already present is
    /// left alone rather than repeated.
    pub async fn mention_in_mr(
        &self,
        mr: &ItemRef,
        target: &ItemRef,
        relation: Relation,
    ) -> Result<()> {
        let path = format!(
            "/projects/{}/merge_requests/{}",
            encode(&mr.project),
            mr.iid
        );
        let line = mention_line(target, relation);
        let current: WireDescription = Self::fetch(self.rest(Method::GET, &path)).await?;
        let description = current.description.unwrap_or_default();
        if description.contains(&line) {
            return Ok(());
        }
        let description = if description.trim().is_empty() {
            line
        } else {
            format!("{}\n\n{line}", description.trim_end())
        };
        let request = self
            .rest(Method::PUT, &path)
            .json(&serde_json::json!({ "description": description }));
        Self::send::<Value>(request).await.map(|_| ())
    }

    /// `link_id` is one of the ids `list_related` reported.
    pub async fn remove_link(&self, item: &ItemRef, link_id: u64) -> Result<()> {
        let request = self.rest(Method::DELETE, &links_path(item, &link_id.to_string()));
        Self::send::<Value>(request).await.map(|_| ())
    }

    /// An issue's stored links.
    async fn issue_links(&self, item: &ItemRef) -> Result<Vec<RelatedItem>> {
        let links: Vec<WireLink> =
            Self::fetch(self.rest(Method::GET, &links_path(item, ""))).await?;
        Ok(links
            .into_iter()
            .filter_map(WireLink::into_related)
            .collect())
    }

    /// The merge requests an issue names.  `closed_by` is the subset that will
    /// close it, which is the relation worth showing, so it classifies first
    /// and the wider collection fills in what it left.
    async fn issue_merge_requests(&self, item: &ItemRef) -> Result<Vec<RelatedItem>> {
        let path = |tail: &str| item_path(item.kind, &item.project, &item.iid, tail);
        let (closing, mentioning): (Vec<WireItem>, Vec<WireItem>) = tokio::try_join!(
            Self::fetch(self.rest(Method::GET, &path("closed_by"))),
            Self::fetch(self.rest(Method::GET, &path("related_merge_requests"))),
        )?;
        Ok(fold(
            related(closing, Relation::ClosedBy),
            related(mentioning, Relation::RelatesTo),
        ))
    }

    /// The issues a merge request names.  `closes_issues` is the subset it will
    /// close, and the mirror of an issue's `closed_by`.  Neither row carries a
    /// reference, only a numeric project id, so the ids resolve to paths first.
    async fn mr_issues(&self, item: &ItemRef) -> Result<Vec<RelatedItem>> {
        let path = |tail: &str| item_path(item.kind, &item.project, &item.iid, tail);
        let (closing, mentioning): (Vec<WireIssue>, Vec<WireIssue>) = tokio::try_join!(
            Self::fetch(self.rest(Method::GET, &path("closes_issues"))),
            Self::fetch(self.rest(Method::GET, &path("related_issues"))),
        )?;
        let paths = self
            .project_paths(closing.iter().chain(&mentioning))
            .await?;
        Ok(fold(
            related_issues(closing, Relation::Closes, &paths),
            related_issues(mentioning, Relation::RelatesTo, &paths),
        ))
    }

    /// `path_with_namespace` for every project the rows name — one lookup per
    /// distinct id, which is almost always the merge request's own project.
    ///
    /// ponytail: not cached across calls; give the client a map if a detail
    /// view that reopens often makes the extra read show.
    async fn project_paths<'a>(
        &self,
        rows: impl Iterator<Item = &'a WireIssue>,
    ) -> Result<HashMap<u64, String>> {
        let ids: HashSet<u64> = rows.map(|row| row.project_id).collect();
        let mut paths = HashMap::with_capacity(ids.len());
        for id in ids {
            let project: WireProject =
                Self::fetch(self.rest(Method::GET, &format!("/projects/{id}"))).await?;
            paths.insert(id, project.path_with_namespace);
        }
        Ok(paths)
    }
}

/// Every row that names an item, under one relation.
fn related(rows: Vec<WireItem>, relation: Relation) -> Vec<RelatedItem> {
    rows.into_iter()
        .filter_map(|row| row.into_related(relation))
        .collect()
}

/// The same, for rows that name their project by id.  A row whose project did
/// not resolve is dropped: nothing can address it.
fn related_issues(
    rows: Vec<WireIssue>,
    relation: Relation,
    paths: &HashMap<u64, String>,
) -> Vec<RelatedItem> {
    rows.into_iter()
        .filter_map(|row| row.into_related(relation, paths))
        .collect()
}

/// `closing` wins the overlap: an item that settles the other also shows up in
/// the wider collection as merely related.
fn fold(closing: Vec<RelatedItem>, mentioning: Vec<RelatedItem>) -> Vec<RelatedItem> {
    let closing_items: HashSet<&ItemRef> = closing.iter().map(|r| &r.item).collect();
    let kept: Vec<RelatedItem> = mentioning
        .into_iter()
        .filter(|r| !closing_items.contains(&r.item))
        .collect();
    let mut related = closing;
    related.extend(kept);
    related
}

/// The line a merge request's description carries to name `target`.  GitLab
/// acts on `Closes`; every other relation is the mention itself, so the words
/// around it are for the reader.
fn mention_line(target: &ItemRef, relation: Relation) -> String {
    let keyword = match relation {
        Relation::Closes => "Closes",
        _ => "Related to",
    };
    format!("{keyword} {}", target.reference())
}

/// What `mention_in_mr` reads back before it writes.
#[derive(Deserialize)]
struct WireDescription {
    description: Option<String>,
}

/// The route for an issue's links, or the one link `tail` names.
fn links_path(item: &ItemRef, tail: &str) -> String {
    item_path(
        item.kind,
        &item.project,
        &item.iid,
        &format!("links/{tail}"),
    )
}

/// One row of `GET /issues/:iid/links`.
#[derive(Deserialize)]
struct WireLink {
    issue_link_id: u64,
    link_type: Relation,
    title: String,
    state: String,
    web_url: String,
    references: WireReferences,
}

/// One row of an issue's `closed_by` or `related_merge_requests`.  GitLab
/// stores neither as a link, so neither has an id to delete.
#[derive(Deserialize)]
struct WireItem {
    title: String,
    state: String,
    web_url: String,
    references: WireReferences,
}

impl WireItem {
    fn into_related(self, relation: Relation) -> Option<RelatedItem> {
        Some(RelatedItem {
            relation,
            link_id: None,
            item: ItemRef::parse(&self.references.full)?,
            title: self.title,
            state: self.state,
            web_url: self.web_url,
        })
    }
}

/// One row of a merge request's `closes_issues` or `related_issues`.  These two
/// report a numeric `project_id` and no `references`, unlike every other
/// collection here.
#[derive(Deserialize)]
struct WireIssue {
    iid: u64,
    project_id: u64,
    title: String,
    state: String,
    web_url: String,
}

impl WireIssue {
    fn into_related(self, relation: Relation, paths: &HashMap<u64, String>) -> Option<RelatedItem> {
        Some(RelatedItem {
            relation,
            link_id: None,
            item: ItemRef::issue(paths.get(&self.project_id)?, &self.iid.to_string()),
            title: self.title,
            state: self.state,
            web_url: self.web_url,
        })
    }
}

/// The one field of `GET /projects/:id` that names the project.
#[derive(Deserialize)]
struct WireProject {
    path_with_namespace: String,
}

#[derive(Deserialize)]
struct WireReferences {
    full: String,
}

impl WireLink {
    /// `None` for a row whose reference will not parse: nothing to address.
    fn into_related(self) -> Option<RelatedItem> {
        Some(RelatedItem {
            relation: self.link_type,
            link_id: Some(self.issue_link_id),
            item: ItemRef::parse(&self.references.full)?,
            title: self.title,
            state: self.state,
            web_url: self.web_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{WireIssue, WireItem, WireLink, fold, related, related_issues};
    use glab_core::domain::{ItemKind, ItemRef, Relation};

    #[test]
    fn a_closing_merge_request_outranks_the_same_one_merely_related() {
        let mr = |iid: u32, state: &str| -> WireItem {
            serde_json::from_str(&format!(
                r#"{{
                    "title": "Fix the fetch loop",
                    "state": "{state}",
                    "web_url": "https://gitlab.example.com/team/infra/-/merge_requests/{iid}",
                    "references": {{ "full": "team/infra!{iid}" }}
                }}"#
            ))
            .expect("the documented shape deserializes")
        };

        let folded = fold(
            related(vec![mr(5, "merged")], Relation::ClosedBy),
            related(vec![mr(5, "merged"), mr(9, "opened")], Relation::RelatesTo),
        );
        let rows: Vec<(Relation, &str, ItemKind)> = folded
            .iter()
            .map(|r| (r.relation, r.item.iid.as_str(), r.item.kind))
            .collect();
        assert_eq!(
            rows,
            vec![
                (Relation::ClosedBy, "5", ItemKind::MergeRequest),
                (Relation::RelatesTo, "9", ItemKind::MergeRequest),
            ]
        );
        assert!(
            folded.iter().all(|r| r.link_id.is_none()),
            "neither collection is a stored link"
        );
    }

    /// The merge request side names its project by id, so an id the lookup did
    /// not resolve leaves nothing to address.
    #[test]
    fn a_merge_requests_issues_take_their_project_from_the_resolved_id() {
        let issue = |iid: u32, project_id: u64| -> WireIssue {
            serde_json::from_str(&format!(
                r#"{{
                    "iid": {iid},
                    "project_id": {project_id},
                    "title": "The fetch loop stalls",
                    "state": "opened",
                    "web_url": "https://gitlab.example.com/team/infra/-/issues/{iid}"
                }}"#
            ))
            .expect("the documented shape deserializes")
        };
        let paths: HashMap<u64, String> = [(7, "team/infra".to_string())].into_iter().collect();

        let folded = fold(
            related_issues(vec![issue(5, 7)], Relation::Closes, &paths),
            related_issues(
                vec![issue(5, 7), issue(9, 7), issue(11, 8)],
                Relation::RelatesTo,
                &paths,
            ),
        );
        let rows: Vec<(Relation, String, ItemKind)> = folded
            .iter()
            .map(|r| (r.relation, r.item.reference(), r.item.kind))
            .collect();
        assert_eq!(
            rows,
            vec![
                (
                    Relation::Closes,
                    "team/infra#5".to_string(),
                    ItemKind::Issue
                ),
                (
                    Relation::RelatesTo,
                    "team/infra#9".to_string(),
                    ItemKind::Issue
                ),
            ],
            "an unresolved project id is dropped"
        );
    }

    #[test]
    fn only_a_closing_relation_gets_the_keyword_gitlab_acts_on() {
        let issue = ItemRef::issue("team/infra", "42");
        assert_eq!(
            super::mention_line(&issue, Relation::Closes),
            "Closes team/infra#42"
        );
        assert_eq!(
            super::mention_line(&issue, Relation::RelatesTo),
            "Related to team/infra#42"
        );
    }

    #[test]
    fn a_link_row_folds_into_a_related_item() {
        let row: WireLink = serde_json::from_str(
            r#"{
                "issue_link_id": 77,
                "link_type": "is_blocked_by",
                "iid": 42,
                "title": "Rework the fetch loop",
                "state": "opened",
                "web_url": "https://gitlab.example.com/team/infra/-/issues/42",
                "references": { "full": "team/infra#42" }
            }"#,
        )
        .expect("the documented shape deserializes");

        let related = row.into_related().expect("a full reference resolves");
        assert_eq!(related.relation, Relation::BlockedBy);
        assert_eq!(related.link_id, Some(77));
        assert_eq!(related.item.kind, ItemKind::Issue);
        assert_eq!(related.item.project, "team/infra");
        assert_eq!(related.item.iid, "42");
        assert!(related.is_blocker());
    }
}
