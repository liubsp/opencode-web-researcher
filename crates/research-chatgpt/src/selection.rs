use crate::action;
use anyhow::{Result, bail, ensure};
use research_browser::Page;
use serde_json::{Value, json};
use std::time::Duration;

fn normalized(label: &str) -> String {
    label
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

async fn key(page: &mut Page, key: &str, code: u32) -> Result<()> {
    for kind in ["keyDown", "keyUp"] {
        page.command(
            "Input.dispatchKeyEvent",
            json!({"type":kind,"key":key,"code":key,"windowsVirtualKeyCode":code}),
        )
        .await?;
    }
    tokio::time::sleep(Duration::from_millis(250)).await;
    Ok(())
}

async fn slider_step(page: &mut Page, key_name: &str, code: u32, index: u64) -> Result<Value> {
    key(page, key_name, code).await?;
    let mut status = action(page, "reasoning_status", Value::Null).await?;
    // The live picker may apply its React state update after the key event
    // returns. A stale immediate read must not be mistaken for a locked tier.
    for _ in 0..10 {
        if status["index"].as_u64() != Some(index) || status["ok"] != true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        status = action(page, "reasoning_status", Value::Null).await?;
    }
    Ok(status)
}

pub async fn dismiss(page: &mut Page) -> Result<()> {
    key(page, "Escape", 27).await
}

pub async fn reasoning(page: &mut Page, preferences: &[String]) -> Result<Value> {
    let result = action(page, "open_reasoning", Value::Null).await?;
    ensure!(result["ok"] == true, "reasoning_unavailable: {result}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let current = action(page, "reasoning_status", Value::Null).await?;
    if current["ok"] != true {
        // Older UI exposes menu items rather than a reasoning-effort slider.
        let selected = action(page, "select", json!(preferences)).await?;
        ensure!(selected["ok"] == true, "reasoning_unavailable: {selected}");
        dismiss(page).await?;
        return Ok(selected);
    }
    if normalized(current["selected"].as_str().unwrap_or_default()) == normalized(&preferences[0]) {
        dismiss(page).await?;
        return Ok(current);
    }
    let maximum = current["max"].as_u64().unwrap_or(0);
    ensure!(
        maximum > 0 && maximum <= 16,
        "reasoning_unavailable: unexpected slider range"
    );
    action(page, "reasoning_focus", Value::Null).await?;
    let mut index = current["index"].as_u64().unwrap_or(0);
    while index > 0 {
        let next = slider_step(page, "ArrowLeft", 37, index).await?;
        let next_index = next["index"].as_u64().unwrap_or(index);
        ensure!(
            next_index < index,
            "reasoning_unavailable: slider did not move left"
        );
        index = next_index;
    }
    let mut available = Vec::new();
    loop {
        let value = action(page, "reasoning_status", Value::Null).await?;
        available.push(value.clone());
        if index == maximum {
            break;
        }
        let next = slider_step(page, "ArrowRight", 39, index).await?;
        let next_index = next["index"].as_u64().unwrap_or(index);
        if next_index <= index {
            break;
        } // Subscription-locked endpoint.
        index = next_index;
    }
    let selected = preferences
        .iter()
        .find_map(|preference| {
            available.iter().find(|value| {
                normalized(value["selected"].as_str().unwrap_or_default()) == normalized(preference)
            })
        })
        .cloned();
    let Some(selected) = selected else {
        dismiss(page).await?;
        bail!("reasoning_unavailable: available choices {available:?}");
    };
    let wanted = selected["index"].as_u64().unwrap();
    while index > wanted {
        let next = slider_step(page, "ArrowLeft", 37, index).await?;
        let next_index = next["index"].as_u64().unwrap_or(index);
        ensure!(
            next_index < index,
            "reasoning_unavailable: selection did not move"
        );
        index = next_index;
    }
    let actual = action(page, "reasoning_status", Value::Null).await?;
    ensure!(
        actual["selected"] == selected["selected"],
        "reasoning selection verification failed"
    );
    dismiss(page).await?;
    Ok(actual)
}

pub async fn model(page: &mut Page, model: &str) -> Result<()> {
    ensure!(
        action(page, "open_model", Value::Null).await?["ok"] == true,
        "model_unavailable: cannot open model picker"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut selected = action(page, "select", json!([model])).await?;
    if selected["ok"] != true {
        action(page, "expand_model", Value::Null).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        selected = action(page, "select", json!([model])).await?;
    }
    ensure!(selected["ok"] == true, "model_unavailable: {selected}");
    dismiss(page).await
}
