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
            Ok(
                json!({"request":job,"remaining_prompts":10u32.saturating_sub(thread.prompts),"next_wait_seconds":30}),
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
            Ok(json!({"thread":thread,"requests":store.jobs(&thread.id)?}))
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
