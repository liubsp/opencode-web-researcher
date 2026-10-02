use super::State;
use anyhow::{Result, bail, ensure};
use research_browser::{Chrome, Page};
use research_chatgpt as chatgpt;
use research_core::{Config, Job, Thread, now};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Default)]
struct PacingClock {
    active: Option<(String, Instant)>,
}

impl PacingClock {
    fn ready(&mut self, job: &mut Job, clock: Instant, wall: i64) -> bool {
        // On restart, conservatively compose again. During a run, wall-clock jumps cannot shorten pacing.
        if self.active.as_ref().is_none_or(|(id, _)| id != &job.id) {
            let delay = job
                .pacing_seconds()
                .max(job.send_after.unwrap_or(wall).saturating_sub(wall).max(0) as u64);
            job.send_after = Some(wall + delay as i64);
            self.active = Some((job.id.clone(), clock + Duration::from_secs(delay)));
        }
        clock >= self.active.as_ref().unwrap().1
    }
}

#[derive(Default)]
struct StopClock {
    last: Option<(String, Instant)>,
}

impl StopClock {
    fn ready(&mut self, request: &str, clock: Instant) -> bool {
        if self.last.as_ref().is_some_and(|(id, at)| {
            id == request && clock.duration_since(*at) < Duration::from_secs(5)
        }) {
            return false;
        }
        self.last = Some((request.into(), clock));
        true
    }
}

pub async fn run(state: State) {
    let mut last_cleanup = Instant::now() - Duration::from_secs(60);
    let mut pacing = PacingClock::default();
    let mut stop_clock = StopClock::default();
    loop {
        let job = state.store.lock().unwrap().claim_next_job(now());
        match job {
            Ok(Some(mut job)) => {
                if let Err(error) = process(&state, &mut job, &mut pacing, &mut stop_clock).await {
                    let mut store = state.store.lock().unwrap();
                    // Reload to avoid overwriting a concurrently requested cancellation.
                    if let Ok(mut current) = store.job(&job.id) {
                        if matches!(current.state.as_str(), "queued" | "pacing" | "preparing") {
                            let _ = store.finish(
                                &mut current,
                                "failed",
                                Some(error.to_string()),
                                now(),
                            );
                        } else if !current.terminal() && current.state != "cancel_requested" {
                            current.state = if current.state == "submitting" {
                                "submission_unknown"
                            } else {
                                "needs_attention"
                            }
                            .into();
                            current.error = Some(error.to_string());
                            let _ = store.save_progress(&mut current);
                        }
                    }
                    eprintln!("Request {} needs attention: {error}", job.id);
                }
                let store = state.store.lock().unwrap();
                if let Ok(current) = store.job(&job.id)
                    && let Err(error) = store.checkpoint(&current, &state.dir)
                {
                    eprintln!("Local transcript save failed: {error}");
                }
            }
            Ok(None) => {}
            Err(error) => eprintln!("Queue error: {error}"),
        }
        if let Err(error) = crate::read_chats::process_next(&state).await {
            eprintln!("Chat read failed: {error}");
        }
        if last_cleanup.elapsed() >= Duration::from_secs(60) {
            // Recover exports after a crash or temporary disk failure without opening Chrome.
            let export = || -> Result<()> {
                let mut store = state.store.lock().unwrap();
                let config = Config::load(&state.dir)?;
                store.purge_expired_reads(
                    &state.dir,
                    now() - (config.transcript_retention_days * 86400) as i64,
                )?;
                store.checkpoint_reads(&state.dir)?;
                store.purge_expired_transcripts(
                    &state.dir,
                    now() - (config.transcript_retention_days * 86400) as i64,
                )?;
                for thread in store.threads(None)? {
                    for job in store.jobs(&thread.id)? {
                        store.checkpoint(&job, &state.dir)?;
                    }
                    if thread.state == "remote_deleted" {
                        store.archive(&thread, &state.dir)?;
                    }
                }
                Ok(())
            };
            if let Err(error) = export() {
                eprintln!("Transcript recovery failed: {error}");
            }
            if let Err(error) = cleanup(&state).await {
                eprintln!("Cleanup error: {error}");
            }
            if let Err(error) = close_idle_browser(&state).await {
                eprintln!("Idle browser cleanup failed: {error}");
            }
            last_cleanup = Instant::now();
        }
        tokio::select! {
            _ = state.notify.notified() => {},
            _ = tokio::time::sleep(Duration::from_secs(1)) => {},
        }
    }
}

