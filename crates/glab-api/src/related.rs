//! What an item is related to, over REST.
//!
//! GitLab keeps each kind of relation on its own sub-collection — `links`,
//! `related_merge_requests`, `closes_issues` — and reports each in its own
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
            ItemKind::Issue => self.issue_links(item).await,
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
    use super::WireLink;
    use glab_core::domain::{ItemKind, Relation};

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
