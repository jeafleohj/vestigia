use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    content::RevisionContent,
    error::{DomainError, DomainResult},
    git,
    history::HistorySession,
    paths::{AbsoluteFilePath, RepoRelativePath, RepositoryRoot},
    revision::Revision,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    repo_root: RepositoryRoot,
}

impl Engine {
    pub fn open_repository(path: impl AsRef<Path>) -> DomainResult<Self> {
        let path = path.as_ref();
        let absolute_path = absolutize(path)?;
        let start = if absolute_path.is_dir() {
            absolute_path
        } else {
            absolute_path
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| DomainError::RepositoryNotFound(path.to_path_buf()))?
        };

        let repo_root = discover_repository_root(&start)?;

        Ok(Self {
            repo_root: RepositoryRoot::new(repo_root)?,
        })
    }

    pub fn repo_root(&self) -> &RepositoryRoot {
        &self.repo_root
    }

    pub fn resolve_file_path(
        &self,
        path: impl AsRef<Path>,
    ) -> DomainResult<(AbsoluteFilePath, RepoRelativePath)> {
        let absolute_path = absolutize(path.as_ref())?;

        if !absolute_path.starts_with(self.repo_root.as_path()) {
            return Err(DomainError::PathOutsideRepository {
                path: absolute_path,
                repo_root: self.repo_root.as_path().to_path_buf(),
            });
        }

        let relative_path = absolute_path
            .strip_prefix(self.repo_root.as_path())
            .expect("validated repository prefix")
            .to_path_buf();

        Ok((
            AbsoluteFilePath::new(absolute_path)?,
            RepoRelativePath::new(relative_path)?,
        ))
    }

    pub fn open_file_history(&self, path: impl AsRef<Path>) -> DomainResult<HistorySession> {
        let (target_file, repo_relative_path) = self.resolve_file_path(path)?;
        git::open_file_history(&self.repo_root, target_file, repo_relative_path)
    }

    pub fn scan_file_history(
        &self,
        path: impl AsRef<Path>,
        on_revision: impl FnMut(Revision) -> DomainResult<()>,
    ) -> DomainResult<usize> {
        let (_, repo_relative_path) = self.resolve_file_path(path)?;
        git::scan_file_history(&self.repo_root, &repo_relative_path, on_revision)
    }

    pub fn scan_file_history_with_mode(
        &self,
        path: impl AsRef<Path>,
        mode: git::HistoryMode,
        on_revision: impl FnMut(Revision) -> DomainResult<()>,
    ) -> DomainResult<usize> {
        let (_, repo_relative_path) = self.resolve_file_path(path)?;
        git::scan_file_history_with_mode(&self.repo_root, &repo_relative_path, mode, on_revision)
    }

    pub fn load_revision_content(
        &self,
        repo_relative_path: &RepoRelativePath,
        revision_id: &crate::revision::RevisionId,
    ) -> DomainResult<RevisionContent> {
        git::load_revision_content(&self.repo_root, repo_relative_path, revision_id)
    }

    pub fn profile_file_history(
        &self,
        path: impl AsRef<Path>,
        first_batch_size: usize,
    ) -> DomainResult<git::HistoryScanProfile> {
        let (_, repo_relative_path) = self.resolve_file_path(path)?;
        git::profile_file_history(&self.repo_root, &repo_relative_path, first_batch_size)
    }

    pub fn profile_file_history_with_mode(
        &self,
        path: impl AsRef<Path>,
        mode: git::HistoryMode,
        first_batch_size: usize,
    ) -> DomainResult<git::HistoryScanProfile> {
        let (_, repo_relative_path) = self.resolve_file_path(path)?;
        git::profile_file_history_with_mode(
            &self.repo_root,
            &repo_relative_path,
            mode,
            first_batch_size,
        )
    }

    pub fn current_content<'a>(
        &self,
        session: &'a mut HistorySession,
    ) -> DomainResult<&'a RevisionContent> {
        if session.current_content().is_none() {
            let revision_id = session.current_revision().id.clone();
            let content = git::load_revision_content(
                &self.repo_root,
                session.repo_relative_path(),
                &revision_id,
            )?;

            let _ = session.cache_content(content);
        }

        Ok(session
            .current_content()
            .expect("content must be cached after loading"))
    }

    pub fn revision_content<'a>(
        &self,
        session: &'a mut HistorySession,
        revision: &Revision,
    ) -> DomainResult<&'a RevisionContent> {
        if session.content_for_revision_id(&revision.id).is_none() {
            let content = git::load_revision_content(
                &self.repo_root,
                session.repo_relative_path(),
                &revision.id,
            )?;

            let _ = session.cache_content(content);
        }

        Ok(session
            .content_for_revision_id(&revision.id)
            .expect("content must be cached after loading"))
    }
}

fn absolutize(path: &Path) -> DomainResult<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    let cwd =
        std::env::current_dir().map_err(|_| DomainError::PathMustBeAbsolute(path.to_path_buf()))?;

    Ok(cwd.join(path))
}

fn discover_repository_root(start: &Path) -> DomainResult<PathBuf> {
    for candidate in start.ancestors() {
        if is_git_repository(candidate) {
            return Ok(candidate.to_path_buf());
        }
    }

    Err(DomainError::RepositoryNotFound(start.to_path_buf()))
}

fn is_git_repository(path: &Path) -> bool {
    match fs::metadata(path.join(".git")) {
        Ok(metadata) => metadata.is_dir() || metadata.is_file(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::Engine;
    use crate::error::DomainError;

    fn test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("vestigia-{name}-{unique}"))
    }

    #[test]
    fn open_repository_discovers_root_from_nested_file() {
        let root = test_dir("repo-discovery");
        let nested = root.join("src");
        let file = nested.join("lib.rs");

        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(&nested).unwrap();
        fs::write(&file, "fn main() {}").unwrap();

        let engine = Engine::open_repository(&file).unwrap();

        assert_eq!(engine.repo_root().as_path(), root.as_path());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_file_path_returns_repo_relative_path() {
        let root = test_dir("repo-relative");
        let nested = root.join("src");
        let file = nested.join("lib.rs");

        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(&nested).unwrap();
        fs::write(&file, "fn main() {}").unwrap();

        let engine = Engine::open_repository(&file).unwrap();
        let (_, relative) = engine.resolve_file_path(&file).unwrap();

        assert_eq!(relative.as_path(), PathBuf::from("src/lib.rs").as_path());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_file_path_rejects_files_outside_repository() {
        let root = test_dir("repo-outside");
        let repo_file = root.join("src").join("lib.rs");
        let outside_root = test_dir("outside");
        let outside = outside_root.join("other.rs");

        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(repo_file.parent().unwrap()).unwrap();
        fs::write(&repo_file, "fn main() {}").unwrap();
        fs::create_dir_all(outside.parent().unwrap()).unwrap();
        fs::write(&outside, "fn other() {}").unwrap();

        let engine = Engine::open_repository(&repo_file).unwrap();
        let error = engine.resolve_file_path(&outside).unwrap_err();

        assert!(matches!(error, DomainError::PathOutsideRepository { .. }));

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside_root).unwrap();
    }
}