async fn process(
    state: &State,
    job: &mut Job,
    pacing: &mut PacingClock,
    stop_clock: &mut StopClock,
) -> Result<()> {
    if job.state == "pacing" {
        // Waiting does not hold the store lock; list/cancel/status stay responsive.
        let mut store = state.store.lock().unwrap();
        if store.job(&job.id)?.state == "cancelled" {
            return Ok(());
        }
        let before = job.send_after;
        let ready = pacing.ready(job, Instant::now(), now());
        if before != job.send_after && !store.save_progress(job)? {
            return Ok(());
        }
        if !ready {
            return Ok(());
        }
        job.state = "preparing".into();
        if !store.save_progress(job)? {
            return Ok(());
        }
        pacing.active = None;
    }
    let mut thread = state.store.lock().unwrap().thread(&job.thread_id)?;
    let chrome = Chrome::ensure(&state.dir, &job.config).await?;
    let observing = matches!(
        job.state.as_str(),
        "submitting" | "waiting" | "timed_out" | "cancel_requested"
    );
    if observing {
        ensure!(
            thread.target.is_some() || thread.url.is_some(),
            "No persisted target for submission recovery"
        );
        if thread.url.is_none() {
            ensure!(
                chrome
                    .targets()
                    .await?
                    .iter()
                    .any(|t| t["id"].as_str() == thread.target.as_deref()),
                "submission_unknown: original target lost before conversation URL was saved"
            );
        }
    }
    let mut page = chrome
        .reconnect(thread.target.as_deref(), reconnect_url(&thread, observing))
        .await?;
    thread.target = Some(page.id.clone());
    state.store.lock().unwrap().save_thread(&thread)?;
    if job.state == "preparing" {
        state.store.lock().unwrap().checkpoint(job, &state.dir)?;
        if thread.url.is_none()
            && let Some(project_url) = &thread.chatgpt_project_url
        {
            chatgpt::verify_project(&mut page, project_url).await?;
        }
        {
            let snapshot = chatgpt::ready(&mut page).await?;
            let draft = snapshot["composer_text"].as_str().unwrap_or_default();
            if !draft.trim().is_empty() && draft != research_core::canonical_prompt(&job.prompt) {
                let managed = {
                    let mut store = state.store.lock().unwrap();
                    let managed = if thread.url.is_none() {
                        store.unsent_draft(draft, thread.chatgpt_project_url.as_deref())?
                    } else {
                        store.unsent_thread_draft(draft, &thread.id)?
                    };
                    if let Some(managed) = &managed {
                        store.checkpoint(managed, &state.dir)?;
                        // Consume before the external delete: a crash must not
                        // authorize clearing a later identical human draft.
                        store.consume_draft(&managed.id, draft)?;
                    }
                    managed
                };
                ensure!(
                    managed.is_some(),
                    "composer_not_empty: refusing to overwrite unrelated text"
                );
                chatgpt::clear_managed_draft(&mut page, draft).await?;
            }
        }
        let setup = chatgpt::prepare(
            &mut page,
            &job.config,
            thread.deep_research,
            thread.url.is_none(),
        )
        .await?;
        let prompt = job.prompt.clone();
        chatgpt::fill_managed(&mut page, &prompt, |draft| {
            job.draft = Some(draft.into());
            ensure!(
                state.store.lock().unwrap().save_progress(job)?,
                "Request cancelled during preparation"
            );
            Ok(())
        })
        .await?;
        job.baseline = Some(setup["state"]["turns"].as_array().map_or(0, Vec::len));
        job.submission = chatgpt::observation::submission_anchor(&setup["state"]);
        // Persist intent before Send. A crash beyond this boundary never causes automatic resubmission.
        job.state = "submitting".into();
        job.submitted_at = Some(now());
        job.selection = Some(
            json!({"reasoning":setup["reasoning"],"mode":if thread.deep_research {"deep_research"} else {"auto"}}),
        );
        if !state.store.lock().unwrap().save_progress(job)? {
            return Ok(());
        }
        chatgpt::send(&mut page).await?;
    }
    observe(state, job, &mut thread, &mut page, stop_clock).await
}

