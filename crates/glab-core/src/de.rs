//! The domain types are both the GraphQL wire format and the database format,
//! so each helper accepts the GraphQL form *and* the form `serde` produces
//! when serializing the type back: a connection object or a plain array.

use serde::{Deserialize, Deserializer};

/// `namespace.workItems` gives `gid://gitlab/WorkItem/42950` where the root
/// `issues` query gives `gid://gitlab/Issue/42950` for the same issue, and the
/// two result sets are deduplicated against each other.  `workItemUpdate`
/// takes the `WorkItem` form.
pub fn work_item_gid<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let raw = String::deserialize(d)?;
    Ok(match raw.rsplit_once('/') {
        Some((_, tail)) => format!("gid://gitlab/WorkItem/{tail}"),
        None => raw,
    })
}

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

/// GitLab's GraphQL enums are `SCREAMING_CASE` (`headPipeline.status` is
/// `SUCCESS`); the UI matches and sorts on the lowercase spelling.
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
