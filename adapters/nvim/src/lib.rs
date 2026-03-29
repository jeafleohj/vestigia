use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread,
};

use nvim_oxi as oxi;
use nvim_oxi::{libuv::AsyncHandle, schedule};
use oxi::{
    Result,
    api::{
        self, Buffer, Window,
        opts::CreateCommandOptsBuilder,
        types::{CommandArgs, CommandComplete, CommandNArgs},
    },
};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use vestigia_core::{
    DomainError, Engine, HistoryMode, RepoRelativePath, Revision, RevisionContent, RevisionId,
};

type AdapterResult<T> = std::result::Result<T, AdapterError>;

const HISTORY_BATCH_SIZE: usize = 32;

static ACTIVE_SESSION: OnceLock<Mutex<Option<VestigiaSession>>> = OnceLock::new();
const HISTORY_MODE_NAMES: [&str; 3] = ["fast", "full-history", "full-history-no-merges"];

enum WorkerMessage {
    Batch(Vec<Revision>),
    Finished,
    Failed(String),
}

enum AdapterError {
    Domain(DomainError),
    Nvim(String),
    SessionClosed,
}

struct VestigiaSession {
    engine: Engine,
    mode: HistoryMode,
    target_file: PathBuf,
    repo_relative_path: RepoRelativePath,
    scratch: Buffer,
    revisions: Vec<Revision>,
    current_index: Option<usize>,
    content_cache: HashMap<RevisionId, RevisionContent>,
    loading_complete: bool,
    loading_error: Option<String>,
    update_rx: Receiver<WorkerMessage>,
}

#[oxi::plugin]
fn vestigia_nvim() -> Result<()> {
    let open_opts = CreateCommandOptsBuilder::default()
        .desc("Open Git file history with Vestigia")
        .nargs(CommandNArgs::ZeroOrOne)
        .complete(CommandComplete::CustomList(complete_history_modes.into()))
        .build();
    let prev_opts = CreateCommandOptsBuilder::default()
        .desc("Show the previous Git revision for the active Vestigia session")
        .build();
    let next_opts = CreateCommandOptsBuilder::default()
        .desc("Show the next Git revision for the active Vestigia session")
        .build();
    let meta_opts = CreateCommandOptsBuilder::default()
        .desc("Show metadata for the active Vestigia revision")
        .build();

    api::create_user_command("Vestigia", open_vestigia, &open_opts)?;
    api::create_user_command("VestigiaPrev", open_previous_revision, &prev_opts)?;
    api::create_user_command("VestigiaNext", open_next_revision, &next_opts)?;
    api::create_user_command("VestigiaMeta", show_revision_metadata, &meta_opts)?;

    Ok(())
}

fn open_vestigia(args: CommandArgs) -> Result<()> {
    let mode = match parse_history_mode_arg(&args) {
        Ok(mode) => mode,
        Err(error) => {
            api::err_writeln(&render_adapter_error(&error));
            return Ok(());
        }
    };

    if let Err(error) = run_vestigia(mode) {
        api::err_writeln(&render_adapter_error(&error));
    }

    Ok(())
}

fn run_vestigia(mode: HistoryMode) -> AdapterResult<()> {
    let current = Buffer::current();
    let file_path = current_file_path(&current)?;
    let engine = Engine::open_repository(&file_path).map_err(AdapterError::Domain)?;
    let (_, repo_relative_path) = engine
        .resolve_file_path(&file_path)
        .map_err(AdapterError::Domain)?;

    let scratch = open_or_reuse_scratch_window()?;
    let (update_tx, update_rx) = mpsc::channel();
    let handle = AsyncHandle::new(|| {
        schedule(|_| {
            if let Err(error) = process_worker_updates() {
                api::err_writeln(&render_adapter_error(&error));
            }

            Ok::<(), oxi::Error>(())
        });
    })
    .map_err(nvim_error)?;

    replace_active_session(VestigiaSession {
        engine: engine.clone(),
        mode,
        target_file: file_path.clone(),
        repo_relative_path,
        scratch,
        revisions: Vec::new(),
        current_index: None,
        content_cache: HashMap::new(),
        loading_complete: false,
        loading_error: None,
        update_rx,
    })?;
    render_session()?;

    let _ = thread::spawn(move || run_history_worker(engine, mode, file_path, update_tx, handle));

    Ok(())
}

