use anyhow::{Result, bail, ensure};
use research_browser::Page;
use research_core::Config;
use serde_json::{Value, json};
use std::time::Duration;

pub mod observation;
mod read;
mod selection;
pub use read::capture_rendered_chat;

const INSPECT: &str = include_str!("scripts/inspect.js");
const ACTION: &str = include_str!("scripts/action.js");

pub async fn inspect(page: &mut Page) -> Result<Value> {
    page.eval(INSPECT).await
}

/// New project threads must remain on the configured project landing page until submission.
pub async fn verify_project(page: &mut Page, expected: &str) -> Result<()> {
    ensure!(
        research_core::valid_project_url(expected),
        "Invalid ChatGPT project URL"
    );
    ready(page).await?;
    let actual = page.eval("location.origin + location.pathname").await?;
    ensure!(
        actual.as_str() == Some(expected),
        "project_unavailable: ChatGPT redirected away from the configured project"
    );
    let heading = page.eval("[...document.querySelectorAll('main h1, main h2')].some(el => el.getClientRects().length && el.textContent.trim())").await?;
    ensure!(
        heading == true,
        "project_unavailable: project landing page could not be verified"
    );
    Ok(())
}

async fn action(page: &mut Page, op: &str, argument: Value) -> Result<Value> {
    page.eval(&format!(
        "({ACTION})({})",
        json!({"op":op,"argument":argument})
    ))
    .await
}

pub async fn ready(page: &mut Page) -> Result<Value> {
    for _ in 0..30 {
        let state = inspect(page).await?;
        if state["login_required"] == true {
            bail!("needs_login: sign in in the research Chrome window");
        }
        if state["composer"] == true {
            return Ok(state);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    bail!("needs_attention: ChatGPT composer not ready; inspect the research Chrome window")
}

pub async fn prepare(
    page: &mut Page,
    config: &Config,
    deep: bool,
    new_thread: bool,
) -> Result<Value> {
    ready(page).await?;
    if config.model != "default" {
        selection::model(page, &config.model).await?;
    }
    if new_thread && deep {
        let mode = "Deep research";
        let mut selected = action(page, "select_mode", json!(mode)).await?;
        if selected["ok"] != true {
            action(page, "open_tools", Value::Null).await?;
            tokio::time::sleep(Duration::from_millis(500)).await;
            selected = action(page, "select_mode", json!(mode)).await?;
        }
        ensure!(
            selected["ok"] == true,
            "research_mode_unavailable: {selected}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let reasoning = if deep {
        json!({"level":"mode-managed"})
    } else {
        selection::reasoning(page, &config.reasoning_preferences).await?
    };
    let state = inspect(page).await?;
    ensure!(
        state["busy"] != true,
        "thread_busy: ChatGPT is still generating"
    );
    Ok(json!({"state":state,"reasoning":reasoning}))
}

pub async fn fill(page: &mut Page, text: &str) -> Result<()> {
    let existing = inspect(page).await?;
    if existing["composer_text"]
        .as_str()
        .is_some_and(|value| value.trim() == text.trim())
    {
        return Ok(());
    }
    ensure!(
        existing["composer_text"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .is_empty(),
        "composer_not_empty: refusing to overwrite existing text"
    );
    ensure!(
        action(page, "focus", Value::Null).await?["ok"] == true,
        "composer_unavailable"
    );
    page.command("Input.insertText", json!({"text":text}))
        .await?;
    let state = inspect(page).await?;
    ensure!(
        state["composer_text"].as_str().unwrap_or_default().trim() == text.trim(),
        "composer_mismatch: not sending"
    );
    Ok(())
}

/// Caller must persist submission intent before calling this function.
pub async fn send(page: &mut Page) -> Result<()> {
    ensure!(
        action(page, "send", Value::Null).await?["ok"] == true,
        "send_not_confirmed"
    );
    Ok(())
}

pub async fn start_deep_report(page: &mut Page) -> Result<bool> {
    Ok(action(page, "start_report", Value::Null).await?["ok"] == true)
}

pub async fn delete_conversation(page: &mut Page, expected_url: &str) -> Result<()> {
    // Cleanup opens the exact saved URL when needed. A deleted conversation may redirect
    // home with an explicit deletion notice, allowing recovery after a missed confirmation toast.
    if action(page, "deleted", json!(expected_url)).await?["ok"] == true {
        return Ok(());
    }
    let state = inspect(page).await?;
    ensure!(
        state["url"].as_str() == Some(expected_url),
        "Deletion target changed"
    );
    ensure!(
        research_browser::conversation_url(expected_url),
        "Missing exact conversation URL"
    );
    ensure!(
        action(page, "open_chat_menu", json!(expected_url)).await?["ok"] == true,
        "delete_unavailable: cannot locate the exact conversation menu"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    ensure!(
        action(page, "select", json!(["Delete"])).await?["ok"] == true,
        "delete_unavailable"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    ensure!(
        action(page, "confirm_delete", Value::Null).await?["ok"] == true,
        "delete_confirmation_unavailable"
    );
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let result = action(page, "deleted", json!(expected_url)).await?;
        if result["ok"] == true {
            return Ok(());
        }
    }
    bail!("deletion_unknown: confirmation was clicked but deletion could not be verified")
}