fn reconnect_url(thread: &Thread, observing: bool) -> Option<&str> {
    thread.url.as_deref().or(if observing {
        None
    } else {
        thread.chatgpt_project_url.as_deref()
    })
}

async fn observe(
    state: &State,
    job: &mut Job,
    thread: &mut Thread,
    page: &mut Page,
    stop_clock: &mut StopClock,
) -> Result<()> {
    let mut stable_text = String::new();
    let mut stable_since = Instant::now();
    let window = Instant::now();
    loop {
        let current = state.store.lock().unwrap().job(&job.id)?;
        if current.state == "cancel_requested" {
            job.state = "cancel_requested".into();
        }
        let snapshot = chatgpt::inspect(page).await?;
        // Cancellation can arrive while CDP is awaited. Refresh before any state write/action.
        let current = state.store.lock().unwrap().job(&job.id)?;
        if current.terminal() {
            return Ok(());
        }
        if current.state == "cancel_requested" {
            job.state = current.state;
        }
        if snapshot["login_required"] == true {
            bail!("needs_login: cannot observe submitted request");
        }
        if let Some(url) = snapshot["url"]
            .as_str()
            .filter(|u| research_browser::conversation_url(u))
            && thread.url.as_deref() != Some(url)
        {
            ensure!(
                thread
                    .url
                    .as_deref()
                    .is_none_or(|previous| research_browser::same_conversation_url(previous, url)),
                "Owned conversation changed while observing"
            );
            if let Some(project) = &thread.chatgpt_project_url {
                ensure!(
                    research_browser::same_project_url(project, url),
                    "project_mismatch: submitted conversation left the configured project"
                );
            }
            thread.url = Some(url.into());
            state.store.lock().unwrap().save_thread(thread)?;
        }
        let baseline = job
            .baseline
            .ok_or_else(|| anyhow::anyhow!("Missing submission baseline"))?;
        if let Some(index) = chatgpt::observation::submitted_user(
            &snapshot,
            baseline,
            &job.prompt,
            job.submission.as_ref(),
        ) {
            if let Some(anchor) = &mut job.submission
                && anchor.user_turn_id.is_none()
            {
                anchor.user_turn_id = snapshot["turns"][index]["id"].as_str().map(str::to_owned);
                if !state.store.lock().unwrap().save_progress(job)? {
                    return Ok(());
                }
            }
            if job.state == "submitting" {
                job.state = "waiting".into();
                if !state.store.lock().unwrap().save_progress(job)? {
                    return Ok(());
                }
            }
            let assistant = chatgpt::observation::response_after_submission(
                &snapshot,
                baseline,
                &job.prompt,
                job.submission.as_ref(),
            )?;
            if let Some(response) = assistant {
                let text = response["markdown"].as_str().unwrap_or_default();
                if text != stable_text {
                    stable_text = text.into();
                    stable_since = Instant::now();
                }
                job.response = Some(response.clone());
                if !state.store.lock().unwrap().save_progress(job)? {
                    return Ok(());
                }
                if chatgpt::observation::completion_candidate(&snapshot, response)
                    && stable_since.elapsed() >= Duration::from_secs(5)
                {
                    if job.state != "cancel_requested"
                        && thread.deep_research
                        && chatgpt::start_deep_report(page).await?
                    {
                        stable_since = Instant::now();
                    } else {
                        let status = if job.state == "cancel_requested" {
                            "cancelled"
                        } else {
                            "completed"
                        };
                        state
                            .store
                            .lock()
                            .unwrap()
                            .finish(job, status, None, now())?;
                        return Ok(());
                    }
                }
            }
            if job.state == "cancel_requested"
                && snapshot["busy"] == true
                && stop_clock.ready(&job.id, Instant::now())
                && !chatgpt::stop(page).await?
            {
                job.error = Some("Stop control unavailable; still observing, not resending".into());
                if !state.store.lock().unwrap().save_progress(job)? {
                    return Ok(());
                }
            }
            // Neither Stop success nor failure bypasses the bounded observation/yield below.
            if job.state == "cancel_requested"
                && snapshot["busy"] != true
                && window.elapsed() >= Duration::from_secs(5)
            {
                state.store.lock().unwrap().finish(
                    job,
                    "cancelled",
                    Some("Generation stopped; response may be partial".into()),
                    now(),
                )?;
                return Ok(());
            }
        } else if window.elapsed() >= Duration::from_secs(15)
            && now() - job.submitted_at.unwrap_or(now()) > 60
        {
            bail!("submission_unknown: cannot confirm the user turn; no automatic resend");
        }
        let timeout = if thread.deep_research {
            job.config.deep_research_timeout_seconds
        } else {
            job.config.search_timeout_seconds
        };
        if now() - job.submitted_at.unwrap_or(now()) > timeout as i64
            && job.state != "cancel_requested"
        {
            job.state = "timed_out".into();
            job.error = Some(
                "Response deadline exceeded; still observing this request, not resending".into(),
            );
            if !state.store.lock().unwrap().save_progress(job)? {
                return Ok(());
            }
        }
        // Yield periodically so expiry can run even during a long report. Reconnect is safe and read-only.
        if window.elapsed() > Duration::from_secs(20) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn close_idle_browser(state: &State) -> Result<()> {
    {
        let store = state.store.lock().unwrap();
        for thread in store.threads(None)? {
            if store
                .jobs(&thread.id)?
                .iter()
                .any(|job| !job.terminal() && !matches!(job.state.as_str(), "queued" | "pacing"))
            {
                return Ok(());
            }
        }
    }
    let config = Config::load(&state.dir)?;
    if Chrome::close_if_idle(&state.dir, &config).await? {
        eprintln!("Closed idle research Chrome; browser work will reopen it automatically");
    }
    Ok(())
}

async fn cleanup(state: &State) -> Result<()> {
    let config = Config::load(&state.dir)?;
    let threads = state.store.lock().unwrap().threads(None)?;
    for mut thread in threads {
        if thread.state == "remote_deleted" || thread.cleanup_retry_at > now() {
            continue;
        }
        if thread.state == "active"
            && thread.prompts < 10
            && now() < thread.inactivity_deadline(&config)
        {
            continue;
        }
        {
            let store = state.store.lock().unwrap();
            if !store.jobs(&thread.id)?.iter().all(Job::terminal) {
                continue;
            }
            // Freeze the thread before doing any external I/O.
            thread.state = "archive_pending".into();
            store.save_thread(&thread)?;
            if let Err(error) = store.archive(&thread, &state.dir) {
                thread.cleanup_error = Some(error.to_string());
                schedule_cleanup_retry(&mut thread);
                store.save_thread(&thread)?;
                continue;
            }
            thread.state = "deletion_pending".into();
            store.save_thread(&thread)?;
        }
        let result = retire_remote(state, &mut thread, &config).await;
        match result {
            Ok(()) => {
                thread.state = "remote_deleted".into();
                thread.cleanup_error = None;
                thread.cleanup_attempts = 0;
                thread.cleanup_retry_at = 0;
            }
            Err(error) => {
                thread.cleanup_error = Some(error.to_string());
                schedule_cleanup_retry(&mut thread);
            }
        }
        state.store.lock().unwrap().save_thread(&thread)?;
        // Retire at most one thread per pass, even on success. The worker waits a full
        // 60 seconds after this pass finishes, so a startup/wake backlog cannot burst.
        // Per-thread retry deadlines still apply independently above.
        break;
    }
    Ok(())
}

fn cleanup_delay(attempts: u32) -> i64 {
    (60_i64 * (1_i64 << attempts.saturating_sub(1).min(10))).min(3600)
}

fn schedule_cleanup_retry(thread: &mut Thread) {
    thread.cleanup_attempts = thread.cleanup_attempts.saturating_add(1);
    thread.cleanup_retry_at = now() + cleanup_delay(thread.cleanup_attempts);
}

fn reusable_cleanup_location(current: &str, expected: &str) -> bool {
    research_browser::valid_chat_url(current)
        && (current.split(['?', '#']).next() == Some("https://chatgpt.com/")
            || research_core::valid_project_url(current)
                && research_browser::same_project_url(current, expected))
}

async fn retire_remote(state: &State, thread: &mut Thread, config: &Config) -> Result<()> {
    if let Some(url) = &thread.url {
        if let Some(receipt) = &thread.deletion_receipt {
            ensure!(
                research_browser::same_conversation_url(url, &receipt.url)
                    && matches!(receipt.evidence.as_str(), "ui_notice" | "ui_response"),
                "Deletion receipt does not match owned conversation"
            );
            return Ok(());
        }
        let chrome = Chrome::ensure(&state.dir, config).await?;
        let mut page = match chrome.reconnect(thread.target.as_deref(), Some(url)).await {
            Ok(page) => page,
            Err(_) => {
                // Cleanup can redirect its own tab home/project. Reuse that target
                // instead of accumulating one new tab on every failed retry. Never
                // navigate an unrelated conversation or another project's page.
                let reusable = chrome.targets().await?.iter().any(|target| {
                    target["id"].as_str() == thread.target.as_deref()
                        && target["url"]
                            .as_str()
                            .is_some_and(|current| reusable_cleanup_location(current, url))
                });
                if reusable {
                    let mut page = chrome.reconnect(thread.target.as_deref(), None).await?;
                    page.command("Page.navigate", json!({"url":url})).await?;
                    page
                } else {
                    chrome.open(url).await?
                }
            }
        };
        // Persist replacement tabs before browser work so a retry reuses them.
        thread.target = Some(page.id.clone());
        state.store.lock().unwrap().save_thread(thread)?;
        let evidence = chatgpt::delete_conversation(&mut page, url).await?;
        thread.deletion_receipt = Some(research_core::DeletionReceipt {
            url: url.clone(),
            confirmed_at: now(),
            evidence: evidence.into(),
        });
        // Persist the remote acknowledgement before closing the target or finalizing
        // local retirement; a crash here must not lose the evidence and re-delete.
        state.store.lock().unwrap().save_thread(thread)?;
        chrome.close(&page.id).await?;
    } else {
        // A thread which never submitted has no remote conversation to delete.
        let jobs = state.store.lock().unwrap().jobs(&thread.id)?;
        ensure!(
            jobs.iter().all(|j| j.submitted_at.is_none()),
            "Cannot delete: submitted conversation URL is unknown"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Requires Chrome; isolated profile and daemon, no account or submitted prompt"]
    async fn daemon_closes_idle_chrome_without_an_api_request() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config = Config::default();
        Chrome::ensure(dir.path(), &config).await?;
        std::fs::File::options()
            .write(true)
            .open(dir.path().join("chrome-activity"))?
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))?;
        let service = tokio::spawn(crate::serve(dir.path().to_owned()));
        let closed = tokio::time::timeout(Duration::from_secs(20), async {
            while dir.path().join("chrome.json").exists() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        let descriptor = crate::client::discover(dir.path()).await?;
        crate::client::rpc(&descriptor, json!({"op":"shutdown"}), 5).await?;
        service.await??;
        closed?;
        Ok(())
    }
    #[test]
    fn cleanup_reuses_only_owned_redirect_destinations_not_unrelated_tabs() {
        let expected = "https://chatgpt.com/g/g-p-0123456789abcdef0123456789abcdef-research/c/ours";
        assert!(reusable_cleanup_location("https://chatgpt.com/", expected));
        assert!(reusable_cleanup_location(
            "https://chatgpt.com/g/g-p-0123456789abcdef0123456789abcdef/project",
            expected
        ));
        for current in [
            "https://chatgpt.com/c/other",
            "https://chatgpt.com/g/g-p-fedcba9876543210fedcba9876543210/project",
            "https://chatgpt.com/library",
            "https://chatgpt.com.evil.test/",
            "about:blank",
        ] {
            assert!(!reusable_cleanup_location(current, expected));
        }
    }

    #[test]
    fn stop_spacing_survives_observation_yields_and_is_request_bound() {
        let mut clock = StopClock::default();
        let start = Instant::now();
        assert!(clock.ready("a", start));
        assert!(!clock.ready("a", start + Duration::from_millis(4999)));
        assert!(clock.ready("a", start + Duration::from_secs(5)));
        assert!(!clock.ready("a", start + Duration::from_secs(6)));
        assert!(clock.ready("b", start + Duration::from_secs(6)));
    }

    #[test]
    fn cleanup_backoff_grows_and_caps_without_overflow() {
        assert_eq!(
            (1..=4).map(cleanup_delay).collect::<Vec<_>>(),
            vec![60, 120, 240, 480]
        );
        assert_eq!(cleanup_delay(7), 3600);
        assert_eq!(cleanup_delay(u32::MAX), 3600);
    }
    use research_core::Submit;
    use research_store::Store;

    #[tokio::test]
    async fn retirement_recovers_durable_receipts_without_browser_or_redeletion() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("db"))?;
        let job = store.submit(
            &Submit {
                project: "synthetic".into(),
                session: "synthetic".into(),
                key: "receipt".into(),
                thread_id: None,
                prompt: "synthetic".into(),
                deep_research: false,
            },
            Config::default(),
            now(),
        )?;
        store.cancel(&job.id, now())?;
        let mut thread = store.thread(&job.thread_id)?;
        thread.url = Some("https://chatgpt.com/c/ours".into());
        thread.deletion_receipt = Some(research_core::DeletionReceipt {
            url: thread.url.clone().unwrap(),
            confirmed_at: now(),
            evidence: "ui_response".into(),
        });
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let state = State {
            dir: dir.path().into(),
            store: std::sync::Arc::new(std::sync::Mutex::new(store)),
            descriptor: research_core::ServiceDescriptor {
                protocol: research_core::PROTOCOL,
                port: 0,
                token: "synthetic".into(),
                instance: "synthetic".into(),
                pid: 0,
            },
            notify: std::sync::Arc::new(tokio::sync::Notify::new()),
            shutdown,
        };
        let config = Config {
            chrome_path: Some(dir.path().join("nonexistent-browser")),
            ..Config::default()
        };
        retire_remote(&state, &mut thread, &config).await?;
        assert!(!dir.path().join("chrome.json").exists());
        thread.deletion_receipt.as_mut().unwrap().url = "https://chatgpt.com/c/other".into();
        assert!(
            retire_remote(&state, &mut thread, &config)
                .await
                .unwrap_err()
                .to_string()
                .contains("receipt does not match")
        );
        Ok(())
    }

    #[tokio::test]
    async fn cancellation_during_inspection_never_starts_a_report_or_starves_the_worker()
    -> Result<()> {
        use futures_util::{SinkExt, StreamExt};
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        use tokio_tungstenite::tungstenite::Message;
        for busy in [false, true] {
            let dir = tempfile::tempdir()?;
            let mut store = Store::open(&dir.path().join("db"))?;
            let mut job = store.submit(
                &Submit {
                    project: "p".into(),
                    session: "s".into(),
                    key: "cancel-race".into(),
                    thread_id: None,
                    prompt: "ours".into(),
                    deep_research: true,
                },
                Config::default(),
                now(),
            )?;
            job.state = "waiting".into();
            job.submitted_at = Some(now());
            job.baseline = Some(0);
            store.save_job(&job)?;
            let mut thread = store.thread(&job.thread_id)?;
            thread.url = Some("https://chatgpt.com/c/synthetic".into());
            store.save_thread(&thread)?;
            let mut landing = thread.clone();
            landing.url = None;
            landing.chatgpt_project_url = Some("https://chatgpt.com/g/g-p-example/project".into());
            assert_eq!(
                reconnect_url(&landing, false),
                landing.chatgpt_project_url.as_deref()
            );
            assert_eq!(reconnect_url(&landing, true), None);
            assert_eq!(reconnect_url(&thread, true), thread.url.as_deref());
            let (shutdown, _) = tokio::sync::watch::channel(false);
            let state = State {
                dir: dir.path().into(),
                store: Arc::new(Mutex::new(store)),
                descriptor: research_core::ServiceDescriptor {
                    protocol: research_core::PROTOCOL,
                    port: 0,
                    token: "test".into(),
                    instance: "test".into(),
                    pid: 0,
                },
                notify: Arc::new(tokio::sync::Notify::new()),
                shutdown,
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let port = listener.local_addr()?.port();
            let clicks = Arc::new(AtomicUsize::new(0));
            let remote_clicks = clicks.clone();
            let api_store = state.store.clone();
            let id = job.id.clone();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                while let Some(Ok(Message::Text(raw))) = socket.next().await {
                    let request: serde_json::Value = serde_json::from_str(&raw).unwrap();
                    let expression = request["params"]["expression"].as_str().unwrap_or_default();
                    let value = if expression.contains("\"op\":\"start_report\"") {
                        remote_clicks.fetch_add(1, Ordering::SeqCst);
                        json!({"ok":true})
                    } else if expression.contains("\"op\":\"stop\"") {
                        json!({"ok":false})
                    } else {
                        // Cancellation arrives while the worker awaits CDP.
                        api_store.lock().unwrap().cancel(&id, now()).unwrap();
                        json!({"url":"https://chatgpt.com/c/synthetic","busy":busy,"turns":[{"role":"user","text":"ours"},{"role":"assistant","markdown":"research plan","complete":!busy}]})
                    };
                    socket
                        .send(Message::Text(
                            json!({"id":request["id"],"result":{"result":{"value":value}}})
                                .to_string()
                                .into(),
                        ))
                        .await
                        .unwrap();
                }
            });
            let mut page =
                Page::connect("synthetic".into(), &format!("ws://127.0.0.1:{port}/test")).await?;
            tokio::time::timeout(
                Duration::from_secs(26),
                observe(
                    &state,
                    &mut job,
                    &mut thread,
                    &mut page,
                    &mut StopClock::default(),
                ),
            )
            .await??;
            assert_eq!(clicks.load(Ordering::SeqCst), 0);
            assert_eq!(
                state.store.lock().unwrap().job(&job.id)?.state,
                if busy {
                    "cancel_requested"
                } else {
                    "cancelled"
                }
            );
            assert_eq!(state.store.lock().unwrap().thread(&thread.id)?.prompts, 1);
            drop(page);
            server.await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn expired_backlog_retires_only_one_thread_per_pass() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("state.sqlite"))?;
        for index in 0..10 {
            let mut job = store.submit(
                &Submit {
                    project: "p".into(),
                    session: "s".into(),
                    key: format!("expired-{index}"),
                    thread_id: None,
                    prompt: "test".into(),
                    deep_research: false,
                },
                Config::default(),
                1,
            )?;
            store.finish(&mut job, "failed", Some("never submitted".into()), 2)?;
        }
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let state = State {
            dir: dir.path().into(),
            store: std::sync::Arc::new(std::sync::Mutex::new(store)),
            descriptor: research_core::ServiceDescriptor {
                protocol: research_core::PROTOCOL,
                port: 0,
                token: "test".into(),
                instance: "test".into(),
                pid: 0,
            },
            notify: std::sync::Arc::new(tokio::sync::Notify::new()),
            shutdown,
        };
        for expected in 1..=2 {
            cleanup(&state).await?;
            let threads = state.store.lock().unwrap().threads(None)?;
            assert_eq!(
                threads
                    .iter()
                    .filter(|t| t.state == "remote_deleted")
                    .count(),
                expected
            );
        }
        Ok(())
    }

    #[test]
    fn pacing_uses_monotonic_time_and_restarts_conservatively() -> Result<()> {
        let mut store = Store::open(std::path::Path::new(":memory:"))?;
        let mut job = store.submit(
            &Submit {
                project: "p".into(),
                session: "s".into(),
                key: "k".into(),
                thread_id: None,
                prompt: "one two".into(),
                deep_research: false,
            },
            Config {
                pause_jitter_seconds: 0,
                ..Config::default()
            },
            10,
        )?;
        let start = Instant::now();
        let mut clock = PacingClock::default();
        assert!(!clock.ready(&mut job, start, 10));
        assert!(!clock.ready(&mut job, start + Duration::from_secs(17), 9999));
        assert!(clock.ready(&mut job, start + Duration::from_secs(18), 28));
        // A newly booted daemon never treats an old wall-clock timestamp as proof of elapsed pacing.
        let mut restarted = PacingClock::default();
        assert!(!restarted.ready(&mut job, start + Duration::from_secs(500), 510));
        assert_eq!(job.send_after, Some(528));
        let mut next_thread = job.clone();
        next_thread.id = "another-request".into();
        next_thread.thread_id = "another-thread".into();
        next_thread.send_after = None;
        assert!(!clock.ready(&mut next_thread, start + Duration::from_secs(900), 910));
        assert!(!clock.ready(&mut next_thread, start + Duration::from_secs(917), 927));
        assert!(clock.ready(&mut next_thread, start + Duration::from_secs(918), 928));
        Ok(())
    }
}