fn open_previous_revision(_args: CommandArgs) -> Result<()> {
    if let Err(error) = with_active_session(|state| match state.current_index {
        Some(index) if index + 1 < state.revisions.len() => {
            state.current_index = Some(index + 1);
            render_state(state)
        }
        Some(_) if state.loading_complete => {
            api::err_writeln("Vestigia: already at oldest loaded revision");
            Ok(())
        }
        Some(_) | None => {
            api::err_writeln("Vestigia: history still loading older revisions");
            Ok(())
        }
    }) {
        api::err_writeln(&render_adapter_error(&error));
    }

    Ok(())
}

fn open_next_revision(_args: CommandArgs) -> Result<()> {
    if let Err(error) = with_active_session(|state| match state.current_index {
        Some(index) if index > 0 => {
            state.current_index = Some(index - 1);
            render_state(state)
        }
        Some(_) => {
            api::err_writeln("Vestigia: already at newest loaded revision");
            Ok(())
        }
        None => {
            api::err_writeln("Vestigia: history still loading");
            Ok(())
        }
    }) {
        api::err_writeln(&render_adapter_error(&error));
    }

    Ok(())
}

fn show_revision_metadata(_args: CommandArgs) -> Result<()> {
    if let Err(error) = with_active_session(|state| {
        let Some(revision) = current_revision(state) else {
            api::err_writeln("Vestigia: history still loading");
            return Ok(());
        };

        let mut lines = vec![
            format!("mode: {}", render_mode(state.mode)),
            format!("history: {}", render_history_summary(state)),
            format!("index: {}", display_revision_position(revision)),
            format!("id: {}", revision.id),
            format!("short: {}", revision.short_id),
            format!("author: {}", revision.author_name.as_str()),
        ];

        if let Some(email) = &revision.author_email {
            lines.push(format!("email: {}", email.as_str()));
        }

        lines.push(format!(
            "author_date: {}",
            format_utc_datetime(revision.author_time.seconds(),)
        ));
        lines.push(format!(
            "commit_date: {}",
            format_utc_datetime(revision.commit_time.seconds(),)
        ));
        lines.push(format!("summary: {}", revision.summary.as_str()));
        lines.push(String::new());
        lines.push("message:".to_owned());
        lines.extend(revision.message.as_str().lines().map(str::to_owned));

        api::command("botright new").map_err(nvim_error)?;
        let mut scratch = Buffer::current();
        api::command("setlocal buftype=nofile bufhidden=wipe noswapfile").map_err(nvim_error)?;
        api::command("setlocal nobuflisted").map_err(nvim_error)?;
        api::command("setlocal modifiable").map_err(nvim_error)?;
        scratch.set_lines(0..0, true, lines).map_err(nvim_error)?;
        api::command("setlocal nomodified").map_err(nvim_error)?;
        api::command("setlocal nomodifiable").map_err(nvim_error)?;
        api::command("nnoremap <silent> <buffer> q <Cmd>bwipeout!<CR>").map_err(nvim_error)?;
        api::command(&format!("file vestigia-meta://{}", revision.short_id)).map_err(nvim_error)?;

        Ok(())
    }) {
        api::err_writeln(&render_adapter_error(&error));
    }

    Ok(())
}

fn run_history_worker(
    engine: Engine,
    mode: HistoryMode,
    file_path: PathBuf,
    update_tx: Sender<WorkerMessage>,
    update_handle: AsyncHandle,
) {
    let mut batch = Vec::with_capacity(HISTORY_BATCH_SIZE);
    let mut sent_first_revision = false;

    let result = engine.scan_file_history_with_mode(&file_path, mode, |revision| {
        if !sent_first_revision {
            sent_first_revision = true;
            send_worker_message(
                &update_tx,
                &update_handle,
                WorkerMessage::Batch(vec![revision]),
            );
            return Ok(());
        }

        batch.push(revision);

        if batch.len() >= HISTORY_BATCH_SIZE {
            flush_batch(&update_tx, &update_handle, &mut batch);
        }

        Ok(())
    });

    flush_batch(&update_tx, &update_handle, &mut batch);

    match result {
        Ok(_) => send_worker_message(&update_tx, &update_handle, WorkerMessage::Finished),
        Err(error) => send_worker_message(
            &update_tx,
            &update_handle,
            WorkerMessage::Failed(render_domain_error(&error)),
        ),
    }
}

