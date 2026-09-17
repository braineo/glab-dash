//! What an item is related to, over REST.
//!
//! GitLab keeps each kind of relation on its own sub-collection — `links`,
//! `related_merge_requests`, `closed_by` — and reports each in its own
//! shape.  `list_related` walks the ones the item's kind has and folds them
//! into one list, so a new collection is one more leg here.

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use glab_core::domain::{ItemKind, ItemRef, RelatedItem, Relation};

use crate::client::{GitLabClient, item_path};

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
            // ponytail: a merge request's collections are a leg each, like
            // `issue_links`, when that view grows the section.
            ItemKind::MergeRequest => Ok(Vec::new()),
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
        let (closing, mentioning): (Vec<WireMr>, Vec<WireMr>) = tokio::try_join!(
            Self::fetch(self.rest(Method::GET, &path("closed_by"))),
            Self::fetch(self.rest(Method::GET, &path("related_merge_requests"))),
        )?;
        Ok(fold_merge_requests(closing, mentioning))
    }
}

/// `closing` classifies first and wins the overlap: a merge request that will
/// close the issue also shows up as merely related.
fn fold_merge_requests(closing: Vec<WireMr>, mentioning: Vec<WireMr>) -> Vec<RelatedItem> {
    let mut related: Vec<RelatedItem> = closing
        .into_iter()
        .filter_map(|mr| mr.into_related(Relation::ClosedBy))
        .collect();
    let closing_items: std::collections::HashSet<ItemRef> =
        related.iter().map(|r| r.item.clone()).collect();
    related.extend(
        mentioning
            .into_iter()
            .filter_map(|mr| mr.into_related(Relation::RelatesTo))
            .filter(|r| !closing_items.contains(&r.item)),
    );
    related
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

/// One row of `GET /issues/:iid/closed_by` or `/related_merge_requests`.
/// GitLab stores neither as a link, so neither has an id to delete.
#[derive(Deserialize)]
struct WireMr {
    title: String,
    state: String,
    web_url: String,
    references: WireReferences,
}

impl WireMr {
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
    use super::{WireLink, WireMr, fold_merge_requests};
    use glab_core::domain::{ItemKind, Relation};

    #[test]
    fn a_closing_merge_request_outranks_the_same_one_merely_related() {
        let mr = |iid: u32, state: &str| -> WireMr {
            serde_json::from_str(&format!(
                r#"{{
                    "iid": {iid},
                    "title": "Fix the fetch loop",
                    "state": "{state}",
                    "web_url": "https://gitlab.example.com/team/infra/-/merge_requests/{iid}",
                    "references": {{ "full": "team/infra!{iid}" }}
                }}"#
            ))
            .expect("the documented shape deserializes")
        };

        let folded = fold_merge_requests(
            vec![mr(5, "merged")],
            vec![mr(5, "merged"), mr(9, "opened")],
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
