use anyhow::{Result, bail};
use serde_json::Value;

/// Match only the newly submitted user turn and its following answer. Earlier stable answers
/// and later manual conversation edits must never satisfy a pending request.
pub fn response_after<'a>(
    snapshot: &'a Value,
    baseline: usize,
    prompt: &str,
) -> Result<Option<&'a Value>> {
    let Some(turns) = snapshot["turns"].as_array() else {
        return Ok(None);
    };
    let Some((index, _)) = turns.iter().enumerate().skip(baseline).find(|(_, turn)| {
        turn["role"] == "user"
            && turn["text"]
                .as_str()
                .is_some_and(|text| text.trim() == prompt.trim())
    }) else {
        return Ok(None);
    };
    if turns
        .iter()
        .skip(index + 1)
        .any(|turn| turn["role"] == "user")
    {
        bail!("conversation_changed: another user message appeared after this submission");
    }
    Ok(turns
        .iter()
        .skip(index + 1)
        .rev()
        .find(|turn| turn["role"] == "assistant"))
}

pub fn user_turn_confirmed(snapshot: &Value, baseline: usize, prompt: &str) -> bool {
    snapshot["turns"].as_array().is_some_and(|turns| {
        turns.iter().skip(baseline).any(|turn| {
            turn["role"] == "user"
                && turn["text"]
                    .as_str()
                    .is_some_and(|text| text.trim() == prompt.trim())
        })
    })
}

pub fn completion_candidate(snapshot: &Value, response: &Value) -> bool {
    snapshot["busy"] == false
        && response["complete"] == true
        && response["markdown"]
            .as_str()
            .is_some_and(|text| !text.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prior_answer_and_thinking_pause_are_not_completion() {
        let old = json!({"turns":[{"role":"user","text":"question"},{"role":"assistant","markdown":"old","complete":true}],"busy":false});
        assert!(response_after(&old, 2, "question").unwrap().is_none());
        let mut active = old.clone();
        active["turns"].as_array_mut().unwrap().extend([
            json!({"role":"user","text":"question"}),
            json!({"role":"assistant","markdown":"still thinking","complete":false}),
        ]);
        let response = response_after(&active, 2, "question").unwrap().unwrap();
        assert!(!completion_candidate(&active, response));
        active["turns"][3]["complete"] = json!(true);
        active["busy"] = json!(true);
        assert!(!completion_candidate(&active, &active["turns"][3]));
        active["busy"] = json!(false);
        assert!(completion_candidate(&active, &active["turns"][3]));
    }

    #[test]
    fn manual_followup_cannot_be_misattributed() {
        let snapshot = json!({"turns":[{"role":"user","text":"ours"},{"role":"assistant","markdown":"first"},
            {"role":"user","text":"manual"},{"role":"assistant","markdown":"not ours"}]});
        assert!(response_after(&snapshot, 0, "ours").is_err());
        assert!(!user_turn_confirmed(&snapshot, 4, "ours"));
    }
}
