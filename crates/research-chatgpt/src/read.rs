use anyhow::{Result, ensure};
use research_browser::Page;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};

const SCROLL: &str = r#"(op => {
  const turn = document.querySelector('[data-message-author-role]');
  let root = turn?.parentElement;
  while (root && !(root.scrollHeight > root.clientHeight + 10 && /auto|scroll/.test(getComputedStyle(root).overflowY))) root = root.parentElement;
  root ||= document.scrollingElement;
  if (op === 'top') root.scrollTop = 0;
  if (op === 'next') root.scrollTop += Math.max(200, root.clientHeight * .8);
  return {top:root.scrollTop, height:root.scrollHeight, bottom:root.scrollTop + root.clientHeight >= root.scrollHeight - 5};
})"#;

/// Read only the currently rendered branch. No composer, menu, model, or Send interactions.
pub async fn capture_rendered_chat(page: &mut Page) -> Result<Value> {
    page.eval(&format!("({SCROLL})('top')")).await?;
    // Allow history loaded by scrolling to the beginning to settle.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let mut turns: Vec<Value> = vec![];
    let mut seen = HashMap::<String, usize>::new();
    let mut bottom_stable = 0;
    let mut previous = String::new();
    let mut limited = true;
    let mut missing_ids = false;
    let mut snapshot = Value::Null;
    for _ in 0..80 {
        snapshot = super::inspect(page).await?;
        ensure!(
            snapshot["login_required"] != true,
            "needs_login: sign in to read this conversation"
        );
        for turn in snapshot["turns"].as_array().into_iter().flatten() {
            let key = if let Some(id) = turn["id"].as_str() {
                id.to_owned()
            } else {
                missing_ids = true;
                format!("{}:{}", turn["role"], turn["markdown"])
            };
            if let Some(index) = seen.get(&key) {
                turns[*index] = turn.clone();
            } else {
                seen.insert(key, turns.len());
                turns.push(turn.clone());
            }
        }
        if turns
            .iter()
            .map(|t| t["markdown"].as_str().unwrap_or_default().len())
            .sum::<usize>()
            > 1_000_000
        {
            break;
        }
        let position = page.eval(&format!("({SCROLL})('position')")).await?;
        let fingerprint = format!(
            "{}:{}",
            position["height"],
            serde_json::to_string(&snapshot["turns"])?
        );
        if position["bottom"] == true && fingerprint == previous {
            bottom_stable += 1;
        } else {
            bottom_stable = 0;
        }
        if bottom_stable >= 3 {
            limited = false;
            break;
        }
        previous = fingerprint;
        page.eval(&format!("({SCROLL})('next')")).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    ensure!(
        !turns.is_empty(),
        "chat_unavailable_or_empty: no readable messages found"
    );
    let captured_at = research_core::now();
    let mut markdown = format!(
        "# {}\n\nSource: {}\nCaptured at (Unix seconds): {}\n\n",
        snapshot["title"].as_str().unwrap_or("ChatGPT conversation"),
        snapshot["url"].as_str().unwrap_or_default(),
        captured_at
    );
    for turn in &turns {
        markdown.push_str(&format!(
            "## {}\n\n{}\n\n",
            turn["role"].as_str().unwrap_or("unknown"),
            turn["markdown"].as_str().unwrap_or_default()
        ));
    }
    Ok(
        json!({"source_url":snapshot["url"],"title":snapshot["title"],"captured_at":captured_at,
        "turns":turns,"markdown":markdown,"busy":snapshot["busy"],"capture_limit_reached":limited,
        "missing_message_ids":missing_ids,"coverage":"rendered_current_branch",
        "limitations":"Only messages rendered while scrolling the current branch are captured. Alternate branches, unloaded history, attachments, and separate report panels may be missing. This is not a verified full account export."}),
    )
}
