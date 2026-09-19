use anyhow::Result;
use research_core::ChatReference;
use research_store::Store;
use serde_json::json;

#[test]
fn imports_are_durable_idempotent_and_never_managed_threads() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = dir.path().join("db");
    let mut store = Store::open(&db)?;
    let chats = vec![
        ChatReference::parse("00000000-0000-4000-8000-000000000001")?,
        ChatReference::parse("00000000-0000-4000-8000-000000000002")?,
    ];
    let mut read = store.submit_read("project", "session", "key", chats.clone(), 10)?;
    assert_eq!(
        store
            .submit_read("project", "session", "key", chats.clone(), 20)?
            .id,
        read.id
    );
    assert!(
        store
            .submit_read("project", "other", "key", chats.clone(), 20)
            .is_err()
    );
    assert!(
        store
            .submit_read("project", "session", "key", vec![chats[0].clone()], 20)
            .is_err()
    );
    assert!(store.threads(None)?.is_empty());
    assert!(store.next_job()?.is_none());
    assert_eq!(store.last_finished()?, 0);
    read.state = "reading".into();
    read.results.push(json!({"chat_id":chats[0].id,"state":"completed","markdown":"# Saved\n\ncafé 👋","turns":[{"role":"assistant"}]}));
    store.save_read(&read)?;
    drop(store);
    let mut store = Store::open(&db)?;
    assert_eq!(store.next_read()?.unwrap().results.len(), 1);
    store.purge_expired_reads(dir.path(), 100)?;
    assert!(store.read_request(&read.id).is_ok()); // In-progress batches are retained.
    read.results
        .push(json!({"chat_id":chats[1].id,"state":"failed","error":"inaccessible"}));
    read.state = "completed".into();
    read.updated_at = 30;
    store.save_read(&read)?;
    store.checkpoint_reads(dir.path())?;
    let file = dir.path().join("imports").join(&read.id).join("0.md");
    assert_eq!(std::fs::read_to_string(&file)?, "# Saved\n\ncafé 👋");
    assert!(read.summary()["results"][0].get("markdown").is_none());
    assert_eq!(read.summary()["results"][1]["state"], "failed");
    assert!(store.threads(None)?.is_empty());
    store.purge_expired_reads(dir.path(), 30)?;
    assert!(file.exists());
    store.purge_expired_reads(dir.path(), 31)?;
    assert!(!file.exists());
    assert!(store.read_request(&read.id).is_err());
    Ok(())
}
