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
        actual
            .as_str()
            .is_some_and(|url| research_core::valid_project_url(url)
                && research_browser::same_project_url(url, expected)),
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
    let initial = ready(page).await?;
    ensure!(
        deep || initial["mode"].is_null(),
        "research_mode_mismatch: ordinary chat requested but a composer mode is active"
    );
    if config.model != "default" {
        selection::model(page, &config.model).await?;
    }
    if new_thread && deep {
        let mode = "Deep research";
        let mut selected = Value::Null;
        let mut opened = false;
        for _ in 0..30 {
            selected = action(page, "select_mode", json!(mode)).await?;
            if selected["ok"] == true {
                break;
            }
            if !opened {
                opened = action(page, "open_tools", Value::Null).await?["ok"] == true;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        ensure!(
            selected["ok"] == true,
            "research_mode_unavailable: {selected}"
        );
        let mut applied = false;
        for _ in 0..30 {
            if inspect(page).await?["mode"] == "deep_research" {
                applied = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        ensure!(
            applied,
            "research_mode_unavailable: Deep Research selection was not applied"
        );
    }
    let reasoning = if deep {
        json!({"level":"mode-managed"})
    } else {
        selection::reasoning(page, &config.reasoning_preferences).await?
    };
    let state = inspect(page).await?;
    ensure!(
        if deep {
            !new_thread || state["mode"] == "deep_research"
        } else {
            state["mode"].is_null()
        },
        "research_mode_mismatch: composer mode changed during preparation"
    );
    ensure!(
        state["busy"] != true,
        "thread_busy: ChatGPT is still generating"
    );
    Ok(json!({"state":state,"reasoning":reasoning}))
}

pub async fn fill(page: &mut Page, text: &str) -> Result<()> {
    fill_managed(page, text, |_| Ok(())).await
}

/// Record successful insertion before verification so a failed round-trip retains draft provenance.
pub async fn fill_managed(
    page: &mut Page,
    text: &str,
    mut written: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let text = research_core::canonical_prompt(text);
    let existing = inspect(page).await?;
    if existing["composer_text"]
        .as_str()
        .is_some_and(|value| value == text)
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
    let actual = state["composer_text"].as_str().unwrap_or_default();
    written(actual)?;
    ensure!(actual == text, "composer_mismatch: not sending");
    Ok(())
}

/// The caller must preserve and identify an unsent managed prompt before clearing it.
pub async fn clear_managed_draft(page: &mut Page, expected: &str) -> Result<()> {
    // Check and delete in one browser evaluation; never clear an edited or unrelated draft.
    let result = page
        .eval(&format!(
            "(() => {{ const state = {INSPECT}; if (state.busy || state.composer_text !== {}) return {{ok:false,error:'draft_changed'}}; return ({ACTION})({{op:'clear_draft'}}); }})()",
            json!(expected.trim())
        ))
        .await?;
    ensure!(result["ok"] == true, "managed_draft_not_cleared: {result}");
    ensure!(
        inspect(page).await?["composer_text"].as_str() == Some(""),
        "managed_draft_not_cleared: composer still contains text"
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

pub async fn stop(page: &mut Page) -> Result<bool> {
    Ok(action(page, "stop", Value::Null).await?["ok"] == true)
}

pub async fn delete_conversation(page: &mut Page, expected_url: &str) -> Result<&'static str> {
    ensure!(
        research_browser::conversation_url(expected_url),
        "Missing exact conversation URL"
    );
    // Cleanup opens the exact saved URL when needed. A deleted conversation may redirect
    // home with an explicit deletion notice, allowing recovery after a missed confirmation toast.
    if action(page, "deleted", json!(expected_url)).await?["ok"] == true {
        return Ok("ui_notice");
    }
    // The composer can mount before conversation hydration/menu controls. Wait
    // for an identity-bound menu rather than mistaking that intermediate UI for failure.
    let mut opened = false;
    for attempt in 0..60 {
        if action(page, "deleted", json!(expected_url)).await?["ok"] == true {
            return Ok("ui_notice");
        }
        let Ok(state) = action(page, "deletion_state", json!(expected_url)).await else {
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue; // bounded hydration/navigation context changes
        };
        ensure!(
            state["unavailable"] != true,
            "deletion_access_unavailable: owned conversation is inaccessible; deletion is not confirmed"
        );
        if state["matching"] == true {
            let result = action(page, "open_chat_menu", json!(expected_url)).await?;
            ensure!(
                result["error"] != "ambiguous_conversation_menu",
                "delete_unavailable: ambiguous conversation toolbar"
            );
            if result["ok"] == true {
                opened = true;
                break;
            }
        }
        if attempt == 19 {
            // Older project chats may be absent from the truncated sidebar, and
            // the header's generic More menu can contain Plugins but no Delete.
            // The project landing page provides an ID-bound authoritative row.
            if let Some((project, _)) = expected_url.rsplit_once("/c/") {
                let landing = format!("{project}/project");
                if research_core::valid_project_url(&landing) {
                    page.command("Page.navigate", json!({"url":landing}))
                        .await?;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    ensure!(
        opened,
        "delete_unavailable: cannot locate the owned conversation menu after hydration"
    );
    ensure!(
        wait_cleanup_control(page, "delete_menu_item", expected_url).await,
        "delete_unavailable: owned menu has no unique Delete control"
    );
    page.watch_deletion(expected_url).await?;
    ensure!(
        wait_cleanup_control(page, "confirm_delete", expected_url).await,
        "delete_confirmation_unavailable: owned deletion dialog is missing or ambiguous"
    );
    if let Some(evidence) = wait_for_deletion(page, expected_url).await {
        return Ok(evidence);
    }
    // Some project chats redirect home without a deletion toast. Reopening the
    // exact saved URL produces ChatGPT's explicit deleted-conversation notice.
    // A redirect alone is never evidence, and a different open chat is not touched.
    let current = page.eval("location.href").await?;
    if current.as_str().is_some_and(|url| {
        research_browser::valid_chat_url(url) && !research_browser::conversation_url(url)
    }) {
        page.command("Page.navigate", json!({"url":expected_url}))
            .await?;
        if let Some(evidence) = wait_for_deletion(page, expected_url).await {
            return Ok(evidence);
        }
    }
    bail!("deletion_unknown: confirmation was clicked but deletion could not be verified")
}

async fn wait_cleanup_control(page: &mut Page, op: &str, expected_url: &str) -> bool {
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Ok(result) = action(page, op, json!(expected_url)).await
            && result["ok"] == true
        {
            return true;
        }
    }
    false
}

async fn wait_for_deletion(page: &mut Page, expected_url: &str) -> Option<&'static str> {
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        // Navigation may briefly destroy the JavaScript execution context.
        if let Ok(result) = action(page, "deleted", json!(expected_url)).await
            && result["ok"] == true
        {
            return Some("ui_notice");
        }
        if page.deletion_acknowledged().await {
            return Some("ui_response");
        }
    }
    None
}
