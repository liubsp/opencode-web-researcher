use anyhow::{Result, bail};
use research_core::{SubmissionAnchor, canonical_prompt};
use serde_json::Value;

pub fn submission_anchor(snapshot: &Value) -> Option<SubmissionAnchor> {
    let turns = snapshot["turns"].as_array()?;
    let ids = turns
        .iter()
        .map(|turn| turn["id"].as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    Some(SubmissionAnchor {
        last_turn_id: ids.last().cloned(),
        prior_turn_ids: ids,
        user_turn_id: None,
    })
}

pub fn submitted_user(
    snapshot: &Value,
    baseline: usize,
    prompt: &str,
    anchor: Option<&SubmissionAnchor>,
) -> Option<usize> {
    let turns = snapshot["turns"].as_array()?;
    let prompt = canonical_prompt(prompt);
    let matches = |turn: &Value| {
        turn["role"] == "user"
            && turn["text"]
                .as_str()
                .is_some_and(|text| canonical_prompt(text) == prompt)
    };
    if let Some(anchor) = anchor {
        if let Some(id) = &anchor.user_turn_id {
            return turns
                .iter()
                .position(|turn| turn["id"].as_str() == Some(id) && matches(turn));
        }
        let start = match &anchor.last_turn_id {
            Some(id) => {
                turns
                    .iter()
                    .position(|turn| turn["id"].as_str() == Some(id))?
                    + 1
            }
            None if baseline == 0 => 0,
            None => return None,
        };
        let candidates = turns
            .iter()
            .enumerate()
            .skip(start)
            .filter(|(_, turn)| {
                matches(turn)
                    && turn["id"]
                        .as_str()
                        .is_some_and(|id| !anchor.prior_turn_ids.iter().any(|prior| prior == id))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        return (candidates.len() == 1).then(|| candidates[0]);
    }
    turns
        .iter()
        .enumerate()
        .skip(baseline)
        .find(|(_, turn)| matches(turn))
        .map(|(index, _)| index)
}

/// Match only the newly submitted user turn and its following answer. Earlier stable answers
/// and later manual conversation edits must never satisfy a pending request.
pub fn response_after<'a>(
    snapshot: &'a Value,
    baseline: usize,
    prompt: &str,
) -> Result<Option<&'a Value>> {
    response_after_submission(snapshot, baseline, prompt, None)
}

pub fn response_after_submission<'a>(
    snapshot: &'a Value,
    baseline: usize,
    prompt: &str,
    anchor: Option<&SubmissionAnchor>,
) -> Result<Option<&'a Value>> {
    let Some(turns) = snapshot["turns"].as_array() else {
        return Ok(None);
    };
    let Some(index) = submitted_user(snapshot, baseline, prompt, anchor) else {
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
    submitted_user(snapshot, baseline, prompt, None).is_some()
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

    #[test]
    fn message_anchors_survive_shrunk_history_without_accepting_an_old_answer() {
        let before = json!({"turns":[{"id":"old-user","role":"user","text":"same"},{"id":"old-answer","role":"assistant"}]});
        let mut anchor = submission_anchor(&before).unwrap();
        assert!(submitted_user(&before, 2, "same", Some(&anchor)).is_none());
        let after = json!({"turns":[{"id":"old-answer","role":"assistant"},{"id":"new-user","role":"user","text":"same"},{"id":"new-answer","role":"assistant"}]});
        assert_eq!(submitted_user(&after, 2, "same", Some(&anchor)), Some(1));
        anchor.user_turn_id = Some("new-user".into());
        let virtualized = json!({"turns":[{"id":"new-user","role":"user","text":"same"},{"id":"new-answer","role":"assistant"}]});
        assert_eq!(
            response_after_submission(&virtualized, 2, "same", Some(&anchor))
                .unwrap()
                .unwrap()["id"],
            "new-answer"
        );
        anchor.user_turn_id = None;
        assert!(submitted_user(&virtualized, 2, "same", Some(&anchor)).is_none()); // Missing anchor remains ambiguous.
    }
}