fn flush_batch(
    update_tx: &Sender<WorkerMessage>,
    update_handle: &AsyncHandle,
    batch: &mut Vec<Revision>,
) {
    if batch.is_empty() {
        return;
    }

    let message = WorkerMessage::Batch(std::mem::take(batch));
    send_worker_message(update_tx, update_handle, message);
}

fn send_worker_message(
    update_tx: &Sender<WorkerMessage>,
    update_handle: &AsyncHandle,
    message: WorkerMessage,
) {
    if update_tx.send(message).is_ok() {
        let _ = update_handle.send();
    }
}

fn process_worker_updates() -> AdapterResult<()> {
    let session = active_session_slot();
    let mut guard = session
        .lock()
        .map_err(|_| AdapterError::Nvim("failed to lock active Vestigia session".to_owned()))?;
    let Some(state) = guard.as_mut() else {
        return Ok(());
    };

    if !state.scratch.is_valid() {
        *guard = None;
        return Ok(());
    }

    let mut changed = false;

    loop {
        match state.update_rx.try_recv() {
            Ok(WorkerMessage::Batch(mut revisions)) => {
                if state.current_index.is_none() && !revisions.is_empty() {
                    state.current_index = Some(0);
                }

                state.revisions.append(&mut revisions);
                changed = true;
            }
            Ok(WorkerMessage::Finished) => {
                state.loading_complete = true;
                changed = true;
            }
            Ok(WorkerMessage::Failed(message)) => {
                state.loading_complete = true;
                state.loading_error = Some(message);
                changed = true;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                state.loading_complete = true;
                changed = true;
                break;
            }
        }
    }

    if changed {
        match render_state(state) {
            Ok(()) => {}
            Err(AdapterError::SessionClosed) => {
                *guard = None;
            }
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

fn render_session() -> AdapterResult<()> {
    match with_active_session(render_state) {
        Ok(()) => Ok(()),
        Err(AdapterError::SessionClosed) => {
            clear_active_session()?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn render_state(state: &mut VestigiaSession) -> AdapterResult<()> {
    if !state.scratch.is_valid() {
        return Err(AdapterError::SessionClosed);
    }

    let scratch = state.scratch.clone();

    let (lines, title, buffer_name) = match current_revision(state).cloned() {
        Some(revision) => {
            let lines = render_revision_content(current_content(state, &revision)?);
            let title = format!(
                "Vestigia [{}] [{} / {}] {} {} {}",
                render_mode(state.mode),
                display_revision_position(&revision),
                history_status_label(state),
                revision.short_id,
                revision.author_name.as_str(),
                revision.summary.as_str()
            );
            (lines, title, format!("vestigia://{}", revision.short_id))
        }
        None => {
            let mut lines = vec![
                format!("Loading history for {}", state.target_file.display()),
                format!("Loaded revisions: {}", state.revisions.len()),
            ];

            if let Some(message) = &state.loading_error {
                lines.push(String::new());
                lines.push(message.clone());
            } else if state.loading_complete {
                lines.push(String::new());
                lines.push("Vestigia: file has no recorded history".to_owned());
            }

            (
                lines,
                format!(
                    "Vestigia [{}] [loading {}]",
                    render_mode(state.mode),
                    history_status_label(state)
                ),
                format!("vestigia://{}", history_buffer_suffix(state)),
            )
        }
    };

    let escaped = title.replace('\'', "''");
    let mut buffer_for_call = scratch.clone();
    scratch
        .call::<_, _, ()>(move |_| -> Result<()> {
            api::command("setlocal modifiable")?;
            buffer_for_call.set_lines(.., true, lines)?;
            api::command("setlocal nomodified")?;
            api::command("setlocal nomodifiable")?;
            buffer_for_call.set_name(buffer_name)?;
            api::command(&format!("let &l:winbar = '{escaped}'"))?;
            Ok(())
        })
        .map_err(nvim_error)?;

    Ok(())
}

fn current_content<'a>(
    state: &'a mut VestigiaSession,
    revision: &Revision,
) -> AdapterResult<&'a RevisionContent> {
    if !state.content_cache.contains_key(&revision.id) {
        let content = state
            .engine
            .load_revision_content(&state.repo_relative_path, &revision.id)
            .map_err(AdapterError::Domain)?;
        state.content_cache.insert(revision.id.clone(), content);
    }

    Ok(state
        .content_cache
        .get(&revision.id)
        .expect("content must be cached after loading"))
}

fn current_revision(state: &VestigiaSession) -> Option<&Revision> {
    state
        .current_index
        .and_then(|index| state.revisions.get(index))
}

fn history_status_label(state: &VestigiaSession) -> String {
    if state.loading_complete {
        state.revisions.len().to_string()
    } else {
        format!("{}+", state.revisions.len())
    }
}

fn display_revision_position(revision: &Revision) -> usize {
    revision.index.get() + 1
}

fn render_history_summary(state: &VestigiaSession) -> String {
    if state.loading_complete {
        format!("complete ({} revisions)", state.revisions.len())
    } else {
        format!("loading ({} revisions loaded)", state.revisions.len())
    }
}

fn history_buffer_suffix(state: &VestigiaSession) -> String {
    current_revision(state)
        .map(|revision| revision.short_id.to_string())
        .unwrap_or_else(|| "loading".to_owned())
}

fn parse_history_mode_arg(args: &CommandArgs) -> AdapterResult<HistoryMode> {
    let Some(raw_mode) = args.fargs.first() else {
        return Ok(HistoryMode::Fast);
    };

    match raw_mode.as_str() {
        "fast" => Ok(HistoryMode::Fast),
        "full-history" => Ok(HistoryMode::FullHistory),
        "full-history-no-merges" => Ok(HistoryMode::FullHistoryNoMerges),
        other => Err(AdapterError::Nvim(format!(
            "invalid history mode `{other}`; expected fast, full-history, or full-history-no-merges"
        ))),
    }
}

fn complete_history_modes(
    (arg_lead, _cmdline, _cursor_pos): (String, String, usize),
) -> Vec<String> {
    HISTORY_MODE_NAMES
        .iter()
        .copied()
        .filter(|mode| mode.starts_with(arg_lead.as_str()))
        .map(str::to_owned)
        .collect()
}

fn render_mode(mode: HistoryMode) -> &'static str {
    match mode {
        HistoryMode::Fast => "fast",
        HistoryMode::FullHistory => "full-history",
        HistoryMode::FullHistoryNoMerges => "full-history-no-merges",
    }
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

fn open_scratch_window() -> AdapterResult<Buffer> {
    api::command("botright new").map_err(nvim_error)?;
    api::command("setlocal buftype=nofile bufhidden=hide noswapfile").map_err(nvim_error)?;
    api::command("setlocal modifiable").map_err(nvim_error)?;
    api::command("setlocal nomodifiable").map_err(nvim_error)?;
    api::command("nnoremap <silent> <buffer> [h <Cmd>VestigiaPrev<CR>").map_err(nvim_error)?;
    api::command("nnoremap <silent> <buffer> ]h <Cmd>VestigiaNext<CR>").map_err(nvim_error)?;
    api::command("nnoremap <silent> <buffer> gm <Cmd>VestigiaMeta<CR>").map_err(nvim_error)?;
    api::command("nnoremap <silent> <buffer> q <Cmd>close<CR>").map_err(nvim_error)?;
    Ok(Buffer::current())
}

fn open_or_reuse_scratch_window() -> AdapterResult<Buffer> {
    let Some(scratch) = active_scratch_buffer()? else {
        return open_scratch_window();
    };

    if let Some(window) = find_window_for_buffer(&scratch)? {
        api::set_current_win(&window).map_err(nvim_error)?;
        return Ok(scratch);
    }

    api::command("botright new").map_err(nvim_error)?;
    api::set_current_buf(&scratch).map_err(nvim_error)?;
    Ok(scratch)
}

fn active_scratch_buffer() -> AdapterResult<Option<Buffer>> {
    let session = active_session_slot();
    let guard = session
        .lock()
        .map_err(|_| AdapterError::Nvim("failed to lock active Vestigia session".to_owned()))?;

    Ok(guard
        .as_ref()
        .and_then(|state| state.scratch.is_valid().then(|| state.scratch.clone())))
}

fn find_window_for_buffer(buffer: &Buffer) -> AdapterResult<Option<Window>> {
    for window in api::list_wins() {
        let Ok(window_buffer) = window.get_buf() else {
            continue;
        };

        if window_buffer == *buffer {
            return Ok(Some(window));
        }
    }

    Ok(None)
}

fn replace_active_session(state: VestigiaSession) -> AdapterResult<()> {
    let session = active_session_slot();
    let mut guard = session
        .lock()
        .map_err(|_| AdapterError::Nvim("failed to lock active Vestigia session".to_owned()))?;
    *guard = Some(state);
    Ok(())
}

fn clear_active_session() -> AdapterResult<()> {
    let session = active_session_slot();
    let mut guard = session
        .lock()
        .map_err(|_| AdapterError::Nvim("failed to lock active Vestigia session".to_owned()))?;
    *guard = None;
    Ok(())
}

fn with_active_session<T>(
    f: impl FnOnce(&mut VestigiaSession) -> AdapterResult<T>,
) -> AdapterResult<T> {
    let session = active_session_slot();
    let mut guard = session
        .lock()
        .map_err(|_| AdapterError::Nvim("failed to lock active Vestigia session".to_owned()))?;
    if guard
        .as_ref()
        .is_some_and(|state| !state.scratch.is_valid())
    {
        *guard = None;
    }
    let state = guard.as_mut().ok_or_else(|| {
        AdapterError::Nvim("no active Vestigia session; run :Vestigia first".to_owned())
    })?;

    f(state)
}

fn active_session_slot() -> &'static Mutex<Option<VestigiaSession>> {
    ACTIVE_SESSION.get_or_init(|| Mutex::new(None))
}

fn current_file_path(buffer: &Buffer) -> AdapterResult<PathBuf> {
    let path = buffer.get_name().map_err(nvim_error)?;

    if path.as_os_str().is_empty() {
        return Err(AdapterError::Domain(DomainError::RepositoryNotFound(
            PathBuf::from("<unnamed-buffer>"),
        )));
    }

    Ok(path)
}

fn render_revision_content(content: &RevisionContent) -> Vec<String> {
    match content {
        RevisionContent::Deleted { .. } => vec!["<deleted in this revision>".to_owned()],
        RevisionContent::Unavailable { message, .. } => {
            vec![format!("<content unavailable: {message}>")]
        }
        RevisionContent::Binary { .. } => vec!["<binary content>".to_owned()],
        RevisionContent::UnsupportedEncoding { .. } => {
            vec!["<unsupported encoding>".to_owned()]
        }
        RevisionContent::Text { content, .. } => content.lines().map(str::to_owned).collect(),
    }
}

fn render_domain_error(error: &DomainError) -> String {
    match error {
        DomainError::EmptyCommitMessage => "Vestigia: empty commit message".to_owned(),
        DomainError::EmptySummary => "Vestigia: empty revision summary".to_owned(),
        DomainError::EmptyAuthorName => "Vestigia: empty author name".to_owned(),
        DomainError::InvalidAuthorEmail(email) => {
            format!("Vestigia: invalid author email: {email}")
        }
        DomainError::EmptyRevisionId => "Vestigia: empty revision id".to_owned(),
        DomainError::EmptyShortRevisionId => "Vestigia: empty short revision id".to_owned(),
        DomainError::InvalidRevisionIndex { index, len } => {
            format!("Vestigia: revision index {index} is out of bounds for history length {len}")
        }
        DomainError::EmptyHistory => "Vestigia: file has no recorded history".to_owned(),
        DomainError::RepositoryNotFound(path) => {
            format!("Vestigia: no Git repository found for {}", path.display())
        }
        DomainError::PathOutsideRepository { path, repo_root } => format!(
            "Vestigia: path {} is outside repository {}",
            path.display(),
            repo_root.display()
        ),
        DomainError::HistoryBackendUnavailable => {
            "Vestigia: history backend is unavailable".to_owned()
        }
        DomainError::Git { operation, message } => {
            format!("Vestigia: git error during {operation}: {message}")
        }
        DomainError::PathMustBeAbsolute(path) => {
            format!("Vestigia: path must be absolute: {}", path.display())
        }
        DomainError::PathMustBeRelative(path) => {
            format!("Vestigia: path must be relative: {}", path.display())
        }
    }
}

fn render_adapter_error(error: &AdapterError) -> String {
    match error {
        AdapterError::Domain(error) => render_domain_error(error),
        AdapterError::Nvim(message) => format!("Vestigia: Neovim error: {message}"),
        AdapterError::SessionClosed => "Vestigia: session closed".to_owned(),
    }
}

fn nvim_error<E: ToString>(error: E) -> AdapterError {
    AdapterError::Nvim(error.to_string())
}
