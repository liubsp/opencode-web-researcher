use anyhow::Result;
use research_core::{Config, Submit};
use research_store::Store;

fn input(key: &str, thread: Option<String>) -> Submit {
    Submit {
        project: "a".into(),
        session: "session-a".into(),
        key: key.into(),
        thread_id: thread,
        prompt: "check sources pls".into(),
        deep_research: false,
    }
}

#[test]
fn randomized_expiry_survives_reload_and_controls_followup_admission() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db");
    let config = Config {
        inactivity_max_hours: 48,
        ..Config::default()
    };
    let mut store = Store::open(&path)?;
    let mut job = store.submit(&input("first", None), config.clone(), 10)?;
    store.finish(&mut job, "failed", None, 20)?;
    let thread = store.thread(&job.thread_id)?;
    let deadline = thread.inactivity_deadline(&config);
    let delay = deadline - thread.active_at;
    assert!((86400..=172800).contains(&delay));
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(
        store.thread(&thread.id)?.inactivity_deadline(&config),
        deadline
    );
    let followup = input("followup", Some(thread.id.clone()));
    assert!(
        store
            .submit(&followup, config.clone(), deadline)
            .unwrap_err()
            .to_string()
            .contains("thread_expired")
    );
    store.submit(&followup, config.clone(), deadline - 1)?;
    let resumed = store.thread(&thread.id)?;
    assert_eq!(resumed.inactivity_deadline(&config), deadline - 1 + delay);
    // Deterministic boundary seeds cover both ends of the window and distinct per-chat offsets.
    let mut sample = thread;
    sample.id = "00000000-0000-0000-0000-000000000000".into();
    assert_eq!(
        sample.inactivity_deadline(&config) - sample.active_at,
        86400
    );
    sample.id = "00000000-0000-0000-0000-000000015180".into();
    assert_eq!(
        sample.inactivity_deadline(&config) - sample.active_at,
        172800
    );
    let defaults = Config::default();
    sample.id = "00000000-0000-0000-0000-00000007e900".into();
    assert_eq!(
        sample.inactivity_deadline(&defaults) - sample.active_at,
        604800
    );
    let fixed = Config {
        inactivity_max_hours: 24,
        ..defaults
    };
    assert_eq!(sample.inactivity_deadline(&fixed) - sample.active_at, 86400);
    Ok(())
}

#[test]
fn retries_cannot_change_payload_or_scope() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("db"))?;
    let original = input("key", None);
    let job = store.submit(&original, Config::default(), 10)?;
    let mut changed = original.clone();
    changed.prompt = "different".into();
    assert!(store.submit(&changed, Config::default(), 11).is_err());
    changed = original.clone();
    changed.session = "other".into();
    assert!(store.submit(&changed, Config::default(), 11).is_err());
    changed = original.clone();
    changed.deep_research = true;
    assert!(store.submit(&changed, Config::default(), 11).is_err());
    changed = input("other-key", Some(job.thread_id.clone()));
    changed.project = "b".into();
    assert!(store.submit(&changed, Config::default(), 11).is_err());
    assert_eq!(store.thread(&job.thread_id)?.prompts, 1);
    Ok(())
}

#[test]
fn exact_key_retries_only_provably_unsent_failure() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db");
    let mut store = Store::open(&path)?;
    let request = input("failed-preparation", None);
    let config = Config::default();
    let mut job = store.submit(&request, config.clone(), 10)?;
    assert_eq!(store.submit(&request, config.clone(), 11)?.id, job.id);
    job.state = "preparing".into();
    store.finish(&mut job, "failed", Some("composer not ready".into()), 20)?;
    assert_eq!(store.thread(&job.thread_id)?.prompts, 0);
    drop(store);
    let mut store = Store::open(&path)?;
    let retried = store.submit(&request, config.clone(), 30)?;
    assert_eq!(retried.id, job.id);
    assert_eq!(retried.state, "queued");
    assert_eq!(retried.created_at, 30);
    assert!(retried.error.is_none());
    assert!(retried.send_after.is_none());
    assert_eq!(store.thread(&job.thread_id)?.prompts, 1);
    assert_eq!(store.submit(&request, config.clone(), 31)?.id, job.id);
    assert_eq!(store.thread(&job.thread_id)?.prompts, 1);

    let mut submitted = retried;
    submitted.state = "submitting".into();
    submitted.submitted_at = Some(40);
    submitted.baseline = Some(0);
    store.finish(
        &mut submitted,
        "failed",
        Some("could not observe".into()),
        41,
    )?;
    let replay = store.submit(&request, config, 50)?;
    assert_eq!(replay.state, "failed");
    assert_eq!(replay.submitted_at, Some(40));
    assert_eq!(store.thread(&job.thread_id)?.prompts, 1);
    Ok(())
}

