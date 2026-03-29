pub mod content;
pub mod error;
pub mod history;
pub mod paths;
pub mod revision;

pub use content::RevisionContent;
pub use error::{DomainError, DomainResult};
pub use history::HistorySession;
pub use paths::{AbsoluteFilePath, RepoRelativePath, RepositoryRoot};
pub use revision::{
    AuthorEmail, AuthorName, AuthorTime, CommitMessage, CommitTime, Revision, RevisionId,
    RevisionIndex, RevisionSummary, ShortRevisionId,
};
