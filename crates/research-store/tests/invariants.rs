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
    std::fs::write(&path, "modified archive")?;
    assert!(store.archive(&thread, dir.path()).is_err());
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
    assert!(folder.join("prompt.json").exists());
    assert!(!folder.join("exchange.json").exists());
    first.response = Some(serde_json::json!({"markdown":"first answer"}));
    store.finish(&mut first, "completed", None, 20)?;
    // Simulate shutdown after SQLite commit, before the file export. Recovery needs no browser.
    drop(store);
    let mut store = Store::open(&db)?;
    store.checkpoint(&store.job(&first.id)?, dir.path())?;
    let bytes = std::fs::read(folder.join("exchange.json"))?;
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
    assert_eq!(std::fs::read(folder.join("exchange.json"))?, bytes);
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
                86404
            )
            .is_err()
    );
    job.state = "timed_out".into();
    store.save_job(&job)?;
    assert!(!job.terminal());
    assert_eq!(store.next_job()?.unwrap().state, "timed_out");
    Ok(())
}
