use crate::State;
use anyhow::{Result, ensure};
use research_browser::Chrome;
use research_core::{ChatReference, Config, now};
use serde_json::{Value, json};
use std::time::Duration;

pub async fn process_next(state: &State) -> Result<()> {
    let mut request = {
        let store = state.store.lock().unwrap();
        let Some(mut request) = store.next_read()? else {
            return Ok(());
        };
        request.state = "reading".into();
        store.save_read(&request)?;
        request
    };
    let reference = request.chats[request.results.len()].clone();
    let result = read_chat(state, &reference).await;
    // Cancellation may arrive while the browser read is in progress.
    let store = state.store.lock().unwrap();
    request = store.read_request(&request.id)?;
    let result = match result {
        Ok(mut result) => {
            result["chat_id"] = json!(reference.id);
            result["state"] = json!("completed");
            result
        }
        Err(error) => {
            json!({"chat_id":reference.id,"source_url":reference.url,"state":"failed","error":error.to_string(),"captured_at":now()})
        }
    };
    request.results.push(result);
    request.updated_at = now();
    if request.state != "cancelled" && request.results.len() == request.chats.len() {
        request.state = "completed".into();
    }
    store.save_read(&request)?;
    store.checkpoint_reads(&state.dir)?;
    Ok(())
}

async fn read_chat(state: &State, reference: &ChatReference) -> Result<Value> {
    let config = Config::load(&state.dir)?;
    let chrome = Chrome::ensure(&state.dir, &config).await?;
    // Use a dedicated read tab; never navigate an existing user/managed tab or register ownership.
    let mut page = chrome.open(&reference.url).await?;
    let capture = async {
        let mut loaded = false;
        for _ in 0..30 {
            let snapshot = research_chatgpt::inspect(&mut page).await?;
            ensure!(snapshot["login_required"] != true, "needs_login: sign in to the account that can access this chat");
            if ChatReference::parse(snapshot["url"].as_str().unwrap_or_default()).is_ok_and(|actual| actual.id == reference.id)
                && snapshot["turns"].as_array().is_some_and(|turns| !turns.is_empty()) {
                loaded = true;
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        ensure!(loaded, "chat_unavailable_or_empty: chat did not load; it may be deleted, inaccessible, or require another account");
        let result = research_chatgpt::capture_rendered_chat(&mut page).await?;
        let actual = ChatReference::parse(result["source_url"].as_str().unwrap_or_default())?;
        ensure!(actual.id == reference.id, "Chat changed during capture; refusing mismatched content");
        Ok(result)
    }.await;
    // Close only the temporary tab created above, never delete the remote conversation.
    if let Err(error) = chrome.close(&page.id).await {
        eprintln!("Read tab cleanup failed: {error}");
    }
    capture
}
