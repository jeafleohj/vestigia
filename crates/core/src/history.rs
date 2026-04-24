use std::collections::HashMap;

use crate::{
    content::RevisionContent,
    error::{DomainError, DomainResult},
    paths::{AbsoluteFilePath, RepoRelativePath, RepositoryRoot},
    revision::{Revision, RevisionId, RevisionIndex},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySession {
    target_file: AbsoluteFilePath,
    repo_root: RepositoryRoot,
    repo_relative_path: RepoRelativePath,
    revisions: Vec<Revision>,
    current_index: RevisionIndex,
    content_cache: HashMap<RevisionId, RevisionContent>,
}

impl HistorySession {
    pub fn new(
        target_file: AbsoluteFilePath,
        repo_root: RepositoryRoot,
        repo_relative_path: RepoRelativePath,
        revisions: Vec<Revision>,
    ) -> DomainResult<Self> {
        if revisions.is_empty() {
            return Err(DomainError::EmptyHistory);
        }

        Ok(Self {
            target_file,
            repo_root,
            repo_relative_path,
            revisions,
            current_index: RevisionIndex::new(0),
            content_cache: HashMap::new(),
        })
    }

    pub fn target_file(&self) -> &AbsoluteFilePath {
        &self.target_file
    }

    pub fn repo_root(&self) -> &RepositoryRoot {
        &self.repo_root
    }

    pub fn repo_relative_path(&self) -> &RepoRelativePath {
        &self.repo_relative_path
    }

    pub fn revisions(&self) -> &[Revision] {
        &self.revisions
    }

    pub fn current_index(&self) -> RevisionIndex {
        self.current_index
    }

    pub fn current_revision(&self) -> &Revision {
        &self.revisions[self.current_index.get()]
    }

    pub fn current_content(&self) -> Option<&RevisionContent> {
        self.content_for_revision_id(&self.current_revision().id)
    }

    pub fn len(&self) -> usize {
        self.revisions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.revisions.is_empty()
    }

    pub fn content_for_revision_id(&self, revision_id: &RevisionId) -> Option<&RevisionContent> {
        self.content_cache.get(revision_id)
    }

    pub fn cache_content(&mut self, content: RevisionContent) -> &RevisionContent {
        let revision_id = content.revision_id().clone();
        self.content_cache.entry(revision_id).or_insert(content)
    }

    pub fn jump_to(&mut self, index: RevisionIndex) -> DomainResult<&Revision> {
        if index.get() >= self.revisions.len() {
            return Err(DomainError::InvalidRevisionIndex {
                index: index.get(),
                len: self.revisions.len(),
            });
        }

        self.current_index = index;

        Ok(self.current_revision())
    }

    pub fn move_older(&mut self) -> Option<&Revision> {
        let next = self.current_index.get() + 1;

        if next >= self.revisions.len() {
            return None;
        }

        self.current_index = RevisionIndex::new(next);

        Some(self.current_revision())
    }

    pub fn move_newer(&mut self) -> Option<&Revision> {
        let current = self.current_index.get();

        if current == 0 {
            return None;
        }

        self.current_index = RevisionIndex::new(current - 1);

        Some(self.current_revision())
    }
}

#[cfg(test)]
mod tests {
    use super::HistorySession;
    use crate::{
        paths::{AbsoluteFilePath, RepoRelativePath, RepositoryRoot},
        revision::{
            AuthorName, AuthorTime, CommitMessage, CommitTime, Revision, RevisionId, RevisionIndex,
            RevisionSummary, ShortRevisionId,
        },
    };

    fn sample_revision(index: usize) -> Revision {
        Revision::new(
            RevisionId::new(format!("commit-{index}")).unwrap(),
            ShortRevisionId::new(format!("c{index}")).unwrap(),
            AuthorName::new("Jhony").unwrap(),
            None,
            AuthorTime::new(0, 0),
            CommitTime::new(0, 0),
            RevisionSummary::new(format!("Summary {index}")).unwrap(),
            CommitMessage::new(format!("Message {index}")).unwrap(),
            RevisionIndex::new(index),
        )
        .unwrap()
    }

    #[test]
    fn starts_at_most_recent_revision() {
        let session = HistorySession::new(
            AbsoluteFilePath::new("/tmp/file.rs").unwrap(),
            RepositoryRoot::new("/tmp").unwrap(),
            RepoRelativePath::new("file.rs").unwrap(),
            vec![sample_revision(0), sample_revision(1)],
        )
        .unwrap();

        assert_eq!(session.current_index().get(), 0);
        assert_eq!(session.current_revision().summary.as_str(), "Summary 0");
    }

    #[test]
    fn move_older_advances_until_the_end() {
        let mut session = HistorySession::new(
            AbsoluteFilePath::new("/tmp/file.rs").unwrap(),
            RepositoryRoot::new("/tmp").unwrap(),
            RepoRelativePath::new("file.rs").unwrap(),
            vec![sample_revision(0), sample_revision(1)],
        )
        .unwrap();

        assert_eq!(session.move_older().unwrap().summary.as_str(), "Summary 1");
        assert!(session.move_older().is_none());
    }

    #[test]
    fn jump_to_rejects_out_of_bounds_indexes() {
        let mut session = HistorySession::new(
            AbsoluteFilePath::new("/tmp/file.rs").unwrap(),
            RepositoryRoot::new("/tmp").unwrap(),
            RepoRelativePath::new("file.rs").unwrap(),
            vec![sample_revision(0)],
        )
        .unwrap();

        assert!(session.jump_to(RevisionIndex::new(1)).is_err());
    }
}