#[test]
fn unsent_retry_never_jumps_past_later_work_or_changes_payload() -> Result<()> {
    let mut store = Store::open(std::path::Path::new(":memory:"))?;
    let config = Config::default();
    let original = input("first", None);
    let mut job = store.submit(&original, config.clone(), 10)?;
    store.finish(&mut job, "failed", None, 11)?;
    let mut changed = original.clone();
    changed.prompt = "not the original".into();
    assert!(store.submit(&changed, config.clone(), 12).is_err());
    let followup = input("second", Some(job.thread_id.clone()));
    store.submit(&followup, config.clone(), 13)?;
    assert!(
        store
            .submit(&original, config, 14)
            .unwrap_err()
            .to_string()
            .contains("retry_order_conflict")
    );
    assert_eq!(store.job(&job.id)?.state, "failed");
    Ok(())
}

#[test]
fn archive_is_durable_and_retirement_retries_preserve_it() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("db"))?;
    let mut job = store.submit(&input("first", None), Config::default(), 10)?;
    let mut thread = store.thread(&job.thread_id)?;
    assert!(store.archive(&thread, dir.path()).is_err());
    job.state = "waiting".into();
    job.submitted_at = Some(20);
    job.response = Some(serde_json::json!({"markdown":"[source](https://example.org)"}));
    store.finish(&mut job, "completed", None, 30)?;
    thread = store.thread(&thread.id)?;
    store.archive(&thread, dir.path())?;
    thread.state = "deletion_pending".into();
    thread.cleanup_error = Some("login required".into());
    thread.cleanup_attempts = 5;
    thread.cleanup_retry_at = 9999;
    store.archive(&thread, dir.path())?;
    let path = dir
        .path()
        .join("archives")
        .join(&thread.id)
        .join("thread.md");
    assert!(std::fs::read_to_string(&path)?.contains("https://example.org"));
    let legacy = path.with_extension("json");
    std::fs::write(&legacy, serde_json::to_vec(&job)?)?;
    store.archive(&thread, dir.path())?;
    assert!(!legacy.exists());
    std::fs::write(&path, "modified archive")?;
    std::fs::write(&legacy, serde_json::to_vec(&job)?)?;
    assert!(store.archive(&thread, dir.path()).is_err());
    assert!(legacy.exists()); // Verification failure must not discard the previous export.
    Ok(())
}

#[test]
fn retention_requires_age_and_confirmed_remote_cleanup() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut store = Store::open(&dir.path().join("db"))?;
    let mut job = store.submit(&input("old", None), Config::default(), 1)?;
    store.finish(&mut job, "completed", None, 10)?;
    store.checkpoint(&job, dir.path())?;
    let mut thread = store.thread(&job.thread_id)?;
    store.archive(&thread, dir.path())?;
    assert_eq!(store.purge_expired_transcripts(dir.path(), 11)?, 0);
    thread.state = "deletion_pending".into();
    store.save_thread(&thread)?;
    assert_eq!(store.purge_expired_transcripts(dir.path(), 11)?, 0);
    thread.state = "remote_deleted".into();
    store.save_thread(&thread)?;
    assert_eq!(store.purge_expired_transcripts(dir.path(), 10)?, 0);
    assert_eq!(store.purge_expired_transcripts(dir.path(), 11)?, 1);
    assert!(store.thread(&thread.id).is_err());
    assert!(store.job(&job.id).is_err());
    assert!(!dir.path().join("transcripts").join(&thread.id).exists());
    assert!(!dir.path().join("archives").join(&thread.id).exists());
    assert_eq!(store.purge_expired_transcripts(dir.path(), 11)?, 0);
    Ok(())
}

