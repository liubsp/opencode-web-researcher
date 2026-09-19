use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChatReference {
    pub id: String,
    pub url: String,
}

impl ChatReference {
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if let Ok(id) = uuid::Uuid::parse_str(input) {
            let id = id.to_string();
            return Ok(Self {
                url: format!("https://chatgpt.com/c/{id}"),
                id,
            });
        }
        let url = url::Url::parse(input)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str() == Some("chatgpt.com")
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none(),
            "Expected a ChatGPT conversation ID or HTTPS chatgpt.com conversation URL"
        );
        let parts: Vec<_> = url.path().split('/').collect();
        ensure!(
            (parts.len() == 3 && parts[1] == "c")
                || (parts.len() == 5
                    && parts[1] == "g"
                    && parts[2].starts_with("g-")
                    && parts[3] == "c"),
            "Expected a conversation URL, not a project or share link"
        );
        let id = uuid::Uuid::parse_str(parts.last().unwrap())?.to_string();
        let mut url = url;
        url.set_query(None);
        url.set_fragment(None);
        Ok(Self {
            id,
            url: url.to_string(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadRequest {
    pub id: String,
    pub project: String,
    pub session: String,
    pub key: String,
    pub chats: Vec<ChatReference>,
    pub state: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub results: Vec<Value>,
}

impl ReadRequest {
    pub fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "completed" | "cancelled")
    }

    pub fn summary(&self) -> Value {
        serde_json::json!({"id":self.id,"kind":"read_chats","state":self.state,
            "created_at":self.created_at,"chat_count":self.chats.len(),
            "results":self.results.iter().enumerate().map(|(index,result)| {
                let mut entry = result.clone();
                if let Some(object) = entry.as_object_mut() {
                    let markdown = object.remove("markdown").unwrap_or(Value::Null);
                    object.insert("total_chars".into(), serde_json::json!(markdown.as_str().unwrap_or_default().chars().count()));
                    let turns = object.remove("turns").unwrap_or(Value::Null);
                    object.insert("turn_count".into(), serde_json::json!(turns.as_array().map_or(0, Vec::len)));
                    object.insert("chat_index".into(), serde_json::json!(index));
                }
                entry
            }).collect::<Vec<_>>()})
    }
}
