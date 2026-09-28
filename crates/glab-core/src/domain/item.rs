use serde::{Deserialize, Serialize};

use super::{User, project_from_reference};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Issue,
    MergeRequest,
}

impl ItemKind {
    pub fn sigil(self) -> char {
        match self {
            ItemKind::Issue => '#',
            ItemKind::MergeRequest => '!',
        }
    }
}

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

    pub fn reference(&self) -> String {
        format!("{}{}{}", self.project, self.kind.sigil(), self.iid)
    }

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

pub trait Item {
    fn kind(&self) -> ItemKind;
    /// Not an identity: the same item arrives under more than one gid
    /// depending on the query.  Key by [`ItemRef`] instead.
    fn gid(&self) -> &str;
    fn iid(&self) -> &str;
    /// Full: `group/project#123`, never a bare `#123`.
    fn reference(&self) -> &str;
    fn title(&self) -> &str;
    fn state(&self) -> &str;
    fn web_url(&self) -> Option<&str>;
    fn labels(&self) -> &[String];
    fn assignees(&self) -> &[User];

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

/// How one item stands to another, named from the item's own side:
/// [`Relation::Blocks`] means the item carrying this blocks the one it names.
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
    /// What `workItemAddLinkedItems` accepts; GitLab derives the rest.
    pub const LINKABLE: [Relation; 3] =
        [Relation::BlockedBy, Relation::Blocks, Relation::RelatesTo];

    pub fn label(self) -> &'static str {
        match self {
            Relation::BlockedBy => "blocked by",
            Relation::Blocks => "blocks",
            Relation::RelatesTo => "relates to",
            Relation::ClosedBy => "closed by",
            Relation::Closes => "closes",
        }
    }

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedItem {
    pub relation: Relation,
    pub gid: String,
    pub item: ItemRef,
    pub title: String,
    pub state: String,
    pub web_url: String,
}

impl RelatedItem {
    pub fn is_open(&self) -> bool {
        self.state == STATE_OPENED
    }

    pub fn is_blocker(&self) -> bool {
        self.relation == Relation::BlockedBy && self.is_open()
    }

    pub fn rank(&self) -> (u8, bool) {
        (self.relation.rank(), !self.is_open())
    }

    pub fn is_unlinkable(&self, item_kind: ItemKind) -> bool {
        item_kind == ItemKind::Issue
            && self.item.kind == ItemKind::Issue
            && Relation::LINKABLE.contains(&self.relation)
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
            gid: "gid://gitlab/WorkItem/1".to_string(),
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
