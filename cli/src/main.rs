use std::{path::PathBuf, process::ExitCode, time::Duration};

use clap::{Parser, Subcommand, ValueEnum};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use vestigia_core::{DomainError, Engine, HistoryMode, RevisionContent, RevisionIndex};

#[derive(Debug, Parser)]
#[command(name = "vestigia")]
#[command(about = "Inspect Git file history using the Vestigia core")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    History {
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = CliHistoryMode::FullHistoryNoMerges)]
        mode: CliHistoryMode,
    },
    Meta {
        path: PathBuf,
        #[arg(long, default_value_t = 0)]
        index: usize,
    },
    Show {
        path: PathBuf,
        #[arg(long, default_value_t = 0)]
        index: usize,
    },
    Profile {
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = CliHistoryMode::FullHistoryNoMerges)]
        mode: CliHistoryMode,
        #[arg(long, default_value_t = 32)]
        first_batch_size: usize,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliHistoryMode {
    Fast,
    FullHistory,
    FullHistoryNoMerges,
}

impl From<CliHistoryMode> for HistoryMode {
    fn from(value: CliHistoryMode) -> Self {
        match value {
            CliHistoryMode::Fast => HistoryMode::Fast,
            CliHistoryMode::FullHistory => HistoryMode::FullHistory,
            CliHistoryMode::FullHistoryNoMerges => HistoryMode::FullHistoryNoMerges,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", render_error(&error));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), DomainError> {
    match cli.command {
        Command::History { path, mode } => history(path, mode.into()),
        Command::Meta { path, index } => meta(path, index),
        Command::Show { path, index } => show(path, index),
        Command::Profile {
            path,
            mode,
            first_batch_size,
        } => profile(path, mode.into(), first_batch_size),
    }
}

fn history(path: PathBuf, mode: HistoryMode) -> Result<(), DomainError> {
    let engine = Engine::open_repository(&path)?;
    let (_, repo_relative_path) = engine.resolve_file_path(&path)?;
    let mut revisions = Vec::new();
    let _ = engine.scan_file_history_with_mode(&path, mode, |revision| {
        revisions.push(revision);
        Ok(())
    })?;

    println!("repo: {}", engine.repo_root().as_path().display());
    println!("file: {}", repo_relative_path.as_path().display());
    println!("mode: {}", render_mode(mode));
    println!();

    for revision in &revisions {
        println!(
            "[{}] {} {} {}",
            revision.index.get(),
            revision.short_id,
            revision.author_name.as_str(),
            revision.summary.as_str()
        );
    }

    Ok(())
}

fn meta(path: PathBuf, index: usize) -> Result<(), DomainError> {
    let engine = Engine::open_repository(&path)?;
    let mut session = engine.open_file_history(&path)?;
    let revision = session.jump_to(RevisionIndex::new(index))?;

    println!("index: {}", revision.index.get());
    println!("id: {}", revision.id);
    println!("short: {}", revision.short_id);
    println!("author: {}", revision.author_name.as_str());
    if let Some(email) = &revision.author_email {
        println!("email: {}", email.as_str());
    }
    println!(
        "author_date: {}",
        format_utc_datetime(revision.author_time.seconds())
    );
    println!(
        "commit_date: {}",
        format_utc_datetime(revision.commit_time.seconds())
    );
    println!("summary: {}", revision.summary.as_str());
    println!("message:\n{}", revision.message.as_str());

    Ok(())
}

fn show(path: PathBuf, index: usize) -> Result<(), DomainError> {
    let engine = Engine::open_repository(&path)?;
    let mut session = engine.open_file_history(&path)?;
    let _ = session.jump_to(RevisionIndex::new(index))?;
    let content = engine.current_content(&mut session)?;

    match content {
        RevisionContent::Deleted { .. } => {
            println!("<deleted in this revision>");
        }
        RevisionContent::Unavailable { message, .. } => {
            println!("<content unavailable: {message}>");
        }
        RevisionContent::Binary { .. } => {
            println!("<binary content>");
        }
        RevisionContent::UnsupportedEncoding { .. } => {
            println!("<unsupported encoding>");
        }
        RevisionContent::Text { content, .. } => {
            print!("{content}");
        }
    }

    Ok(())
}

fn profile(path: PathBuf, mode: HistoryMode, first_batch_size: usize) -> Result<(), DomainError> {
    let engine = Engine::open_repository(&path)?;
    let (_, repo_relative_path) = engine.resolve_file_path(&path)?;
    let profile = engine.profile_file_history_with_mode(&path, mode, first_batch_size)?;

    println!("repo: {}", engine.repo_root().as_path().display());
    println!("file: {}", repo_relative_path.as_path().display());
    println!("mode: {}", render_mode(mode));
    println!("commits_scanned: {}", profile.commits_scanned);
    println!("revisions_found: {}", profile.revisions_found);
    println!("first_batch_size: {}", profile.first_batch_size);
    println!(
        "first_revision_after: {}",
        profile
            .first_revision_after
            .map(format_duration)
            .unwrap_or_else(|| "none".to_owned())
    );
    println!(
        "first_revision_commit_index: {}",
        profile
            .first_revision_commit_index
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_owned())
    );
    println!(
        "first_batch_after: {}",
        profile
            .first_batch_after
            .map(format_duration)
            .unwrap_or_else(|| "none".to_owned())
    );
    println!(
        "total_duration: {}",
        format_duration(profile.total_duration)
    );

    Ok(())
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3}s", duration.as_secs_f64())
}

fn format_utc_datetime(seconds: i64) -> String {
    let Ok(datetime) = OffsetDateTime::from_unix_timestamp(seconds) else {
        return seconds.to_string();
    };

    datetime
        .to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .map(|value| value.replace('T', " ").trim_end_matches('Z').to_owned() + " UTC")
        .unwrap_or_else(|_| seconds.to_string())
}

fn render_mode(mode: HistoryMode) -> &'static str {
    match mode {
        HistoryMode::Fast => "fast",
        HistoryMode::FullHistory => "full-history",
        HistoryMode::FullHistoryNoMerges => "full-history-no-merges",
    }
}

fn render_error(error: &DomainError) -> String {
    match error {
        DomainError::EmptyCommitMessage => "empty commit message".to_owned(),
        DomainError::EmptySummary => "empty revision summary".to_owned(),
        DomainError::EmptyAuthorName => "empty author name".to_owned(),
        DomainError::InvalidAuthorEmail(email) => format!("invalid author email: {email}"),
        DomainError::EmptyRevisionId => "empty revision id".to_owned(),
        DomainError::EmptyShortRevisionId => "empty short revision id".to_owned(),
        DomainError::InvalidRevisionIndex { index, len } => {
            format!("revision index {index} is out of bounds for history length {len}")
        }
        DomainError::EmptyHistory => "file has no recorded history".to_owned(),
        DomainError::RepositoryNotFound(path) => {
            format!("no Git repository found for path {}", path.display())
        }
        DomainError::PathOutsideRepository { path, repo_root } => format!(
            "path {} is outside repository {}",
            path.display(),
            repo_root.display()
        ),
        DomainError::HistoryBackendUnavailable => "history backend is unavailable".to_owned(),
        DomainError::Git { operation, message } => {
            format!("git error during {operation}: {message}")
        }
        DomainError::PathMustBeAbsolute(path) => {
            format!("path must be absolute: {}", path.display())
        }
        DomainError::PathMustBeRelative(path) => {
            format!("path must be relative: {}", path.display())
        }
    }
}
