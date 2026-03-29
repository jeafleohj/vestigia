use std::fmt;

use crate::error::{DomainError, DomainResult};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RevisionId(String);

impl RevisionId {
    pub fn new(id: impl Into<String>) -> DomainResult<Self> {
        let id = id.into();

        if id.trim().is_empty() {
            return Err(DomainError::EmptyRevisionId);
        }

        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RevisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RevisionIndex(usize);

impl RevisionIndex {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShortRevisionId(String);

impl ShortRevisionId {
    pub fn new(id: impl Into<String>) -> DomainResult<Self> {
        let id = id.into();

        if id.trim().is_empty() {
            return Err(DomainError::EmptyShortRevisionId);
        }

        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ShortRevisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthorName(String);

impl AuthorName {
    pub fn new(name: impl Into<String>) -> DomainResult<Self> {
        let name = name.into();

        if name.trim().is_empty() {
            return Err(DomainError::EmptyAuthorName);
        }

        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthorEmail(String);

impl AuthorEmail {
    pub fn new(email: impl Into<String>) -> DomainResult<Self> {
        let email = email.into();

        if !email.contains('@') {
            return Err(DomainError::InvalidAuthorEmail(email));
        }

        Ok(Self(email))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AuthorTime {
    seconds: i64,
    offset_minutes: i32,
}

impl AuthorTime {
    pub const fn new(seconds: i64, offset_minutes: i32) -> Self {
        Self {
            seconds,
            offset_minutes,
        }
    }

    pub const fn seconds(self) -> i64 {
        self.seconds
    }

    pub const fn offset_minutes(self) -> i32 {
        self.offset_minutes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommitTime {
    seconds: i64,
    offset_minutes: i32,
}

impl CommitTime {
    pub const fn new(seconds: i64, offset_minutes: i32) -> Self {
        Self {
            seconds,
            offset_minutes,
        }
    }

    pub const fn seconds(self) -> i64 {
        self.seconds
    }

    pub const fn offset_minutes(self) -> i32 {
        self.offset_minutes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RevisionSummary(String);

impl RevisionSummary {
    pub fn new(summary: impl Into<String>) -> DomainResult<Self> {
        let summary = summary.into();

        if summary.trim().is_empty() {
            return Err(DomainError::EmptySummary);
        }

        Ok(Self(summary))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommitMessage(String);

impl CommitMessage {
    pub fn new(message: impl Into<String>) -> DomainResult<Self> {
        let message = message.into();

        if message.trim().is_empty() {
            return Err(DomainError::EmptyCommitMessage);
        }

        Ok(Self(message))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub id: RevisionId,
    pub short_id: ShortRevisionId,
    pub author_name: AuthorName,
    pub author_email: Option<AuthorEmail>,
    pub author_time: AuthorTime,
    pub commit_time: CommitTime,
    pub summary: RevisionSummary,
    pub message: CommitMessage,
    pub index: RevisionIndex,
}

impl Revision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: RevisionId,
        short_id: ShortRevisionId,
        author_name: AuthorName,
        author_email: Option<AuthorEmail>,
        author_time: AuthorTime,
        commit_time: CommitTime,
        summary: RevisionSummary,
        message: CommitMessage,
        index: RevisionIndex,
    ) -> DomainResult<Self> {
        Ok(Self {
            id,
            short_id,
            author_name,
            author_email,
            author_time,
            commit_time,
            summary,
            message,
            index,
        })
    }

    pub fn with_index(mut self, index: RevisionIndex) -> DomainResult<Self> {
        self.index = index;
        Ok(self)
    }
}
