use super::State;
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State as AppState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use research_core::{Config, Submit, now};
use serde_json::{Value, json};
use std::time::Duration;

pub fn router(state: State) -> Router {
    Router::new()
        .route("/v1/rpc", post(handle))
        .layer(DefaultBodyLimit::max(65536))
        .with_state(state)
}

async fn handle(
    AppState(state): AppState<State>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Response {
    let auth = format!("Bearer {}", state.descriptor.token);
    let host = format!("127.0.0.1:{}", state.descriptor.port);
    if headers.get("authorization").and_then(|h| h.to_str().ok()) != Some(&auth)
        || headers.get("host").and_then(|h| h.to_str().ok()) != Some(&host)
        || headers.contains_key("origin")
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"unauthorized"})),
        )
            .into_response();
    }
    match dispatch(&state, input).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing {name}"))
}

fn scoped_job(state: &State, input: &Value) -> Result<research_core::Job> {
    let store = state
        .store
        .lock()
        .map_err(|_| anyhow::anyhow!("Store lock poisoned"))?;
    let job = store.job(field(input, "id")?)?;
    ensure!(
        store.thread(&job.thread_id)?.project == field(input, "project")?,
        "project_mismatch"
    );
    Ok(job)
}

async fn dispatch(state: &State, input: Value) -> Result<Value> {
    let op = field(&input, "op")?;
    if matches!(op, "get" | "wait" | "cancel" | "read_content")
        && field(&input, "id")?.starts_with("read-")
    {
        return read_operation(state, &input).await;
    }
    match field(&input, "op")? {
        "health" => Ok(
            json!({"protocol":state.descriptor.protocol,"instance":state.descriptor.instance,"version":env!("CARGO_PKG_VERSION")}),
        ),
        "submit" => {
            let submit: Submit = serde_json::from_value(input["request"].clone())?;
            let config = Config::load(&state.dir)?;
            let job = state.store.lock().unwrap().submit(&submit, config, now())?;
            state.notify.notify_one();
            Ok(json!(job))
        }
        "read_chats" => {
            let chats = input["chats"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Missing chats array"))?;
            ensure!(
                (1..=10).contains(&chats.len()),
                "Supply 1–10 chats per read request"
            );
            let chats = chats
                .iter()
                .map(|chat| {
                    research_core::ChatReference::parse(
                        chat.as_str()
                            .ok_or_else(|| anyhow::anyhow!("Chat references must be strings"))?,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let request = state.store.lock().unwrap().submit_read(
                field(&input, "project")?,
                field(&input, "session")?,
                field(&input, "request_key")?,
                chats,
                now(),
            )?;
            state.notify.notify_one();
            Ok(crate::transcripts::imported(
                &state.store.lock().unwrap(),
                &state.dir,
                &request,
            ))
        }
        "get" | "wait" => {
            let job = scoped_job(state, &input)?;
            if input["op"] == "wait"
                && !job.terminal()
                && !matches!(job.state.as_str(), "submission_unknown" | "needs_attention")
            {
                let timeout = input["seconds"].as_u64().unwrap_or(30).clamp(1, 60);
                let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout);
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let current = scoped_job(state, &input)?;
                    if current.state != job.state
                        || current.terminal()
                        || tokio::time::Instant::now() >= deadline
                    {
                        break;
                    }
                }
            }
            let job = scoped_job(state, &input)?;
            let thread = state.store.lock().unwrap().thread(&job.thread_id)?;
            let local_transcript =
                crate::transcripts::managed(&state.store.lock().unwrap(), &state.dir, &thread);
            Ok(
                json!({"request":job,"remaining_prompts":10u32.saturating_sub(thread.prompts),"next_wait_seconds":30,"local_transcript":local_transcript}),
            )
        }
        "list" => {
            let project = if input["all_projects"] == true {
                None
            } else {
                Some(field(&input, "project")?)
            };
            let archived = input["archived"] == true;
            let threads = state
                .store
                .lock()
                .unwrap()
                .threads(project)?
                .into_iter()
                .filter(|t| {
                    if archived {
                        t.state != "active"
                    } else {
                        t.state == "active"
                    }
                })
                .collect::<Vec<_>>();
            Ok(json!(threads))
        }
        "archive" | "resume" => {
            let store = state.store.lock().unwrap();
            let mut thread = store.thread(field(&input, "id")?)?;
            ensure!(
                thread.project == field(&input, "project")?,
                "project_mismatch"
            );
            if input["op"] == "resume" {
                ensure!(
                    thread.state == "active" && thread.prompts < 10,
                    "thread_retired"
                );
                ensure!(
                    now() - thread.active_at
                        < (Config::load(&state.dir)?.inactivity_hours * 3600) as i64,
                    "thread_expired"
                );
                thread.active_at = now();
                store.save_thread(&thread)?;
            }
            let local_transcript = crate::transcripts::managed(&store, &state.dir, &thread);
            Ok(
                json!({"thread":thread,"requests":store.jobs(&thread.id)?,"local_transcript":local_transcript}),
            )
        }
        "cancel" => {
            let job = scoped_job(state, &input)?;
            let job = state.store.lock().unwrap().cancel(&job.id, now())?;
            state.notify.notify_one();
            Ok(json!(job))
        }
        "reconcile" => {
            let job = scoped_job(state, &input)?;
            let job = state.store.lock().unwrap().reconcile(&job.id)?;
            state.notify.notify_one();
            Ok(json!(job))
        }
        "retire" => {
            let store = state.store.lock().unwrap();
            let mut thread = store.thread(field(&input, "id")?)?;
            ensure!(
                thread.project == field(&input, "project")?,
                "project_mismatch"
            );
            ensure!(
                store
                    .jobs(&thread.id)?
                    .iter()
                    .all(research_core::Job::terminal),
                "thread_busy"
            );
            if thread.state == "active" {
                thread.state = "archive_pending".into();
                store.save_thread(&thread)?;
            }
            state.notify.notify_one();
            Ok(json!(thread))
        }
        "shutdown" => {
            state.shutdown.send(true)?;
            Ok(json!({"stopping":true}))
        }
        _ => anyhow::bail!("Unknown research operation"),
    }
}

async fn read_operation(state: &State, input: &Value) -> Result<Value> {
    let load = || -> Result<research_core::ReadRequest> {
        let request = state
            .store
            .lock()
            .unwrap()
            .read_request(field(input, "id")?)?;
        ensure!(
            request.project == field(input, "project")?,
            "project_mismatch"
        );
        Ok(request)
    };
    let mut request = load()?;
    match field(input, "op")? {
        "wait" if !request.terminal() => {
            let deadline = tokio::time::Instant::now()
                + Duration::from_secs(input["seconds"].as_u64().unwrap_or(60).clamp(1, 60));
            let count = request.results.len();
            let initial = request.state.clone();
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                request = load()?;
                if request.terminal()
                    || request.state != initial
                    || request.results.len() != count
                    || tokio::time::Instant::now() >= deadline
                {
                    break;
                }
            }
        }
        "cancel" if !request.terminal() => {
            let store = state.store.lock().unwrap();
            request = store.read_request(&request.id)?;
            if !request.terminal() {
                request.state = "cancelled".into();
                request.updated_at = now();
                store.save_read(&request)?;
            }
        }
        "read_content" => {
            let index = input["chat_index"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing chat_index"))?
                as usize;
            let result = request
                .results
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("Chat result not ready or index out of range"))?;
            let markdown = result["markdown"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Chat read failed: {}", result["error"]))?;
            let offset = input["offset"].as_u64().unwrap_or(0) as usize;
            let limit = input["limit"].as_u64().unwrap_or(20000).clamp(1, 64000) as usize;
            let total = markdown.chars().count();
            ensure!(offset <= total, "Offset exceeds transcript length");
            let end = (offset + limit).min(total);
            let summary =
                crate::transcripts::imported(&state.store.lock().unwrap(), &state.dir, &request);
            return Ok(
                json!({"id":request.id,"chat_index":index,"chat_id":result["chat_id"],"source_url":result["source_url"],
                "captured_at":result["captured_at"],"coverage":result["coverage"],"limitations":result["limitations"],
                "offset":offset,"total_chars":total,"next_offset":if end < total {Some(end)} else {None},
                "markdown":markdown.chars().skip(offset).take(limit).collect::<String>(),
                "local_transcript":summary["results"][index]["local_transcript"]}),
            );
        }
        _ => {}
    }
    Ok(crate::transcripts::imported(
        &state.store.lock().unwrap(),
        &state.dir,
        &request,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use research_core::{PROTOCOL, ServiceDescriptor};
    use research_store::Store;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn local_api_auth_scope_idempotency_and_waiting() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let descriptor = ServiceDescriptor {
            protocol: PROTOCOL,
            port: listener.local_addr()?.port(),
            token: "test-secret".into(),
            instance: "test-instance".into(),
            pid: 0,
        };
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let state = State {
            dir: dir.path().into(),
            store: Arc::new(Mutex::new(Store::open(&dir.path().join("db"))?)),
            descriptor: descriptor.clone(),
            notify: Arc::new(tokio::sync::Notify::new()),
            shutdown,
        };
        let mut imported = state.store.lock().unwrap().submit_read(
            "a",
            "s",
            "read-key",
            vec![research_core::ChatReference::parse(
                "00000000-0000-4000-8000-000000000001",
            )?],
            1,
        )?;
        imported.state = "completed".into();
        imported.results.push(json!({"markdown":"Aéñ👋Z","source_url":"https://chatgpt.com/c/00000000-0000-4000-8000-000000000001","turns":[]}));
        state.store.lock().unwrap().save_read(&imported)?;
        let server = tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });
        let url = format!("http://127.0.0.1:{}/v1/rpc", descriptor.port);
        let client = reqwest::Client::builder().no_proxy().build()?;
        let denied = client
            .post(&url)
            .json(&json!({"op":"health"}))
            .send()
            .await?;
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let origin = client
            .post(&url)
            .bearer_auth(&descriptor.token)
            .header("Origin", "https://example.org")
            .json(&json!({"op":"health"}))
            .send()
            .await?;
        assert_eq!(origin.status(), StatusCode::UNAUTHORIZED);
        let health = crate::client::rpc(&descriptor, json!({"op":"health"}), 5).await?;
        assert_eq!(health["instance"], "test-instance");
        let page = crate::client::rpc(&descriptor, json!({"op":"read_content","project":"a","id":imported.id,"chat_index":0,"offset":1,"limit":3}), 5).await?;
        assert_eq!(page["markdown"], "éñ👋");
        assert_eq!(page["next_offset"], 4);
        assert_eq!(page["total_chars"], 5);
        let imported_path = page["local_transcript"]["markdown"]["path"]
            .as_str()
            .unwrap();
        assert_eq!(std::fs::read_to_string(imported_path)?, "Aéñ👋Z");
        assert!(
            crate::client::rpc(
                &descriptor,
                json!({"op":"read_content","project":"b","id":imported.id,"chat_index":0}),
                5
            )
            .await
            .is_err()
        );
        assert!(
            crate::client::rpc(
                &descriptor,
                json!({"op":"get","project":"b","id":imported.id}),
                5
            )
            .await
            .is_err()
        );
        let summary = crate::client::rpc(
            &descriptor,
            json!({"op":"wait","project":"a","id":imported.id}),
            5,
        )
        .await?;
        assert_eq!(summary["kind"], "read_chats");
        assert!(summary["results"][0].get("markdown").is_none());
        let request = json!({"op":"submit","request":{"project":"a","session":"s","key":"first",
            "prompt":"check official docs pls","deep_research":false}});
        let one = crate::client::rpc(&descriptor, request.clone(), 5).await?;
        let duplicate = crate::client::rpc(&descriptor, request, 5).await?;
        assert_eq!(one["id"], duplicate["id"]);
        assert!(
            crate::client::rpc(
                &descriptor,
                json!({"op":"get","project":"b","id":one["id"]}),
                5
            )
            .await
            .is_err()
        );
        let waited = crate::client::rpc(
            &descriptor,
            json!({"op":"wait","project":"a","id":one["id"],"seconds":1}),
            5,
        )
        .await?;
        assert_eq!(waited["request"]["state"], "queued");
        assert_eq!(waited["remaining_prompts"], 9);
        let thread_path = waited["local_transcript"]["markdown"]["path"]
            .as_str()
            .unwrap();
        assert!(std::fs::read_to_string(thread_path)?.contains("check official docs pls"));
        crate::client::rpc(
            &descriptor,
            json!({"op":"cancel","project":"a","id":one["id"]}),
            5,
        )
        .await?;
        let cancelled = crate::client::rpc(
            &descriptor,
            json!({"op":"get","project":"a","id":one["id"]}),
            5,
        )
        .await?;
        assert_eq!(cancelled["request"]["state"], "cancelled");
        assert_eq!(cancelled["remaining_prompts"], 10);
        server.abort();
        Ok(())
    }
}
