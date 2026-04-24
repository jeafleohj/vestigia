use crate::revision::RevisionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevisionContent {
    Deleted {
        revision_id: RevisionId,
    },
    Unavailable {
        revision_id: RevisionId,
        message: String,
    },
    Text {
        revision_id: RevisionId,
        content: String,
        encoding: Option<String>,
    },
    Binary {
        revision_id: RevisionId,
    },
    UnsupportedEncoding {
        revision_id: RevisionId,
        encoding: Option<String>,
    },
}

impl RevisionContent {
    pub fn revision_id(&self) -> &RevisionId {
        match self {
            Self::Deleted { revision_id }
            | Self::Unavailable { revision_id, .. }
            | Self::Text { revision_id, .. }
            | Self::Binary { revision_id }
            | Self::UnsupportedEncoding { revision_id, .. } => revision_id,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { content, .. } => Some(content.as_str()),
            Self::Deleted { .. }
            | Self::Unavailable { .. }
            | Self::Binary { .. }
            | Self::UnsupportedEncoding { .. } => None,
        }
    }
}
