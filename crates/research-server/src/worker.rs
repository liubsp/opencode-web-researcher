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
                .config
                .composition_seconds(&job.prompt)
                .max(job.send_after.unwrap_or(wall).saturating_sub(wall).max(0) as u64);
            job.send_after = Some(wall + delay as i64);
            self.active = Some((job.id.clone(), clock + Duration::from_secs(delay)));
        }
        clock >= self.active.as_ref().unwrap().1
    }
}

pub async fn run(state: State) {
    let mut last_cleanup = Instant::now() - Duration::from_secs(60);
    let mut pacing = PacingClock::default();
    loop {
        let job = state.store.lock().unwrap().claim_next_job(now());
        match job {
            Ok(Some(mut job)) => {
                if let Err(error) = process(&state, &mut job, &mut pacing).await {
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
                        } else if !current.terminal() {
                            current.state = if current.state == "submitting" {
                                "submission_unknown"
                            } else {
                                "needs_attention"
                            }
                            .into();
                            current.error = Some(error.to_string());
                            let _ = store.save_job(&current);
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

async fn process(state: &State, job: &mut Job, pacing: &mut PacingClock) -> Result<()> {
    if job.state == "pacing" {
        // Waiting does not hold the store lock; list/cancel/status stay responsive.
        let store = state.store.lock().unwrap();
        if store.job(&job.id)?.state == "cancelled" {
            return Ok(());
        }
        let before = job.send_after;
        let ready = pacing.ready(job, Instant::now(), now());
        if before != job.send_after {
            store.save_job(job)?;
        }
        if !ready {
            return Ok(());
        }
        job.state = "preparing".into();
        store.save_job(job)?;
        pacing.active = None;
    }
    let mut thread = state.store.lock().unwrap().thread(&job.thread_id)?;
    let chrome = Chrome::ensure(&state.dir, &job.config).await?;
    if matches!(
        job.state.as_str(),
        "submitting" | "waiting" | "timed_out" | "cancel_requested"
    ) {
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
        .reconnect(
            thread.target.as_deref(),
            thread
                .url
                .as_deref()
                .or(thread.chatgpt_project_url.as_deref()),
        )
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
        let setup = chatgpt::prepare(
            &mut page,
            &job.config,
            thread.deep_research,
            thread.url.is_none(),
        )
        .await?;
        job.baseline = Some(setup["state"]["turns"].as_array().map_or(0, Vec::len));
        chatgpt::fill(&mut page, &job.prompt).await?;
        // Persist intent before Send. A crash beyond this boundary never causes automatic resubmission.
        job.state = "submitting".into();
        job.submitted_at = Some(now());
        job.selection = Some(
            json!({"reasoning":setup["reasoning"],"mode":if thread.deep_research {"deep_research"} else {"auto"}}),
        );
        state.store.lock().unwrap().save_job(job)?;
        chatgpt::send(&mut page).await?;
    }
    observe(state, job, &mut thread, &mut page).await
}

async fn observe(state: &State, job: &mut Job, thread: &mut Thread, page: &mut Page) -> Result<()> {
    let mut stable_text = String::new();
    let mut stable_since = Instant::now();
    let window = Instant::now();
    loop {
        let current = state.store.lock().unwrap().job(&job.id)?;
        if current.state == "cancel_requested" {
            page.eval("(() => { const b=document.querySelector('[data-testid=\"stop-button\"],#composer-stop-button'); if(b){b.click();return true}return false })()").await?;
            job.state = "cancel_requested".into();
        }
        let snapshot = chatgpt::inspect(page).await?;
        if snapshot["login_required"] == true {
            bail!("needs_login: cannot observe submitted request");
        }
        if let Some(url) = snapshot["url"]
            .as_str()
            .filter(|u| research_browser::conversation_url(u))
            && thread.url.as_deref() != Some(url)
        {
            ensure!(
                thread.url.is_none(),
                "Owned conversation changed while observing"
            );
            if let Some(project) = &thread.chatgpt_project_url {
                let prefix = project.strip_suffix("/project").unwrap_or(project);
                ensure!(
                    url.starts_with(&format!("{prefix}/c/")),
                    "project_mismatch: submitted conversation left the configured project"
                );
            }
            thread.url = Some(url.into());
            state.store.lock().unwrap().save_thread(thread)?;
        }
        let baseline = job
            .baseline
            .ok_or_else(|| anyhow::anyhow!("Missing submission baseline"))?;
        if chatgpt::observation::user_turn_confirmed(&snapshot, baseline, &job.prompt) {
            if job.state == "submitting" {
                job.state = "waiting".into();
                state.store.lock().unwrap().save_job(job)?;
            }
            let assistant = chatgpt::observation::response_after(&snapshot, baseline, &job.prompt)?;
            if let Some(response) = assistant {
                let text = response["markdown"].as_str().unwrap_or_default();
                if text != stable_text {
                    stable_text = text.into();
                    stable_since = Instant::now();
                }
                job.response = Some(response.clone());
                state.store.lock().unwrap().save_job(job)?;
                if chatgpt::observation::completion_candidate(&snapshot, response)
                    && stable_since.elapsed() >= Duration::from_secs(5)
                {
                    if thread.deep_research && chatgpt::start_deep_report(page).await? {
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
        } else if now() - job.submitted_at.unwrap_or(now()) > 60 {
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
            state.store.lock().unwrap().save_job(job)?;
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
            && now() - thread.active_at < (config.inactivity_hours * 3600) as i64
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

async fn retire_remote(state: &State, thread: &mut Thread, config: &Config) -> Result<()> {
    if let Some(url) = &thread.url {
        let chrome = Chrome::ensure(&state.dir, config).await?;
        let mut page = match chrome.reconnect(thread.target.as_deref(), Some(url)).await {
            Ok(page) => page,
            // Deletion can redirect the owned tab home before its confirmation is captured.
            // Verify the saved URL in a new background tab rather than touching a changed tab.
            Err(_) => chrome.open(url).await?,
        };
        // Persist replacement tabs before browser work so a retry reuses them.
        thread.target = Some(page.id.clone());
        state.store.lock().unwrap().save_thread(thread)?;
        chatgpt::ready(&mut page).await?;
        chatgpt::delete_conversation(&mut page, url).await?;
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
            Config::default(),
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
