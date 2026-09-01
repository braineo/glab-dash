//! Deserializers for the shapes GitLab's GraphQL API returns.
//!
//! The domain types are both the GraphQL wire format and the format the
//! database writes, so each helper accepts the GraphQL form *and* the form
//! `serde` produces when serializing the type back: a connection object or the
//! plain array it round-trips as.

use serde::{Deserialize, Deserializer};

/// A work item's global id, normalized to the `WorkItem` prefix.
///
/// The two queries that return an issue report different prefixes for the same
/// issue — `namespace.workItems` gives `gid://gitlab/WorkItem/42950` where the
/// root `issues` query gives `gid://gitlab/Issue/42950` — and the two result
/// sets are deduplicated against each other. Normalizing on the way in makes
/// them the same string, and makes the id directly usable as the
/// `workItemUpdate` input, which wants the `WorkItem` form.
pub fn work_item_gid<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let raw = String::deserialize(d)?;
    Ok(match raw.rsplit_once('/') {
        Some((_, tail)) => format!("gid://gitlab/WorkItem/{tail}"),
        None => raw,
    })
}

/// A GraphQL connection (`{ "nodes": [...] }`) or a plain array.
pub fn nodes<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Form<T> {
        Connection { nodes: Vec<T> },
        Array(Vec<T>),
    }
    Ok(match Option::<Form<T>>::deserialize(d)? {
        Some(Form::Connection { nodes } | Form::Array(nodes)) => nodes,
        None => Vec::new(),
    })
}

/// Label titles from `labels { nodes { title } }`, or a plain array of strings.
pub fn label_titles<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    struct Titled {
        title: String,
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Form {
        Connection { nodes: Vec<Titled> },
        Titles(Vec<String>),
    }
    Ok(match Option::<Form>::deserialize(d)? {
        Some(Form::Connection { nodes }) => nodes.into_iter().map(|t| t.title).collect(),
        Some(Form::Titles(titles)) => titles,
        None => Vec::new(),
    })
}

/// An optional enum value, lowercased.
///
/// GitLab's GraphQL enums are `SCREAMING_CASE` (`headPipeline.status` is
/// `SUCCESS`), while the UI matches and sorts on the lowercase spelling the
/// REST API used (`success`). Normalizing on the way in keeps one canonical
/// form, so renderers and comparators never need to case-fold.
pub fn lower_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.map(|s| s.to_lowercase()))
}

pub fn user_id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Form {
        Gid(String),
        ID(u64),
    }

    Ok(match Option::<Form>::deserialize(d)? {
        Some(Form::Gid(gid)) => gid,
        Some(Form::ID(id)) => format!("gid://gitlab/User/{id}"),
        None => String::new(),
    })
}
