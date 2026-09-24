//! Naming an item ([`ItemRef`]) and what one item is to another
//! ([`RelatedItem`]), in every combination GitLab relates them in.

use serde::{Deserialize, Serialize};

use super::{User, project_from_reference};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Issue,
    MergeRequest,
}

impl ItemKind {
    /// `#` on an issue, `!` on a merge request.
    pub fn sigil(self) -> char {
        match self {
            ItemKind::Issue => '#',
            ItemKind::MergeRequest => '!',
        }
    }
}

/// An issue or merge request by identity alone, so two refs to the same item
/// compare equal however they were built — which is what lets this key a map.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemRef {
    pub kind: ItemKind,
    pub project: String,
    pub iid: String,
}

impl ItemRef {
    pub fn issue(project: &str, iid: &str) -> Self {
        Self {
            kind: ItemKind::Issue,
            project: project.to_string(),
            iid: iid.to_string(),
        }
    }

    pub fn merge_request(project: &str, iid: &str) -> Self {
        Self {
            kind: ItemKind::MergeRequest,
            project: project.to_string(),
            iid: iid.to_string(),
        }
    }

    /// The full reference GitLab prints, `group/project#123`.
    pub fn reference(&self) -> String {
        format!("{}{}{}", self.project, self.kind.sigil(), self.iid)
    }

    /// `None` for a reference carrying no sigil, which names no item.
    pub fn parse(reference: &str) -> Option<Self> {
        let (project, iid, kind) = if let Some((project, iid)) = reference.rsplit_once('#') {
            (project, iid, ItemKind::Issue)
        } else {
            let (project, iid) = reference.rsplit_once('!')?;
            (project, iid, ItemKind::MergeRequest)
        };
        (!project.is_empty() && !iid.is_empty()).then(|| Self {
            kind,
            project: project.to_string(),
            iid: iid.to_string(),
        })
    }
}

/// What an issue and a merge request answer to in common.
pub trait Item {
    fn kind(&self) -> ItemKind;
    /// The id a GraphQL mutation addresses.  Not an identity — the same item
    /// arrives under more than one gid depending on the query, so look up by
    /// [`ItemRef`].
    fn gid(&self) -> &str;
    fn iid(&self) -> &str;
    /// The full reference GitLab prints, `group/project#123`.
    fn reference(&self) -> &str;
    fn title(&self) -> &str;
    fn state(&self) -> &str;
    /// Nullable on a merge request.
    fn web_url(&self) -> Option<&str>;
    fn labels(&self) -> &[String];
    fn assignees(&self) -> &[User];

    /// e.g. `group/project`.
    fn project_path(&self) -> &str {
        project_from_reference(self.reference())
    }

    fn item_ref(&self) -> ItemRef {
        ItemRef {
            kind: self.kind(),
            project: self.project_path().to_string(),
            iid: self.iid().to_string(),
        }
    }

    fn is_open(&self) -> bool {
        self.state() == STATE_OPENED
    }
}

pub(super) const STATE_OPENED: &str = "opened";

/// How one item stands to another, named from the holder's side:
/// [`Relation::Blocks`] means the item holding this blocks the one it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    #[serde(rename = "is_blocked_by")]
    BlockedBy,
    Blocks,
    RelatesTo,
    /// Derived from the description rather than stored, so it has no link to
    /// drop.
    ClosedBy,
    Closes,
}

impl Relation {
    /// What the links endpoint accepts; GitLab derives the rest.
    pub const LINKABLE: [Relation; 3] =
        [Relation::BlockedBy, Relation::Blocks, Relation::RelatesTo];

    /// "blocked by", with the related item as the object.
    pub fn label(self) -> &'static str {
        match self {
            Relation::BlockedBy => "blocked by",
            Relation::Blocks => "blocks",
            Relation::RelatesTo => "relates to",
            Relation::ClosedBy => "closed by",
            Relation::Closes => "closes",
        }
    }

    /// Blockers first, the merely connected last.
    pub fn rank(self) -> u8 {
        match self {
            Relation::BlockedBy => 0,
            Relation::Blocks => 1,
            Relation::ClosedBy => 2,
            Relation::Closes => 3,
            Relation::RelatesTo => 4,
        }
    }
}

/// A relation and enough of the item it names to show a row without fetching
/// it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedItem {
    pub relation: Relation,
    /// The link's own id, which is what a delete addresses.  `None` when
    /// GitLab derives the relation, leaving nothing to delete.
    pub link_id: Option<u64>,
    pub item: ItemRef,
    pub title: String,
    pub state: String,
    pub web_url: String,
}

impl RelatedItem {
    pub fn is_open(&self) -> bool {
        self.state == STATE_OPENED
    }

    /// An open blocker: the one relation that holds the item up.
    pub fn is_blocker(&self) -> bool {
        self.relation == Relation::BlockedBy && self.is_open()
    }

    /// By relation, and within one, open before settled.
    pub fn rank(&self) -> (u8, bool) {
        (self.relation.rank(), !self.is_open())
    }
}

#[cfg(test)]
mod tests {
    use super::{ItemKind, ItemRef, RelatedItem, Relation};

    #[test]
    fn a_reference_round_trips_through_a_ref() {
        for (reference, kind) in [
            ("group/project#123", ItemKind::Issue),
            ("group/sub/project!45", ItemKind::MergeRequest),
        ] {
            let parsed = ItemRef::parse(reference).expect("a full reference parses");
            assert_eq!(parsed.kind, kind);
            assert_eq!(parsed.reference(), reference);
        }
        assert_eq!(ItemRef::parse("group/project"), None);
        assert_eq!(ItemRef::parse("#123"), None);
    }

    #[test]
    fn blockers_sort_above_everything_and_open_above_settled() {
        let related = |relation, state: &str| RelatedItem {
            relation,
            link_id: Some(1),
            item: ItemRef::issue("g/p", "1"),
            title: String::new(),
            state: state.to_string(),
            web_url: String::new(),
        };
        let mut items = [
            related(Relation::RelatesTo, "opened"),
            related(Relation::BlockedBy, "closed"),
            related(Relation::BlockedBy, "opened"),
            related(Relation::Blocks, "opened"),
        ];
        items.sort_by_key(RelatedItem::rank);

        let order: Vec<(Relation, bool)> =
            items.iter().map(|r| (r.relation, r.is_open())).collect();
        assert_eq!(
            order,
            vec![
                (Relation::BlockedBy, true),
                (Relation::BlockedBy, false),
                (Relation::Blocks, true),
                (Relation::RelatesTo, true),
            ]
        );
        assert!(items[0].is_blocker());
        assert!(!items[1].is_blocker(), "a closed blocker holds nothing up");
    }
}
