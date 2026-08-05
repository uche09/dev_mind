use std::fmt::Display;


pub enum Metadata {
    Name,
    Kind,
    Path,
    Scope,
    Hash,
}

impl Display for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name => write!(f, "name"),
            Self::Kind => write!(f, "kind"),
            Self::Path => write!(f, "path"),
            Self::Scope => write!(f, "scope"),
            Self::Hash => write!(f, "hash")
        }
    }
}