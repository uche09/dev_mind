use std::fmt::Display;

/// Allowed chunk metadata.
///
/// These metadata retains and prevent relevant context of the embedded data
/// from being lost after embedding.
///
/// They give more condext about the retrived data, rather than just vectors.
pub enum Metadata {
    Name,
    Kind,
    Path,
    Scope,
    Hash,
    RawCode,
}

impl Display for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name => write!(f, "name"),
            Self::Kind => write!(f, "kind"),
            Self::Path => write!(f, "path"),
            Self::Scope => write!(f, "scope"),
            Self::Hash => write!(f, "hash"),
            Self::RawCode => write!(f, "raw_code"),
        }
    }
}
