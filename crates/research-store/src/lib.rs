use anyhow::{Result, bail, ensure};
use research_core::{Config, Job, PROMPT_LIMIT, Submit, Thread};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use uuid::Uuid;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        ensure!(
            version <= 1,
            "Database belongs to a newer research service; refusing to downgrade"
        );
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS threads (id TEXT PRIMARY KEY, project TEXT NOT NULL, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY, thread_id TEXT NOT NULL,
                project TEXT NOT NULL, request_key TEXT NOT NULL, state TEXT NOT NULL,
                created_at INTEGER NOT NULL, data TEXT NOT NULL, UNIQUE(project, request_key));
            CREATE INDEX IF NOT EXISTS jobs_queue ON jobs(state,created_at);
            CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
            PRAGMA user_version=1;")?;
        Ok(Self { conn })
    }

    pub fn submit(&mut self, input: &Submit, config: Config, at: i64) -> Result<Job> {
        input.validate()?;
        config.validate()?;
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT data FROM jobs WHERE project=? AND request_key=?",
                params![input.project, input.key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(data) = prior {
            let job: Job = serde_json::from_str(&data)?;
            let original: String = tx.query_row(
                "SELECT data FROM threads WHERE id=?",
                [&job.thread_id],
                |r| r.get(0),
            )?;
            let original: Thread = serde_json::from_str(&original)?;
            ensure!(
                job.prompt == input.prompt
                    && input
                        .thread_id
                        .as_ref()
                        .is_none_or(|id| *id == job.thread_id),
                "idempotency_conflict"
            );
            ensure!(
                job.session == input.session
                    && job.new_thread == input.thread_id.is_none()
                    && (input.thread_id.is_some() || original.deep_research == input.deep_research),
                "idempotency_conflict"
            );
            return Ok(job);
        }
        let mut thread = if let Some(id) = &input.thread_id {
            let data: String = tx.query_row(
                "SELECT data FROM threads WHERE id=? AND project=?",
                params![id, input.project],
                |r| r.get(0),
            )?;
            let thread: Thread = serde_json::from_str(&data)?;
            ensure!(thread.state == "active", "thread_retired");
            ensure!(thread.prompts < PROMPT_LIMIT, "prompt_limit_reached");
            let pending: i64 = tx.query_row("SELECT count(*) FROM jobs WHERE thread_id=? AND state NOT IN ('completed','failed','cancelled')",
                [id], |r| r.get(0))?;
            ensure!(pending == 0, "thread_busy: wait for the existing request");
            ensure!(
                at - thread.active_at < (config.inactivity_hours * 3600) as i64,
                "thread_expired"
            );
            thread
        } else {
            Thread {
                chatgpt_project_url: config.chatgpt_project_url.clone(),
                id: Uuid::new_v4().to_string(),
                project: input.project.clone(),
                session: input.session.clone(),
                title: input.prompt.chars().take(100).collect(),
                deep_research: input.deep_research,
                url: None,
                target: None,
                created_at: at,
                active_at: at,
                prompts: 0,
                state: "active".into(),
                cleanup_error: None,
                cleanup_attempts: 0,
                cleanup_retry_at: 0,
            }
        };
        thread.prompts += 1;
        thread.active_at = at;
        let job = Job {
            id: Uuid::new_v4().to_string(),
            thread_id: thread.id.clone(),
            key: input.key.clone(),
            session: input.session.clone(),
            new_thread: input.thread_id.is_none(),
            prompt: input.prompt.clone(),
            config,
            state: "queued".into(),
            created_at: at,
            send_after: None,
            submitted_at: None,
            baseline: None,
            selection: None,
            response: None,
            error: None,
        };
        tx.execute(
            "INSERT OR REPLACE INTO threads VALUES (?,?,?)",
            params![thread.id, thread.project, serde_json::to_string(&thread)?],
        )?;
        tx.execute(
            "INSERT INTO jobs VALUES (?,?,?,?,?,?,?)",
            params![
                job.id,
                job.thread_id,
                input.project,
                job.key,
                job.state,
                at,
                serde_json::to_string(&job)?
            ],
        )?;
        tx.commit()?;
        Ok(job)
    }

    pub fn thread(&self, id: &str) -> Result<Thread> {
        let text: String =
            self.conn
                .query_row("SELECT data FROM threads WHERE id=?", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn threads(&self, project: Option<&str>) -> Result<Vec<Thread>> {
        let mut statement = self.conn.prepare(
            "SELECT data FROM threads WHERE (?1 IS NULL OR project=?1) ORDER BY rowid DESC",
        )?;
        let rows = statement.query_map([project], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }

    pub fn job(&self, id: &str) -> Result<Job> {
        let text: String = self
            .conn
            .query_row("SELECT data FROM jobs WHERE id=?", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn jobs(&self, thread: &str) -> Result<Vec<Job>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM jobs WHERE thread_id=? ORDER BY rowid")?;
        let rows = stmt.query_map([thread], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }

    pub fn save_thread(&self, thread: &Thread) -> Result<()> {
        self.conn.execute(
            "UPDATE threads SET data=? WHERE id=?",
            params![serde_json::to_string(thread)?, thread.id],
        )?;
        Ok(())
    }

    pub fn save_job(&self, job: &Job) -> Result<()> {
        self.conn.execute(
            "UPDATE jobs SET state=?, data=? WHERE id=?",
            params![job.state, serde_json::to_string(job)?, job.id],
        )?;
        Ok(())
    }

    pub fn next_job(&self) -> Result<Option<Job>> {
        let unresolved: i64 = self.conn.query_row(
            "SELECT count(*) FROM jobs WHERE state IN ('submission_unknown','needs_attention')",
            [],
            |r| r.get(0),
        )?;
        if unresolved > 0 {
            return Ok(None);
        } // Do not overlap unknown in-flight browser work.
        let data: Option<String> = self.conn.query_row(
            "SELECT data FROM jobs WHERE state IN ('queued','pacing','preparing','submitting','waiting','timed_out','cancel_requested') ORDER BY rowid LIMIT 1", [], |r| r.get(0)).optional()?;
        data.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
    }

    pub fn reconcile(&self, id: &str) -> Result<Job> {
        let mut job = self.job(id)?;
        ensure!(
            matches!(job.state.as_str(), "submission_unknown" | "needs_attention"),
            "Request does not need reconciliation"
        );
        ensure!(
            job.submitted_at.is_some() && job.baseline.is_some(),
            "Missing submission evidence"
        );
        job.state = "submitting".into(); // Observation-only path; it never clicks Send.
        job.error = None;
        self.save_job(&job)?;
        Ok(job)
    }

    pub fn last_finished(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM metadata WHERE key='last_finished'",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    pub fn finish(
        &mut self,
        job: &mut Job,
        state: &str,
        error: Option<String>,
        at: i64,
    ) -> Result<()> {
        let mut thread = self.thread(&job.thread_id)?;
        let definitely_unsent = matches!(job.state.as_str(), "queued" | "pacing" | "preparing");
        if definitely_unsent && matches!(state, "failed" | "cancelled") {
            thread.prompts = thread.prompts.saturating_sub(1);
        }
        job.state = state.into();
        job.error = error;
        thread.active_at = at;
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE jobs SET state=?,data=? WHERE id=?",
            params![job.state, serde_json::to_string(job)?, job.id],
        )?;
        tx.execute(
            "UPDATE threads SET data=? WHERE id=?",
            params![serde_json::to_string(&thread)?, thread.id],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO metadata VALUES ('last_finished',?)",
            [at],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn cancel(&mut self, id: &str, at: i64) -> Result<Job> {
        let mut job = self.job(id)?;
        if job.terminal() {
            return Ok(job);
        }
        if matches!(job.state.as_str(), "waiting" | "timed_out") {
            job.state = "cancel_requested".into();
            self.save_job(&job)?;
            return Ok(job);
        }
        if !matches!(job.state.as_str(), "queued" | "pacing") {
            bail!("already_started: submission is being reconciled; cannot safely cancel yet");
        }
        self.finish(&mut job, "cancelled", None, at)?;
        Ok(job)
    }

    pub fn archive(&self, thread: &Thread, dir: &Path) -> Result<()> {
        let jobs = self.jobs(&thread.id)?;
        ensure!(
            jobs.iter().all(Job::terminal),
            "Cannot archive nonterminal work"
        );
        let folder = dir.join("archives").join(&thread.id);
        std::fs::create_dir_all(&folder)?;
        let mut archived_thread = thread.clone();
        archived_thread.state = "archived".into();
        archived_thread.cleanup_error = None;
        archived_thread.cleanup_attempts = 0;
        archived_thread.cleanup_retry_at = 0;
        let json = serde_json::to_vec_pretty(
            &serde_json::json!({"thread":archived_thread,"requests":jobs}),
        )?;
        let mut markdown = format!("# {}\n\nThread: {}\n\n", thread.title, thread.id);
        for job in &jobs {
            markdown.push_str(&format!(
                "## User\n\n{}\n\n## ChatGPT ({})\n\n{}\n\n",
                job.prompt,
                job.state,
                job.response
                    .as_ref()
                    .and_then(|r| r["markdown"].as_str())
                    .unwrap_or("[No complete response captured]")
            ));
        }
        for (name, bytes) in [("thread.json", json), ("thread.md", markdown.into_bytes())] {
            let path = folder.join(name);
            // Write-once archives: retries verify existing bytes instead of replacing durable content.
            if path.exists() {
                ensure!(
                    transcript_matches(&path, &std::fs::read(&path)?, &bytes),
                    "Archive differs from persisted transcript: {}",
                    path.display()
                );
            } else {
                use std::io::Write;
                let tmp = folder.join(format!("{name}.tmp"));
                let mut file = std::fs::File::create(&tmp)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                std::fs::rename(tmp, path)?;
            }
        }
        Ok(())
    }

    /// Write prompt before dispatch and each terminal exchange independently of remote cleanup.
    pub fn checkpoint(&self, job: &Job, dir: &Path) -> Result<()> {
        let folder = dir.join("transcripts").join(&job.thread_id).join(&job.id);
        std::fs::create_dir_all(&folder)?;
        let prompt = serde_json::to_vec_pretty(&serde_json::json!({
            "request_id":job.id,"thread_id":job.thread_id,"prompt":job.prompt,
            "created_at":job.created_at
        }))?;
        write_once(&folder.join("prompt.json"), &prompt)?;
        if job.terminal() {
            let json = serde_json::to_vec_pretty(job)?;
            let markdown = format!(
                "## User\n\n{}\n\n## ChatGPT ({})\n\n{}\n",
                job.prompt,
                job.state,
                job.response
                    .as_ref()
                    .and_then(|r| r["markdown"].as_str())
                    .unwrap_or("[No response captured]")
            );
            write_once(&folder.join("exchange.json"), &json)?;
            write_once(&folder.join("exchange.md"), markdown.as_bytes())?;
        }
        Ok(())
    }

    pub fn purge_expired_transcripts(&mut self, dir: &Path, cutoff: i64) -> Result<usize> {
        let mut removed = 0;
        for thread in self.threads(None)? {
            // Keep recoverable work and the local copy required by pending remote deletion.
            if thread.state != "remote_deleted"
                || thread.active_at >= cutoff
                || !self.jobs(&thread.id)?.iter().all(Job::terminal)
            {
                continue;
            }
            for base in ["transcripts", "archives"] {
                let path = dir.join(base).join(&thread.id);
                match std::fs::remove_dir_all(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            let tx = self.conn.transaction()?;
            tx.execute("DELETE FROM jobs WHERE thread_id=?", [&thread.id])?;
            tx.execute("DELETE FROM threads WHERE id=?", [&thread.id])?;
            tx.commit()?;
            removed += 1;
        }
        Ok(removed)
    }
}

// Normalize only known config metadata, never prompts, responses, or unknown fields.
fn transcript_matches(path: &Path, saved: &[u8], current: &[u8]) -> bool {
    if saved == current {
        return true;
    }
    if path.extension().is_none_or(|ext| ext != "json") {
        return false;
    }
    fn normalize_job(job: &mut serde_json::Value) {
        let Some(config) = job
            .get_mut("config")
            .and_then(serde_json::Value::as_object_mut)
        else {
            return;
        };
        for (old, new) in [
            ("inactivity_hours", "remote_chat_inactivity_hours"),
            (
                "transcript_retention_days",
                "local_transcript_retention_days",
            ),
        ] {
            if !config.contains_key(new)
                && let Some(value) = config.remove(old)
            {
                config.insert(new.into(), value);
            }
        }
        if config.get("local_transcript_retention_days") == Some(&serde_json::json!(30)) {
            config.remove("local_transcript_retention_days");
        }
    }
    fn normalize(bytes: &[u8]) -> Option<serde_json::Value> {
        let mut value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        normalize_job(&mut value);
        if let Some(requests) = value
            .get_mut("requests")
            .and_then(serde_json::Value::as_array_mut)
        {
            for job in requests {
                normalize_job(job);
            }
        }
        Some(value)
    }
    match (normalize(saved), normalize(current)) {
        (Some(saved), Some(current)) => saved == current,
        _ => false,
    }
}

fn write_once(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        ensure!(
            transcript_matches(path, &std::fs::read(path)?, bytes),
            "Saved transcript differs: {}",
            path.display()
        );
        return Ok(());
    }
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transcript_config_aliases_preserve_integrity() {
        let old = br#"{"config":{"inactivity_hours":24},"response":{"markdown":"saved"}}"#;
        let new = br#"{"config":{"remote_chat_inactivity_hours":24,"local_transcript_retention_days":30},"response":{"markdown":"saved"}}"#;
        assert!(transcript_matches(Path::new("exchange.json"), old, new));
        let changed =
            br#"{"config":{"remote_chat_inactivity_hours":24},"response":{"markdown":"changed"}}"#;
        assert!(!transcript_matches(
            Path::new("exchange.json"),
            old,
            changed
        ));
        assert!(!transcript_matches(Path::new("exchange.md"), old, new));
        let archive_old = format!("{{\"requests\":[{}]}}", std::str::from_utf8(old).unwrap());
        let archive_new = format!("{{\"requests\":[{}]}}", std::str::from_utf8(new).unwrap());
        assert!(transcript_matches(
            Path::new("thread.json"),
            archive_old.as_bytes(),
            archive_new.as_bytes()
        ));
    }
    fn input(key: &str, thread: Option<String>) -> Submit {
        Submit {
            project: "project-a".into(),
            session: "session".into(),
            key: key.into(),
            thread_id: thread,
            prompt: "look up rust chrome docs pls".into(),
            deep_research: false,
        }
    }
    #[test]
    fn durable_budget_idempotency_and_archiving() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let db = dir.path().join("state.sqlite");
        let mut store = Store::open(&db)?;
        let first = store.submit(&input("first", None), Config::default(), 100)?;
        assert_eq!(
            store
                .submit(&input("first", None), Config::default(), 200)?
                .id,
            first.id
        );
        assert!(
            store
                .submit(
                    &input("busy", Some(first.thread_id.clone())),
                    Config::default(),
                    200
                )
                .is_err()
        );
        let mut job = first;
        for n in 1..=10 {
            job.state = "waiting".into();
            job.submitted_at = Some(200);
            store.finish(&mut job, "completed", None, 300)?;
            if n < 10 {
                job = store.submit(
                    &input(&n.to_string(), Some(job.thread_id.clone())),
                    Config::default(),
                    301,
                )?;
            }
        }
        drop(store);
        let mut store = Store::open(&db)?;
        assert!(
            store
                .submit(
                    &input("eleventh", Some(job.thread_id.clone())),
                    Config::default(),
                    302
                )
                .is_err()
        );
        let thread = store.thread(&job.thread_id)?;
        assert_eq!(thread.prompts, 10);
        store.archive(&thread, dir.path())?;
        assert!(
            dir.path()
                .join("archives")
                .join(thread.id)
                .join("thread.json")
                .exists()
        );
        Ok(())
    }
    #[test]
    fn cancellation_releases_only_unsent_budget() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let job = store.submit(&input("one", None), Config::default(), 1)?;
        store.cancel(&job.id, 2)?;
        assert_eq!(store.thread(&job.thread_id)?.prompts, 0);
        assert_eq!(
            store
                .submit(&input("one", None), Config::default(), 3)?
                .state,
            "cancelled"
        );
        Ok(())
    }
}
