use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use git2::{Oid, Repository};

use crate::{
    content::RevisionContent,
    error::{DomainError, DomainResult},
    history::HistorySession,
    paths::{AbsoluteFilePath, RepoRelativePath, RepositoryRoot},
    revision::{
        AuthorEmail, AuthorName, AuthorTime, CommitMessage, CommitTime, Revision, RevisionId,
        RevisionIndex, RevisionSummary, ShortRevisionId,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryMode {
    Fast,
    FullHistory,
    FullHistoryNoMerges,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryScanProfile {
    pub commits_scanned: usize,
    pub revisions_found: usize,
    pub first_revision_after: Option<Duration>,
    pub first_revision_commit_index: Option<usize>,
    pub first_batch_after: Option<Duration>,
    pub first_batch_size: usize,
    pub total_duration: Duration,
}

pub fn open_file_history(
    repo_root: &RepositoryRoot,
    target_file: AbsoluteFilePath,
    repo_relative_path: RepoRelativePath,
) -> DomainResult<HistorySession> {
    let mut revisions = Vec::new();
    scan_file_history_with_mode(
        repo_root,
        &repo_relative_path,
        HistoryMode::FullHistoryNoMerges,
        |revision| {
            revisions.push(revision);
            Ok(())
        },
    )?;

    HistorySession::new(
        target_file,
        repo_root.clone(),
        repo_relative_path,
        revisions,
    )
}

pub fn scan_file_history(
    repo_root: &RepositoryRoot,
    repo_relative_path: &RepoRelativePath,
    on_revision: impl FnMut(Revision) -> DomainResult<()>,
) -> DomainResult<usize> {
    scan_file_history_with_mode(
        repo_root,
        repo_relative_path,
        HistoryMode::FullHistoryNoMerges,
        on_revision,
    )
}

pub fn scan_file_history_with_mode(
    repo_root: &RepositoryRoot,
    repo_relative_path: &RepoRelativePath,
    mode: HistoryMode,
    mut on_revision: impl FnMut(Revision) -> DomainResult<()>,
) -> DomainResult<usize> {
    let mut count = 0;

    stream_git_history(repo_root, repo_relative_path, mode, |record| {
        on_revision(record.into_revision(RevisionIndex::new(count))?)?;
        count += 1;
        Ok(())
    })?;

    Ok(count)
}

pub fn profile_file_history(
    repo_root: &RepositoryRoot,
    repo_relative_path: &RepoRelativePath,
    first_batch_size: usize,
) -> DomainResult<HistoryScanProfile> {
    profile_file_history_with_mode(
        repo_root,
        repo_relative_path,
        HistoryMode::FullHistoryNoMerges,
        first_batch_size,
    )
}

pub fn profile_file_history_with_mode(
    repo_root: &RepositoryRoot,
    repo_relative_path: &RepoRelativePath,
    mode: HistoryMode,
    first_batch_size: usize,
) -> DomainResult<HistoryScanProfile> {
    let started_at = Instant::now();
    let mut commits_scanned = 0;
    let mut revisions_found = 0;
    let mut first_revision_after = None;
    let mut first_revision_commit_index = None;
    let mut first_batch_after = None;

    stream_git_history(repo_root, repo_relative_path, mode, |_| {
        commits_scanned += 1;
        revisions_found += 1;

        if first_revision_after.is_none() {
            first_revision_after = Some(started_at.elapsed());
            first_revision_commit_index = Some(commits_scanned);
        }

        if first_batch_after.is_none() && revisions_found >= first_batch_size {
            first_batch_after = Some(started_at.elapsed());
        }

        Ok(())
    })?;

    Ok(HistoryScanProfile {
        commits_scanned,
        revisions_found,
        first_revision_after,
        first_revision_commit_index,
        first_batch_after,
        first_batch_size,
        total_duration: started_at.elapsed(),
    })
}

pub fn load_revision_content(
    repo_root: &RepositoryRoot,
    repo_relative_path: &RepoRelativePath,
    revision_id: &RevisionId,
) -> DomainResult<RevisionContent> {
    let repository = Repository::open(repo_root.as_path())
        .map_err(|error| git_error("open repository", error))?;
    let oid = Oid::from_str(revision_id.as_str())
        .map_err(|error| git_error("parse revision id", error))?;
    let commit = repository
        .find_commit(oid)
        .map_err(|error| git_error("find commit", error))?;
    let tree = commit
        .tree()
        .map_err(|error| git_error("load commit tree", error))?;

    let entry = match tree.get_path(repo_relative_path.as_path()) {
        Ok(entry) => entry,
        Err(_) => {
            return Ok(RevisionContent::Deleted {
                revision_id: revision_id.clone(),
            });
        }
    };

    let object = match entry.to_object(&repository) {
        Ok(object) => object,
        Err(error) => {
            return Ok(RevisionContent::Unavailable {
                revision_id: revision_id.clone(),
                message: format!("load tree entry object: {}", error.message()),
            });
        }
    };
    let blob = match object.peel_to_blob() {
        Ok(blob) => blob,
        Err(error) => {
            return Ok(RevisionContent::Unavailable {
                revision_id: revision_id.clone(),
                message: format!("peel blob: {}", error.message()),
            });
        }
    };

    if blob.is_binary() {
        return Ok(RevisionContent::Binary {
            revision_id: revision_id.clone(),
        });
    }

    match std::str::from_utf8(blob.content()) {
        Ok(content) => Ok(RevisionContent::Text {
            revision_id: revision_id.clone(),
            content: content.to_owned(),
            encoding: Some("utf-8".to_owned()),
        }),
        Err(_) => Ok(RevisionContent::UnsupportedEncoding {
            revision_id: revision_id.clone(),
            encoding: None,
        }),
    }
}

const FIELD_SEPARATOR: u8 = b'\0';

#[derive(Debug)]
struct GitLogRecord {
    id: String,
    short_id: String,
    author_name: String,
    author_email: String,
    author_seconds: String,
    author_date: String,
    commit_seconds: String,
    commit_date: String,
    summary: String,
}

impl GitLogRecord {
    fn parse(raw: &str) -> DomainResult<Self> {
        let mut parts = raw.split('\0');

        let id = next_field(&mut parts, "revision id")?;
        let short_id = next_field(&mut parts, "short revision id")?;
        let author_name = next_field(&mut parts, "author name")?;
        let author_email = next_field(&mut parts, "author email")?;
        let author_seconds = next_field(&mut parts, "author seconds")?;
        let author_date = next_field(&mut parts, "author date")?;
        let commit_seconds = next_field(&mut parts, "commit seconds")?;
        let commit_date = next_field(&mut parts, "commit date")?;
        let summary = next_field(&mut parts, "summary")?;
        Ok(Self {
            id,
            short_id,
            author_name,
            author_email,
            author_seconds,
            author_date,
            commit_seconds,
            commit_date,
            summary,
        })
    }

    fn into_revision(self, index: RevisionIndex) -> DomainResult<Revision> {
        let summary = normalize_summary(Some(&self.summary))?;
        let message = normalize_message(Some(summary.as_str()))?;

        Revision::new(
            RevisionId::new(self.id)?,
            ShortRevisionId::new(self.short_id)?,
            normalize_author_name(Some(&self.author_name))?,
            normalize_author_email(Some(&self.author_email))?,
            AuthorTime::new(
                parse_author_seconds(&self.author_seconds)?,
                parse_author_offset_minutes(&self.author_date)?,
            ),
            CommitTime::new(
                parse_author_seconds(&self.commit_seconds)?,
                parse_author_offset_minutes(&self.commit_date)?,
            ),
            summary,
            message,
            index,
        )
    }
}

fn stream_git_history(
    repo_root: &RepositoryRoot,
    repo_relative_path: &RepoRelativePath,
    mode: HistoryMode,
    mut on_record: impl FnMut(GitLogRecord) -> DomainResult<()>,
) -> DomainResult<()> {
    let format = "%H%x00%h%x00%an%x00%ae%x00%at%x00%ai%x00%ct%x00%cI%x00%s%x00".to_string();
    let output = Command::new("git")
        .current_dir(repo_root.as_path())
        .arg("log")
        .args(history_mode_args(mode))
        .arg(format!("--format={format}"))
        .arg("--")
        .arg(repo_relative_path.as_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| DomainError::Git {
            operation: "spawn git log",
            message: error.to_string(),
        })?;
    consume_git_log_output(output, &mut on_record)
}

fn history_mode_args(mode: HistoryMode) -> &'static [&'static str] {
    match mode {
        HistoryMode::Fast => &[],
        HistoryMode::FullHistory => &["--full-history"],
        HistoryMode::FullHistoryNoMerges => &["--full-history", "--no-merges"],
    }
}

fn consume_git_log_output(
    mut child: std::process::Child,
    on_record: &mut impl FnMut(GitLogRecord) -> DomainResult<()>,
) -> DomainResult<()> {
    let stdout = child.stdout.take().ok_or_else(|| DomainError::Git {
        operation: "read git log stdout",
        message: "git log stdout was not piped".to_owned(),
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| DomainError::Git {
        operation: "read git log stderr",
        message: "git log stderr was not piped".to_owned(),
    })?;

    let mut stdout = BufReader::new(stdout);
    let mut record = Vec::new();
    let field_count = 9;

    loop {
        record.clear();
        let bytes_read = stdout
            .read_until(FIELD_SEPARATOR, &mut record)
            .map_err(|error| DomainError::Git {
                operation: "read git log stdout",
                message: error.to_string(),
            })?;

        if bytes_read == 0 {
            break;
        }

        if record.last() != Some(&FIELD_SEPARATOR) {
            if record.iter().all(|byte| byte.is_ascii_whitespace()) {
                break;
            }

            return Err(DomainError::Git {
                operation: "read git log stdout",
                message: "unexpected trailing bytes in git log output".to_owned(),
            });
        }

        for _ in 1..field_count {
            let bytes_read = stdout
                .read_until(FIELD_SEPARATOR, &mut record)
                .map_err(|error| DomainError::Git {
                    operation: "read git log stdout",
                    message: error.to_string(),
                })?;

            if bytes_read == 0 {
                return Err(DomainError::Git {
                    operation: "read git log stdout",
                    message: "unexpected end of git log record".to_owned(),
                });
            }
        }

        if record.last() == Some(&FIELD_SEPARATOR) {
            let _ = record.pop();
        }

        let raw_record = String::from_utf8(record.clone()).map_err(|error| DomainError::Git {
            operation: "decode git log stdout",
            message: error.to_string(),
        })?;
        let raw_record = raw_record.trim_start_matches(['\n', '\r']);

        if raw_record.trim().is_empty() {
            continue;
        }

        on_record(GitLogRecord::parse(raw_record)?)?;
    }

    let mut stderr_buffer = String::new();
    stderr
        .read_to_string(&mut stderr_buffer)
        .map_err(|error| DomainError::Git {
            operation: "read git log stderr",
            message: error.to_string(),
        })?;

    let status = child.wait().map_err(|error| DomainError::Git {
        operation: "wait for git log",
        message: error.to_string(),
    })?;

    if !status.success() {
        return Err(DomainError::Git {
            operation: "git log",
            message: stderr_buffer.trim().to_owned(),
        });
    }

    Ok(())
}

fn next_field<'a>(
    parts: &mut impl Iterator<Item = &'a str>,
    field: &'static str,
) -> DomainResult<String> {
    parts
        .next()
        .map(str::to_owned)
        .ok_or_else(|| DomainError::Git {
            operation: "parse git log output",
            message: format!("missing {field}"),
        })
}

fn parse_author_seconds(raw: &str) -> DomainResult<i64> {
    raw.parse::<i64>().map_err(|error| DomainError::Git {
        operation: "parse author time",
        message: error.to_string(),
    })
}

fn parse_author_offset_minutes(raw: &str) -> DomainResult<i32> {
    if raw.ends_with('Z') {
        return Ok(0);
    }

    let offset = raw
        .split_whitespace()
        .last()
        .filter(|offset| offset.starts_with('+') || offset.starts_with('-'))
        .map(str::to_owned)
        .or_else(|| raw.get(raw.len().saturating_sub(6)..).map(str::to_owned))
        .ok_or_else(|| DomainError::Git {
            operation: "parse author timezone",
            message: format!("missing timezone in {raw}"),
        })?;

    let normalized = if offset.len() == 6 && offset.as_bytes()[3] == b':' {
        format!("{}{}{}", &offset[0..1], &offset[1..3], &offset[4..6])
    } else {
        offset
    };

    if normalized.len() != 5 {
        return Err(DomainError::Git {
            operation: "parse author timezone",
            message: format!("invalid timezone offset {raw}"),
        });
    }

    let sign = match &normalized[0..1] {
        "+" => 1,
        "-" => -1,
        _ => {
            return Err(DomainError::Git {
                operation: "parse author timezone",
                message: format!("invalid timezone offset {raw}"),
            });
        }
    };
    let hours = normalized[1..3]
        .parse::<i32>()
        .map_err(|error| DomainError::Git {
            operation: "parse author timezone",
            message: error.to_string(),
        })?;
    let minutes = normalized[3..5]
        .parse::<i32>()
        .map_err(|error| DomainError::Git {
            operation: "parse author timezone",
            message: error.to_string(),
        })?;

    Ok(sign * (hours * 60 + minutes))
}

fn normalize_author_name(name: Option<&str>) -> DomainResult<AuthorName> {
    match name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => AuthorName::new(name),
        None => AuthorName::new("Unknown Author"),
    }
}

fn normalize_author_email(email: Option<&str>) -> DomainResult<Option<AuthorEmail>> {
    match email.map(str::trim).filter(|email| !email.is_empty()) {
        Some(email) => match AuthorEmail::new(email) {
            Ok(email) => Ok(Some(email)),
            Err(DomainError::InvalidAuthorEmail(_)) => Ok(None),
            Err(error) => Err(error),
        },
        None => Ok(None),
    }
}

fn normalize_summary(summary: Option<&str>) -> DomainResult<RevisionSummary> {
    match summary.map(str::trim).filter(|summary| !summary.is_empty()) {
        Some(summary) => RevisionSummary::new(summary),
        None => RevisionSummary::new("<no summary>"),
    }
}

fn normalize_message(message: Option<&str>) -> DomainResult<CommitMessage> {
    match message.map(str::trim).filter(|message| !message.is_empty()) {
        Some(message) => CommitMessage::new(message),
        None => CommitMessage::new("<no message>"),
    }
}

fn git_error(operation: &'static str, error: git2::Error) -> DomainError {
    DomainError::Git {
        operation,
        message: error.message().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use git2::{IndexAddOption, Repository, Signature};

    use super::{GitLogRecord, normalize_author_email, parse_author_offset_minutes};
    use crate::{
        content::RevisionContent,
        engine::Engine,
        revision::{AuthorEmail, RevisionIndex},
    };

    fn test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("vestigia-git-{name}-{unique}"))
    }

    fn commit_file(
        repository: &Repository,
        workdir: &Path,
        relative_path: &str,
        content: &str,
        message: &str,
    ) {
        let file_path = workdir.join(relative_path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&file_path, content).unwrap();

        let mut index = repository.index().unwrap();
        index
            .add_all([relative_path], IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = Signature::now("Vestigia", "vestigia@example.com").unwrap();

        let parent_commit = repository
            .head()
            .ok()
            .and_then(|head| head.target())
            .map(|oid| repository.find_commit(oid).unwrap());

        match parent_commit.as_ref() {
            Some(parent) => repository
                .commit(
                    Some("HEAD"),
                    &signature,
                    &signature,
                    message,
                    &tree,
                    &[parent],
                )
                .unwrap(),
            None => repository
                .commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
                .unwrap(),
        };
    }

    #[test]
    fn open_file_history_returns_only_revisions_for_the_requested_file() {
        let root = test_dir("history");
        let repository = Repository::init(&root).unwrap();

        commit_file(&repository, &root, "src/lib.rs", "fn one() {}\n", "Add lib");
        commit_file(
            &repository,
            &root,
            "README.md",
            "# Vestigia\n",
            "Add readme",
        );
        commit_file(
            &repository,
            &root,
            "src/lib.rs",
            "fn two() {}\n",
            "Update lib",
        );

        let file_path = root.join("src/lib.rs");
        let engine = Engine::open_repository(&file_path).unwrap();
        let session = engine.open_file_history(&file_path).unwrap();

        assert_eq!(session.len(), 2);
        assert_eq!(session.current_index(), RevisionIndex::new(0));
        assert_eq!(session.current_revision().summary.as_str(), "Update lib");
        assert_eq!(session.revisions()[1].summary.as_str(), "Add lib");
        assert_eq!(session.revisions()[0].index, RevisionIndex::new(0));
        assert_eq!(session.revisions()[1].index, RevisionIndex::new(1));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn current_content_reads_and_caches_revision_text() {
        let root = test_dir("current-content");
        let repository = Repository::init(&root).unwrap();

        commit_file(&repository, &root, "src/lib.rs", "fn one() {}\n", "Add lib");
        commit_file(
            &repository,
            &root,
            "src/lib.rs",
            "fn two() {}\n",
            "Update lib",
        );

        let file_path = root.join("src/lib.rs");
        let engine = Engine::open_repository(&file_path).unwrap();
        let mut session = engine.open_file_history(&file_path).unwrap();

        let current = engine.current_content(&mut session).unwrap().clone();
        assert_eq!(current.as_text(), Some("fn two() {}\n"));

        let cached = session.current_content().unwrap();
        assert_eq!(cached, &current);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn revision_content_marks_deleted_files() {
        let root = test_dir("deleted-content");
        let repository = Repository::init(&root).unwrap();

        commit_file(&repository, &root, "src/lib.rs", "fn one() {}\n", "Add lib");

        let file_path = root.join("src/lib.rs");
        fs::remove_file(&file_path).unwrap();
        let mut index = repository.index().unwrap();
        index.remove_path(Path::new("src/lib.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = Signature::now("Vestigia", "vestigia@example.com").unwrap();
        let parent = repository
            .head()
            .ok()
            .and_then(|head| head.target())
            .map(|oid| repository.find_commit(oid).unwrap())
            .unwrap();
        repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Delete lib",
                &tree,
                &[&parent],
            )
            .unwrap();

        let deleted_path = root.join("src/lib.rs");
        let engine = Engine::open_repository(&deleted_path).unwrap();
        let mut session = engine.open_file_history(&deleted_path).unwrap();

        assert_eq!(session.current_revision().summary.as_str(), "Delete lib");
        let current = engine.current_content(&mut session).unwrap();
        assert!(matches!(current, RevisionContent::Deleted { .. }));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn normalize_author_email_ignores_invalid_values() {
        assert_eq!(normalize_author_email(Some("broken-email")).unwrap(), None);
        assert_eq!(normalize_author_email(Some("")).unwrap(), None);
        assert_eq!(normalize_author_email(None).unwrap(), None);
        assert_eq!(
            normalize_author_email(Some("vestigia@example.com"))
                .unwrap()
                .as_ref()
                .map(AuthorEmail::as_str),
            Some("vestigia@example.com")
        );
    }

    #[test]
    fn parse_author_offset_minutes_supports_git_and_iso_formats() {
        assert_eq!(
            parse_author_offset_minutes("2026-03-26 15:14:24 +0000").unwrap(),
            0
        );
        assert_eq!(
            parse_author_offset_minutes("2026-03-26 10:14:24 -0500").unwrap(),
            -300
        );
        assert_eq!(
            parse_author_offset_minutes("2026-03-26T15:14:24-05:00").unwrap(),
            -300
        );
        assert_eq!(
            parse_author_offset_minutes("2026-03-26T15:14:24Z").unwrap(),
            0
        );
    }

    #[test]
    fn git_log_record_parse_tolerates_prefixed_newlines_between_records() {
        let raw = r#"
fa896c9462cf3f7b525ed2bf03e2899f249e3501 fa896c9462cf Sandro sandro@example.com 1774538064 2026-03-26 15:14:24 +0000 1774538064 2026-03-26T15:14:24Z osmium: init at 0.0.16 (#497201)"#;
        let record = GitLogRecord::parse(raw.trim_start_matches(['\n', '\r'])).unwrap();

        assert_eq!(record.id, "fa896c9462cf3f7b525ed2bf03e2899f249e3501");
        assert_eq!(record.short_id, "fa896c9462cf");
        assert_eq!(record.author_name, "Sandro");
        assert_eq!(record.summary, "osmium: init at 0.0.16 (#497201)");
        assert_eq!(record.commit_date, "2026-03-26T15:14:24Z");
    }
}
