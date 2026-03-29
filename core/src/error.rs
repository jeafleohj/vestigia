use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    EmptyCommitMessage,
    EmptySummary,
    EmptyAuthorName,
    InvalidAuthorEmail(String),
    EmptyRevisionId,
    EmptyShortRevisionId,
    InvalidRevisionIndex {
        index: usize,
        len: usize,
    },
    EmptyHistory,
    RepositoryNotFound(PathBuf),
    PathOutsideRepository {
        path: PathBuf,
        repo_root: PathBuf,
    },
    HistoryBackendUnavailable,
    Git {
        operation: &'static str,
        message: String,
    },
    PathMustBeAbsolute(PathBuf),
    PathMustBeRelative(PathBuf),
}

pub type DomainResult<T> = Result<T, DomainError>;
