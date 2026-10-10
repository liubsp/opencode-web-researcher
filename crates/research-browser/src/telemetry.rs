//! Passive, independent CDP observer. No launches, navigations, account API calls, or payload logs.
use crate::{BrowserRecord, Chrome, pacing::Pacer};
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};
use tokio_tungstenite::tungstenite::Message;

pub async fn run(dir: std::path::PathBuf) {
    let pacer = Pacer::new(&dir);
    let _ = pacer.observed("observer_disconnected", None, None);
    loop {
        if dir.join("chrome.json").exists() {
            if observe(&dir, &pacer).await.is_err() {
                let _ = pacer.observed("observer_disconnected", None, None);
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        } else {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

async fn observe(dir: &Path, pacer: &Pacer) -> Result<()> {
    let record: BrowserRecord = serde_json::from_slice(&std::fs::read(dir.join("chrome.json"))?)?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    // Verify exact browser identity without starting it or extending its idle lifetime.
    Chrome::from_record(&record, http).await?;
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(&record.websocket),
    )
    .await??;
    socket.send(Message::Text(json!({"id":1,"method":"Target.setAutoAttach","params":{"autoAttach":true,"waitForDebuggerOnStart":false,"flatten":true,"filter":[{"type":"page","exclude":false},{"exclude":true}]}}).to_string().into())).await?;
    let mut sequence = 1;
    let mut network = Network::default();
    let mut pending = HashMap::<u64, String>::new();
    let mut sessions = HashMap::<String, String>::new();
    while let Some(message) = socket.next().await {
        match message? {
            Message::Text(text) => {
                let value: Value = serde_json::from_str(&text)?;
                if value["id"] == 1 {
                    anyhow::ensure!(
                        value.get("error").is_none(),
                        "Passive CDP attachment failed"
                    );
                    pacer.observed("observer_connected", None, None)?;
                }
                anyhow::ensure!(value.get("error").is_none(), "Passive CDP command failed");
                if value["method"] == "Target.attachedToTarget" {
                    sequence += 1;
                    let target = value["params"]["targetInfo"]["targetId"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Missing observer target"))?
                        .to_owned();
                    let session = value["params"]["sessionId"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Missing observer session"))?
                        .to_owned();
                    pending.insert(sequence, target.clone());
                    sessions.insert(session, target);
                    socket.send(Message::Text(json!({"id":sequence,"sessionId":value["params"]["sessionId"],"method":"Network.enable","params":{}}).to_string().into())).await?;
                }
                if let Some(target) = value["id"].as_u64().and_then(|id| pending.remove(&id)) {
                    pacer.observer_target(&target, true)?;
                }
                if value["method"] == "Target.detachedFromTarget"
                    && let Some(target) = value["params"]["sessionId"]
                        .as_str()
                        .and_then(|session| sessions.remove(session))
                {
                    pacer.observer_target(&target, false)?;
                }
                if let Some((event, status, retry)) = network.event(&value) {
                    pacer.observed(event, status, retry)?;
                }
            }
            Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await?,
            Message::Close(_) => break,
            _ => {}
        }
    }
    pacer.observed("observer_disconnected", None, None)?;
    Ok(())
}

#[derive(Default)]
struct Network {
    requests: HashSet<(String, String)>,
}
impl Network {
    fn event(&mut self, value: &Value) -> Option<(&'static str, Option<u16>, Option<u64>)> {
        let p = &value["params"];
        if value["method"] == "Target.detachedFromTarget" {
            if let Some(session) = p["sessionId"].as_str() {
                self.requests.retain(|(s, _)| s != session);
            }
            return None;
        }
        let key = (
            value["sessionId"].as_str().unwrap_or_default().to_owned(),
            p["requestId"].as_str()?.to_owned(),
        );
        match value["method"].as_str()? {
            "Network.requestWillBeSent" => {
                if !p["request"]["url"]
                    .as_str()
                    .is_some_and(crate::valid_chat_url)
                {
                    return None;
                }
                self.requests.insert(key);
                if p["type"] == "Document" {
                    return Some(("document_request", None, None));
                }
            }
            "Network.responseReceived" if self.requests.contains(&key) => {
                let status = p["response"]["status"].as_u64()? as u16;
                if status >= 400 {
                    let retry = p["response"]["headers"]
                        .as_object()
                        .and_then(|h| {
                            h.iter()
                                .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
                        })
                        .and_then(|(_, v)| retry_after(v.as_str()?));
                    return Some(("http_error", Some(status), retry));
                }
            }
            "Network.loadingFailed" if self.requests.remove(&key) => {
                return Some(("network_failure", None, None));
            }
            "Network.loadingFinished" => {
                self.requests.remove(&key);
            }
            _ => {}
        }
        None
    }
}

fn retry_after(value: &str) -> Option<u64> {
    let value = value.trim();
    value.parse().ok().or_else(|| {
        let until = httpdate::parse_http_date(value).ok()?;
        Some(
            until
                .duration_since(std::time::SystemTime::now())
                .unwrap_or_default()
                .as_secs()
                .saturating_add(1),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_after_accepts_seconds_and_http_dates() {
        assert_eq!(retry_after(" 1200 "), Some(1200));
        assert_eq!(retry_after("invalid"), None);
        let date =
            httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(1200));
        assert!((1199..=1201).contains(&retry_after(&date).unwrap()));
    }
    #[test]
    fn telemetry_is_session_bound_and_never_returns_private_network_payloads() {
        let mut network = Network::default();
        assert_eq!(network.event(&json!({"sessionId":"a","method":"Network.requestWillBeSent","params":{"requestId":"1","type":"Document","request":{"url":"https://chatgpt.com/c/private?token=secret","postData":"private prompt"}}})), Some(("document_request", None, None)));
        let response = |session| json!({"sessionId":session,"method":"Network.responseReceived","params":{"requestId":"1","response":{"status":429,"headers":{"Retry-After":"1200","Authorization":"private"}}}});
        assert_eq!(network.event(&response("b")), None);
        assert_eq!(
            network.event(&response("a")),
            Some(("http_error", Some(429), Some(1200)))
        );
        assert_eq!(network.event(&json!({"sessionId":"a","method":"Network.loadingFailed","params":{"requestId":"1","errorText":"private URL"}})), Some(("network_failure", None, None)));
        assert_eq!(network.event(&json!({"method":"Network.requestWillBeSent","params":{"requestId":"2","type":"Document","request":{"url":"https://chatgpt.com.evil.test/"}}})), None);
    }
}
