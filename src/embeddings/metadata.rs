use std::fmt::Display;


pub enum Metadata {
    Kind,
    Path,
    Scope,
    Hash,
}

impl Display for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kind => write!(f, "kind"),
            Self::Path => write!(f, "path"),
            Self::Scope => write!(f, "scope"),
            Self::Hash => write!(f, "hash")
        }
    }
}