#[test]
fn each_exchange_is_saved_before_retirement_and_recovers_from_database() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = dir.path().join("db");
    let mut store = Store::open(&db)?;
    let mut first = store.submit(&input("first", None), Config::default(), 10)?;
    store.checkpoint(&first, dir.path())?;
    let folder = dir
        .path()
        .join("transcripts")
        .join(&first.thread_id)
        .join(&first.id);
    assert!(folder.join("prompt.md").exists());
    assert!(!folder.join("exchange.md").exists());
    first.response = Some(serde_json::json!({"markdown":"first answer"}));
    store.finish(&mut first, "completed", None, 20)?;
    // Simulate shutdown after SQLite commit, before the file export. Recovery needs no browser.
    drop(store);
    let mut store = Store::open(&db)?;
    store.checkpoint(&store.job(&first.id)?, dir.path())?;
    let bytes = std::fs::read(folder.join("exchange.md"))?;
    assert!(std::fs::read_to_string(folder.join("exchange.md"))?.contains("first answer"));
    let mut second = store.submit(
        &input("second", Some(first.thread_id.clone())),
        Config::default(),
        21,
    )?;
    second.response = Some(serde_json::json!({"markdown":"second answer"}));
    store.finish(&mut second, "completed", None, 30)?;
    store.checkpoint(&second, dir.path())?;
    store.checkpoint(&first, dir.path())?;
    assert_eq!(std::fs::read(folder.join("exchange.md"))?, bytes);
    let full = std::fs::read_to_string(
        dir.path()
            .join("transcripts")
            .join(&first.thread_id)
            .join("thread.md"),
    )?;
    assert!(full.contains("first answer") && full.contains("second answer"));
    assert_eq!(store.thread(&first.thread_id)?.state, "active");
    Ok(())
}

#[test]
fn ambiguous_submission_blocks_new_work_until_observation_only_recovery() -> Result<()> {
    let mut store = Store::open(std::path::Path::new(":memory:"))?;
    let mut first = store.submit(&input("one", None), Config::default(), 1)?;
    first.state = "submission_unknown".into();
    first.submitted_at = Some(2);
    first.baseline = Some(0);
    store.save_job(&first)?;
    store.submit(&input("two", None), Config::default(), 3)?;
    assert!(store.next_job()?.is_none());
    assert!(store.cancel(&first.id, 4).is_err());
    let recovered = store.reconcile(&first.id)?;
    assert_eq!(recovered.state, "submitting");
    assert_eq!(recovered.baseline, Some(0));
    assert_eq!(store.next_job()?.unwrap().id, first.id);
    assert_eq!(store.thread(&first.thread_id)?.prompts, 1);
    Ok(())
}

#[test]
fn concurrent_reservations_cannot_overrun_the_tenth_prompt() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db");
    let mut store = Store::open(&path)?;
    let mut job = store.submit(&input("first", None), Config::default(), 1)?;
    for n in 1..=9 {
        job.state = "waiting".into();
        job.submitted_at = Some(2);
        store.finish(&mut job, "completed", None, 3)?;
        if n < 9 {
            job = store.submit(
                &input(&n.to_string(), Some(job.thread_id.clone())),
                Config::default(),
                4,
            )?;
        }
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|n| {
            let path = path.clone();
            let id = job.thread_id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                barrier.wait();
                store
                    .submit(&input(&format!("race-{n}"), Some(id)), Config::default(), 5)
                    .is_ok()
            })
        })
        .collect();
    let successful = handles
        .into_iter()
        .map(|h| u32::from(h.join().unwrap()))
        .sum::<u32>();
    assert_eq!(successful, 1);
    assert_eq!(store.thread(&job.thread_id)?.prompts, 10);
    Ok(())
}

#[test]
fn polling_does_not_refresh_ttl_and_timeouts_are_not_safe_to_archive() -> Result<()> {
    let mut store = Store::open(std::path::Path::new(":memory:"))?;
    let mut job = store.submit(&input("first", None), Config::default(), 1)?;
    job.state = "waiting".into();
    job.submitted_at = Some(2);
    store.finish(&mut job, "completed", None, 3)?;
    for _ in 0..10 {
        store.threads(Some("a"))?;
        store.job(&job.id)?;
    }
    assert_eq!(store.thread(&job.thread_id)?.active_at, 3);
    assert!(
        store
            .submit(
                &input("late", Some(job.thread_id.clone())),
                Config::default(),
                store
                    .thread(&job.thread_id)?
                    .inactivity_deadline(&Config::default())
            )
            .is_err()
    );
    job.state = "timed_out".into();
    store.save_job(&job)?;
    assert!(!job.terminal());
    assert_eq!(store.next_job()?.unwrap().state, "timed_out");
    Ok(())
}
