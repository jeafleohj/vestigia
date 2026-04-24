use std::path::{Path, PathBuf};

use crate::error::{DomainError, DomainResult};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AbsoluteFilePath(PathBuf);

impl AbsoluteFilePath {
    pub fn new(path: impl Into<PathBuf>) -> DomainResult<Self> {
        let path = path.into();

        if !path.is_absolute() {
            return Err(DomainError::PathMustBeAbsolute(path));
        }

        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_inner(self) -> PathBuf {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepositoryRoot(PathBuf);

impl RepositoryRoot {
    pub fn new(path: impl Into<PathBuf>) -> DomainResult<Self> {
        let path = path.into();

        if !path.is_absolute() {
            return Err(DomainError::PathMustBeAbsolute(path));
        }

        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_inner(self) -> PathBuf {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoRelativePath(PathBuf);

impl RepoRelativePath {
    pub fn new(path: impl Into<PathBuf>) -> DomainResult<Self> {
        let path = path.into();

        if path.is_absolute() {
            return Err(DomainError::PathMustBeRelative(path));
        }

        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_inner(self) -> PathBuf {
        self.0
    }
}
