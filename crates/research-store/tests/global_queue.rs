use anyhow::Result;
use research_core::{Config, Submit};
use research_store::Store;

fn input(project: &str, key: &str) -> Submit {
    Submit {
        project: project.into(),
        session: format!("session-{project}"),
        key: key.into(),
        thread_id: None,
        prompt: "one two".into(),
        deep_research: false,
    }
}

#[test]
fn only_one_global_slot_and_new_threads_wait_the_entire_delay() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = dir.path().join("db");
    let mut store = Store::open(&db)?;
    let first = store.submit(&input("project-a", "one"), Config::default(), 10)?;
    let second = store.submit(&input("project-b", "two"), Config::default(), 10)?;
    let mut active = store.claim_next_job(20)?.unwrap();
    assert_eq!(active.id, first.id);
    assert_eq!(active.send_after, Some(38)); // 3 seconds typing plus the full 15-second pause.
    let mut other = Store::open(&db)?;
    assert_eq!(other.claim_next_job(500)?.unwrap().id, first.id);
    assert_eq!(other.job(&second.id)?.state, "queued");
    assert_eq!(other.job(&second.id)?.send_after, None);
    let mut illicit = second.clone();
    illicit.state = "preparing".into();
    assert!(other.save_job(&illicit).is_err()); // Database-level enforcement across connections.
    for state in ["waiting", "timed_out", "cancel_requested"] {
        active.state = state.into();
        active.submitted_at = Some(38);
        store.save_job(&active)?;
        assert_eq!(other.claim_next_job(1000)?.unwrap().id, first.id);
    }
    active.state = "submission_unknown".into();
    store.save_job(&active)?;
    assert!(other.claim_next_job(1000)?.is_none());
    assert_eq!(other.job(&second.id)?.state, "queued");
    active.state = "waiting".into();
    store.save_job(&active)?;
    store.finish(&mut active, "completed", None, 1000)?;
    drop(store);
    drop(other);
    let mut restarted = Store::open(&db)?;
    let next = restarted.claim_next_job(1000)?.unwrap();
    assert_eq!(next.id, second.id);
    assert_eq!(next.send_after, Some(1018)); // Time spent queued never prepays pacing.
    restarted.cancel(&next.id, 1001)?;
    let third = restarted.submit(&input("project-c", "three"), Config::default(), 1001)?;
    let claimed = restarted.claim_next_job(1001)?.unwrap();
    assert_eq!(claimed.id, third.id);
    assert_eq!(claimed.send_after, Some(1019)); // Cancellation cannot transfer a pacing credit.
    Ok(())
}

#[test]
fn queued_cancellation_cannot_preempt_recovered_active_work() -> Result<()> {
    let mut store = Store::open(std::path::Path::new(":memory:"))?;
    let older = store.submit(&input("a", "old"), Config::default(), 1)?;
    let mut recovering = store.submit(&input("b", "recover"), Config::default(), 2)?;
    recovering.state = "waiting".into();
    store.save_job(&recovering)?;
    assert_eq!(store.claim_next_job(100)?.unwrap().id, recovering.id);
    store.cancel(&older.id, 100)?;
    assert_eq!(store.claim_next_job(101)?.unwrap().id, recovering.id);
    Ok(())
}
