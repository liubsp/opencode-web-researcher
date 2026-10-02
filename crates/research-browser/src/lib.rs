use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use futures_util::{SinkExt, StreamExt};
use research_core::Config;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

mod deletion;
mod launch;

pub struct Page {
    pub id: String,
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    sequence: u64,
    activity: Option<PathBuf>,
    deletion: Option<deletion::Watch>,
}

impl Page {
    pub async fn connect(id: String, websocket: &str) -> Result<Self> {
        let url = url::Url::parse(websocket)?;
        ensure!(
            url.scheme() == "ws" && url.host_str() == Some("127.0.0.1"),
            "CDP endpoint must be loopback"
        );
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async(websocket),
        )
        .await??;
        Ok(Self {
            id,
            socket,
            sequence: 0,
            activity: None,
            deletion: None,
        })
    }

    pub async fn command(&mut self, method: &str, params: Value) -> Result<Value> {
        if let Some(path) = &self.activity {
            std::fs::write(path, research_core::now().to_string())?;
        }
        self.sequence += 1;
        let id = self.sequence;
        self.socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .await?;
        tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(message) = self.socket.next().await {
                match message? {
                    Message::Text(text) => {
                        let value: Value = serde_json::from_str(&text)?;
                        if let Some(watch) = &mut self.deletion {
                            watch.event(&value);
                        }
                        if value["id"] == id {
                            if let Some(error) = value.get("error") {
                                bail!("CDP {method}: {error}");
                            }
                            return Ok(value["result"].clone());
                        }
                    }
                    Message::Ping(bytes) => self.socket.send(Message::Pong(bytes)).await?,
                    Message::Close(_) => bail!("Chrome target disconnected"),
                    _ => {}
                }
            }
            bail!("Chrome target disconnected")
        })
        .await
        .context("CDP command timed out")?
    }

    pub async fn eval(&mut self, expression: &str) -> Result<Value> {
        let value = self
            .command(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
            )
            .await?;
        if value.get("exceptionDetails").is_some() {
            bail!("Browser script failed: {}", value["exceptionDetails"]);
        }
        Ok(value["result"]["value"].clone())
    }

    pub async fn watch_deletion(&mut self, expected: &str) -> Result<()> {
        ensure!(conversation_url(expected), "Invalid deletion target");
        self.command("Network.enable", json!({})).await?;
        self.deletion = Some(deletion::Watch::new(expected));
        Ok(())
    }

    pub async fn deletion_acknowledged(&mut self) -> bool {
        let ready = self
            .deletion
            .as_ref()
            .map_or_else(Vec::new, deletion::Watch::ready);
        for (id, status) in ready {
            if status == 204 {
                return true;
            }
            let body = match self
                .command("Network.getResponseBody", json!({"requestId":id}))
                .await
            {
                Ok(value) if value["base64Encoded"] != true => value["body"]
                    .as_str()
                    .and_then(|body| serde_json::from_str::<Value>(body).ok())
                    .unwrap_or(Value::Null),
                _ => continue,
            };
            let watch = self.deletion.as_mut().unwrap();
            let acknowledged = watch.acknowledged(status, &body);
            watch.consume(&id);
            if acknowledged {
                return true;
            }
        }
        false
    }
}

pub struct Chrome {
    pub port: u16,
    http: reqwest::Client,
    websocket: String,
    activity: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct BrowserRecord {
    port: u16,
    websocket: String,
    profile: String,
}

fn launch_arguments(profile: &str, port: u16, marker: &str) -> Vec<String> {
    vec![
        format!("--user-data-dir={profile}"),
        format!("--remote-debugging-port={port}"),
        "--remote-debugging-address=127.0.0.1".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--start-minimized".into(),
        "--window-position=-2000,-2000".into(),
        marker.into(),
    ]
}

pub fn chrome_path(config: &Config) -> Result<PathBuf> {
    if let Some(path) = &config.chrome_path {
        ensure!(
            path.is_file(),
            "Configured Chrome executable does not exist"
        );
        return Ok(path.clone());
    }
    let mut paths = vec![PathBuf::from(
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    )];
    for root in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(root) {
            paths.push(PathBuf::from(root).join("Google/Chrome/Application/chrome.exe"));
        }
    }
    paths
        .into_iter()
        .find(|p| p.is_file())
        .context("Google Chrome not found; set chrome_path in config.json")
}

impl Chrome {
    /// Check the saved owned browser without launching it or refreshing its activity timer.
    pub async fn close_if_idle(dir: &Path, config: &Config) -> Result<bool> {
        if !config.chrome_auto_close {
            return Ok(false);
        }
        let activity = dir.join("chrome-activity");
        let idle = || -> bool {
            std::fs::metadata(&activity)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|elapsed| {
                    elapsed >= Duration::from_secs(config.chrome_idle_timeout_minutes * 60)
                })
        };
        if !activity.exists() {
            // Existing installations get a full idle interval after upgrade.
            std::fs::write(&activity, research_core::now().to_string())?;
            return Ok(false);
        }
        if !idle() {
            return Ok(false);
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("chrome.lock"))?;
        if lock.try_lock_exclusive().is_err() {
            return Ok(false);
        }
        let record_path = dir.join("chrome.json");
        let bytes = match std::fs::read(&record_path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        let record: BrowserRecord = serde_json::from_slice(&bytes)?;
        let profile = std::fs::canonicalize(dir.join("chrome-profile"))?;
        ensure!(
            record.profile == profile.to_string_lossy().trim_start_matches(r"\\?\"),
            "Recorded Chrome profile does not match"
        );
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()?;
        let Ok(chrome) = Self::from_record(&record, http.clone()).await else {
            return Ok(false);
        };
        if !idle() {
            return Ok(false);
        }
        let mut browser = Page::connect("browser".into(), &chrome.websocket).await?;
        // Browser.close can drop CDP before returning its reply. Confirm endpoint shutdown below.
        browser
            .socket
            .send(Message::Text(
                json!({"id":1,"method":"Browser.close","params":{}})
                    .to_string()
                    .into(),
            ))
            .await?;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if http
                .get(format!("http://127.0.0.1:{}/json/version", chrome.port))
                .send()
                .await
                .is_err()
            {
                std::fs::remove_file(record_path)?;
                return Ok(true);
            }
        }
        bail!("Idle Chrome did not finish shutting down")
    }

    pub async fn ensure(dir: &Path, config: &Config) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("chrome.lock"))?;
        let mut locked = false;
        for _ in 0..120 {
            if lock.try_lock_exclusive().is_ok() {
                locked = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        ensure!(locked, "Timed out waiting for Chrome startup lock");
        let activity = dir.join("chrome-activity");
        std::fs::write(&activity, research_core::now().to_string())?;
        let profile = dir.join("chrome-profile");
        std::fs::create_dir_all(&profile)?;
        let profile = std::fs::canonicalize(profile)?;
        // Chrome accepts ordinary Windows paths more reliably than verbatim paths.
        let profile_arg = profile
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_owned();
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()?;
        let record_path = dir.join("chrome.json");
        if let Ok(bytes) = std::fs::read(&record_path)
            && let Ok(record) = serde_json::from_slice::<BrowserRecord>(&bytes)
        {
            ensure!(
                record.profile == profile_arg,
                "Recorded Chrome profile does not match"
            );
            if let Ok(mut chrome) = Self::from_record(&record, http.clone()).await {
                chrome.activity = Some(activity.clone());
                chrome.minimize().await?;
                return Ok(chrome);
            }
        }
        // Explicit nonzero port, as in ask-bridge. Port 0 itself changes Chrome's automation state.
        let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = reservation.local_addr()?.port();
        let marker_path = dir.join(format!("bootstrap-{}.html", uuid::Uuid::new_v4()));
        std::fs::write(
            &marker_path,
            "<!doctype html><title>Web research</title><p>Research browser is ready.</p>",
        )?;
        let marker_path = std::fs::canonicalize(&marker_path)?;
        let marker_path = PathBuf::from(
            marker_path
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_owned(),
        );
        let marker = url::Url::from_file_path(&marker_path)
            .map_err(|_| anyhow::anyhow!("Invalid bootstrap path"))?
            .to_string();
        drop(reservation);
        let mut launcher = launch::background(
            &chrome_path(config)?,
            &launch_arguments(&profile_arg, port, &marker),
        )?;
        let chrome = Self {
            port,
            http: http.clone(),
            websocket: String::new(),
            activity: Some(activity.clone()),
        };
        for _ in 0..60 {
            launcher.check()?;
            tokio::time::sleep(Duration::from_millis(500)).await;
            // Prove this listener contains the unpredictable launch marker before recording ownership.
            // This avoids adopting an unrelated Chrome if another process wins the port allocation race.
            if chrome
                .targets()
                .await
                .is_ok_and(|targets| targets.iter().any(|target| target["url"] == marker))
            {
                let version: Value = http
                    .get(format!("http://127.0.0.1:{port}/json/version"))
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                let record = BrowserRecord {
                    port,
                    profile: profile_arg.clone(),
                    websocket: version["webSocketDebuggerUrl"]
                        .as_str()
                        .context("Missing browser endpoint")?
                        .into(),
                };
                let mut chrome = Self::from_record(&record, http.clone()).await?;
                chrome.activity = Some(activity.clone());
                chrome.minimize().await?;
                let tmp = dir.join("chrome.json.tmp");
                std::fs::write(&tmp, serde_json::to_vec(&record)?)?;
                if record_path.exists() {
                    std::fs::remove_file(&record_path)?;
                }
                std::fs::rename(tmp, &record_path)?;
                // The loaded page stays valid; the one-use ownership marker need not remain on disk.
                let _ = std::fs::remove_file(&marker_path);
                return Ok(chrome);
            }
        }
        bail!("Chrome did not expose its research-profile endpoint; check for a locked profile")
    }

    async fn from_record(record: &BrowserRecord, http: reqwest::Client) -> Result<Self> {
        let endpoint = url::Url::parse(&record.websocket)?;
        ensure!(
            endpoint.scheme() == "ws"
                && endpoint.host_str() == Some("127.0.0.1")
                && endpoint.port() == Some(record.port)
                && endpoint.path().starts_with("/devtools/browser/"),
            "Invalid saved browser identity"
        );
        let version: Value = http
            .get(format!("http://127.0.0.1:{}/json/version", record.port))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            version["webSocketDebuggerUrl"] == record.websocket,
            "Browser identity changed; refusing recycled endpoint"
        );
        let mut page = Page::connect("browser".into(), &record.websocket).await?;
        page.command("Browser.getVersion", json!({})).await?;
        Ok(Self {
            port: record.port,
            http,
            websocket: record.websocket.clone(),
            activity: None,
        })
    }

    pub async fn targets(&self) -> Result<Vec<Value>> {
        if let Some(path) = &self.activity {
            std::fs::write(path, research_core::now().to_string())?;
        }
        Ok(self
            .http
            .get(format!("http://127.0.0.1:{}/json/list", self.port))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn browser_page(&self) -> Result<Page> {
        let mut page = Page::connect("browser".into(), &self.websocket).await?;
        page.activity = self.activity.clone();
        Ok(page)
    }

    pub async fn open(&self, url: &str) -> Result<Page> {
        ensure!(
            url == "about:blank" || valid_chat_url(url),
            "Only ChatGPT URLs are supported"
        );
        let mut browser = self.browser_page().await?;
        let created = browser
            .command("Target.createTarget", json!({"url":url,"background":true}))
            .await?;
        for _ in 0..20 {
            if let Some(target) = self
                .targets()
                .await?
                .into_iter()
                .find(|t| t["id"] == created["targetId"])
            {
                return self.attach(&target).await;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        bail!("Background tab did not become available")
    }

    pub async fn minimize(&self) -> Result<()> {
        let mut browser = self.browser_page().await?;
        let mut windows = std::collections::HashSet::new();
        for target in self
            .targets()
            .await?
            .into_iter()
            .filter(|t| t["type"] == "page")
        {
            let window = match browser
                .command(
                    "Browser.getWindowForTarget",
                    json!({"targetId":target["id"]}),
                )
                .await
            {
                Ok(window) => window,
                Err(error)
                    if error.to_string().contains("No target with given id")
                        && !self
                            .targets()
                            .await?
                            .iter()
                            .any(|current| current["id"] == target["id"]) =>
                {
                    // A tab can finish closing between enumeration and its window lookup.
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let Some(id) = window["windowId"].as_i64()
                && windows.insert(id)
                && window["bounds"]["windowState"] != "minimized"
            {
                browser
                    .command(
                        "Browser.setWindowBounds",
                        json!({"windowId":id,"bounds":{"windowState":"minimized"}}),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn window_state(&self, target: &str) -> Result<String> {
        let mut browser = self.browser_page().await?;
        let window = browser
            .command("Browser.getWindowForTarget", json!({"targetId":target}))
            .await?;
        Ok(window["bounds"]["windowState"]
            .as_str()
            .unwrap_or("unknown")
            .into())
    }

    /// Explicit human login is the only operation allowed to restore/activate a research window.
    pub async fn open_interactive(&self, url: &str) -> Result<Page> {
        let mut page = self.open(url).await?;
        let mut browser = self.browser_page().await?;
        let window = browser
            .command("Browser.getWindowForTarget", json!({"targetId":page.id}))
            .await?;
        browser
            .command(
                "Browser.setWindowBounds",
                json!({"windowId":window["windowId"],"bounds":{"windowState":"normal"}}),
            )
            .await?;
        browser.command("Browser.setWindowBounds", json!({"windowId":window["windowId"],"bounds":{"left":100,"top":100,"width":1440,"height":1000}})).await?;
        page.command("Page.bringToFront", json!({})).await?;
        Ok(page)
    }

    async fn attach(&self, target: &Value) -> Result<Page> {
        let mut page = Page::connect(
            target["id"].as_str().context("Missing target ID")?.into(),
            target["webSocketDebuggerUrl"]
                .as_str()
                .context("Missing target endpoint")?,
        )
        .await?;
        page.activity = self.activity.clone();
        // Keep renderer animations/stream updates alive in a minimized window. This is renderer-only
        // focus emulation, not Page.bringToFront or OS window activation.
        page.command(
            "Emulation.setFocusEmulationEnabled",
            json!({"enabled":true}),
        )
        .await?;
        Ok(page)
    }

    pub async fn reconnect(&self, target: Option<&str>, url: Option<&str>) -> Result<Page> {
        for entry in self.targets().await? {
            if target == entry["id"].as_str() && entry["type"] == "page" {
                let current = entry["url"].as_str().unwrap_or_default();
                ensure!(
                    valid_chat_url(current),
                    "Owned tab was navigated away from ChatGPT"
                );
                if let Some(expected) = url {
                    ensure!(
                        same_conversation_url(current, expected)
                            || current.split('?').next() == Some(expected),
                        "Owned conversation changed"
                    );
                }
                return self.attach(&entry).await;
            }
        }
        self.open(url.unwrap_or("https://chatgpt.com/")).await
    }

    pub async fn close(&self, target: &str) -> Result<()> {
        if let Some(path) = &self.activity {
            std::fs::write(path, research_core::now().to_string())?;
        }
        ensure!(
            !target.is_empty()
                && target
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "Invalid target ID"
        );
        self.http
            .get(format!(
                "http://127.0.0.1:{}/json/close/{target}",
                self.port
            ))
            .send()
            .await?
            .error_for_status()?;
        // The close endpoint acknowledges intent before macOS removes the target from /json/list.
        // Confirm tab closure before reusing the browser; this is not evidence of chat deletion.
        tokio::time::timeout(Duration::from_secs(5), async {
            for _ in 0..40 {
                if !self
                    .targets()
                    .await?
                    .iter()
                    .any(|entry| entry["id"] == target)
                {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            bail!("Chrome tab did not finish closing")
        })
        .await
        .context("Chrome tab close timed out")?
    }
}

pub fn valid_chat_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("chatgpt.com")
            && u.port().is_none()
            && u.username().is_empty()
            && u.password().is_none()
    })
}

pub fn conversation_url(value: &str) -> bool {
    valid_chat_url(value)
        && url::Url::parse(value).is_ok_and(|u| {
            let p: Vec<_> = u.path().split('/').collect();
            (p.len() == 3 && p[1] == "c" && !p[2].is_empty())
                || (p.len() == 5
                    && p[1] == "g"
                    && p[2].starts_with("g-p-")
                    && p[3] == "c"
                    && !p[4].is_empty())
        })
}

fn project_key(segment: &str) -> &str {
    let Some(id) = segment.strip_prefix("g-p-") else {
        return segment;
    };
    if id.len() >= 32
        && id.as_bytes()[..32].iter().all(u8::is_ascii_hexdigit)
        && (id.len() == 32 || id.as_bytes()[32] == b'-')
    {
        &segment[..36]
    } else {
        segment
    }
}

pub fn same_project_url(a: &str, b: &str) -> bool {
    if !valid_chat_url(a) || !valid_chat_url(b) {
        return false;
    }
    let key = |value: &str| {
        let url = url::Url::parse(value).ok()?;
        let parts: Vec<_> = url.path().split('/').collect();
        (parts.len() >= 4 && parts[1] == "g" && parts[2].starts_with("g-p-"))
            .then(|| project_key(parts[2]).to_owned())
    };
    key(a).is_some_and(|project| Some(project) == key(b))
}

pub fn same_conversation_url(a: &str, b: &str) -> bool {
    if !conversation_url(a) || !conversation_url(b) {
        return false;
    }
    let key = |value: &str| {
        let url = url::Url::parse(value).ok()?;
        let parts: Vec<_> = url.path().split('/').collect();
        Some(if parts.len() == 3 {
            (None, parts[2].to_owned())
        } else {
            (Some(project_key(parts[2]).to_owned()), parts[4].to_owned())
        })
    };
    key(a) == key(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn scripted_http(
        replies: Vec<(&'static str, Value)>,
    ) -> Result<(u16, tokio::task::JoinHandle<()>)> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let task = tokio::spawn(async move {
            for (path, body) in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut headers = Vec::new();
                let mut buffer = [0; 4096];
                while !headers.windows(4).any(|part| part == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).await.unwrap();
                    assert!(count > 0 && headers.len() < 16384);
                    headers.extend_from_slice(&buffer[..count]);
                }
                assert!(headers.starts_with(format!("GET {path} ").as_bytes()));
                let body = body.to_string();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        });
        Ok((port, task))
    }

    #[tokio::test]
    async fn tab_close_waits_for_the_exact_target_to_disappear() -> Result<()> {
        let (port, server) = scripted_http(vec![
            ("/json/close/one", json!("Target is closing")),
            ("/json/list", json!([{"id":"one"},{"id":"unrelated"}])),
            ("/json/list", json!([{"id":"unrelated"}])),
        ])
        .await?;
        let chrome = Chrome {
            port,
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(2))
                .build()?,
            websocket: String::new(),
            activity: None,
        };
        chrome.close("one").await?;
        assert!(chrome.close("").await.is_err());
        tokio::time::timeout(Duration::from_secs(2), server).await??;
        Ok(())
    }

    #[tokio::test]
    async fn minimize_tolerates_only_confirmed_disappearing_targets() -> Result<()> {
        for (disappeared, message) in [
            (true, "No target with given id"),
            (false, "No target with given id"),
            (true, "Window access denied"),
        ] {
            let mut replies = vec![("/json/list", json!([{"id":"one","type":"page"}]))];
            if message == "No target with given id" {
                replies.push((
                    "/json/list",
                    if disappeared {
                        json!([])
                    } else {
                        json!([{"id":"one","type":"page"}])
                    },
                ));
            }
            let (port, http_server) = scripted_http(replies).await?;
            let websocket = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let ws_port = websocket.local_addr()?.port();
            let ws_server = tokio::spawn(async move {
                let (stream, _) = websocket.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let request: Value =
                    serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                        .unwrap();
                assert_eq!(request["method"], "Browser.getWindowForTarget");
                assert_eq!(request["params"]["targetId"], "one");
                socket
                    .send(Message::Text(
                        json!({"id":request["id"],"error":{"code":-32000,"message":message}})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
            });
            let chrome = Chrome {
                port,
                http: reqwest::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(2))
                    .build()?,
                websocket: format!("ws://127.0.0.1:{ws_port}/test"),
                activity: None,
            };
            assert_eq!(
                chrome.minimize().await.is_ok(),
                disappeared && message == "No target with given id"
            );
            http_server.await?;
            ws_server.await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn ui_deletion_acknowledgements_are_observed_through_real_cdp_correlation() -> Result<()>
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            for _ in 0..3 {
                let message = socket.next().await.unwrap().unwrap();
                let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "Network.enable" => json!({}),
                    "Runtime.evaluate" => {
                        for event in [
                            json!({"method":"Network.requestWillBeSent","params":{"requestId":"delete-one","request":{"method":"DELETE","url":"https://chatgpt.com/backend-api/conversation/id/ours"}}}),
                            json!({"method":"Network.responseReceived","params":{"requestId":"delete-one","response":{"url":"https://chatgpt.com/backend-api/conversation/id/ours","status":200}}}),
                            json!({"method":"Network.loadingFinished","params":{"requestId":"delete-one"}}),
                        ] {
                            socket
                                .send(Message::Text(event.to_string().into()))
                                .await
                                .unwrap();
                        }
                        json!({"result":{"value":0}})
                    }
                    "Network.getResponseBody" => {
                        assert_eq!(request["params"]["requestId"], "delete-one");
                        json!({"body":"{\"success\":true}","base64Encoded":false})
                    }
                    other => panic!("Unexpected CDP command {other}"),
                };
                socket
                    .send(Message::Text(
                        json!({"id":request["id"],"result":result})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
            }
        });
        let mut page =
            Page::connect("synthetic".into(), &format!("ws://127.0.0.1:{port}/test")).await?;
        page.watch_deletion("https://chatgpt.com/c/ours").await?;
        assert!(!page.deletion_acknowledged().await);
        page.eval("0").await?;
        assert!(page.deletion_acknowledged().await);
        server.await?;
        Ok(())
    }

    #[tokio::test]
    async fn idle_checks_do_not_launch_chrome_or_refresh_activity() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut config = Config {
            chrome_path: Some(dir.path().join("missing-browser")),
            ..Config::default()
        };
        assert!(!Chrome::close_if_idle(dir.path(), &config).await?);
        let activity = dir.path().join("chrome-activity");
        let old = std::time::SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&activity)?
            .set_modified(old)?;
        config.chrome_auto_close = false;
        assert!(!Chrome::close_if_idle(dir.path(), &config).await?);
        config.chrome_auto_close = true;
        assert!(!Chrome::close_if_idle(dir.path(), &config).await?);
        assert_eq!(std::fs::metadata(activity)?.modified()?, old);
        assert!(!dir.path().join("chrome.json").exists());
        Ok(())
    }

    #[tokio::test]
    #[ignore = "Requires Chrome; isolated profile, no account or prompt submission"]
    async fn idle_close_and_relaunch_restore_browser_access() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config = Config::default();
        let chrome = Chrome::ensure(dir.path(), &config).await?;
        let mut page = chrome.open("https://chatgpt.com/").await?;
        let target = page.id.clone();
        let activity = dir.path().join("chrome-activity");
        let expire = || -> Result<()> {
            std::fs::File::options()
                .write(true)
                .open(&activity)?
                .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))?;
            Ok(())
        };
        expire()?;
        page.eval("1 + 1").await?;
        assert!(!Chrome::close_if_idle(dir.path(), &config).await?);
        expire()?;
        assert!(Chrome::close_if_idle(dir.path(), &config).await?);
        assert!(!dir.path().join("chrome.json").exists());
        drop(page);
        let reopened = Chrome::ensure(dir.path(), &config).await?;
        let mut page = reopened
            .reconnect(Some(&target), Some("https://chatgpt.com/"))
            .await?;
        assert_ne!(page.id, target);
        assert_eq!(page.eval("1 + 1").await?, 2);
        assert_eq!(reopened.window_state(&page.id).await?, "minimized");
        drop(page);
        expire()?;
        assert!(Chrome::close_if_idle(dir.path(), &config).await?);
        Ok(())
    }

    #[test]
    fn validates_chatgpt_origin_exactly() {
        assert!(valid_chat_url("https://chatgpt.com/c/example"));
        for value in [
            "http://chatgpt.com/",
            "https://chatgpt.com.evil.org/",
            "https://chatgpt.com@evil.org/",
            "https://user@chatgpt.com/",
            "https://chatgpt.com:9222/",
            "file:///chatgpt.com",
        ] {
            assert!(!valid_chat_url(value), "{value}");
        }
    }

    #[test]
    fn conversation_identity_ignores_project_slugs_but_not_ids_or_scope() {
        let saved = "https://chatgpt.com/g/g-p-0123456789abcdef0123456789abcdef-research/c/ours";
        let current = "https://chatgpt.com/g/g-p-0123456789abcdef0123456789abcdef/c/ours";
        assert!(same_conversation_url(saved, current));
        assert!(same_conversation_url(
            saved,
            &format!("{current}?view=chat")
        ));
        assert!(same_project_url(
            current,
            &saved.replace("/c/ours", "/project")
        ));
        for other in [
            current.replace("/ours", "/other"),
            current.replace(
                "0123456789abcdef0123456789abcdef",
                "fedcba9876543210fedcba9876543210",
            ),
            current.replace("chatgpt.com", "chatgpt.com.evil.test"),
            "https://chatgpt.com/c/ours".into(),
            "about:blank".into(),
        ] {
            assert!(!same_conversation_url(saved, &other));
        }
    }

    #[tokio::test]
    async fn cdp_correlates_replies_while_ignoring_interleaved_events() -> Result<()> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let request: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            for value in [
                json!({"method":"Page.loaded","params":{}}),
                json!({"id":99,"result":{}}),
                json!({"id":request["id"],"result":{"result":{"value":"expected"}}}),
            ] {
                socket
                    .send(Message::Text(value.to_string().into()))
                    .await
                    .unwrap();
            }
        });
        let mut page = Page::connect("test".into(), &format!("ws://127.0.0.1:{port}/test")).await?;
        assert_eq!(page.eval("'expected'").await?, "expected");
        server.await?;
        assert!(
            Page::connect("test".into(), "ws://example.org/test")
                .await
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn background_reconnect_reuses_tabs_and_minimize_is_idempotent() -> Result<()> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for initially_minimized in [true, false] {
            let http = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let port = http.local_addr()?.port();
            let websocket = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let ws_port = websocket.local_addr()?.port();
            let updates = Arc::new(AtomicUsize::new(0));
            let writes = updates.clone();
            let minimized = Arc::new(AtomicBool::new(initially_minimized));
            let http_server = tokio::spawn(async move {
                for _ in 0..3 {
                    let (mut stream, _) = http.accept().await.unwrap();
                    let mut buffer = [0; 4096];
                    let mut headers = Vec::new();
                    loop {
                        let count = stream.read(&mut buffer).await.unwrap();
                        assert!(count > 0);
                        headers.extend_from_slice(&buffer[..count]);
                        if headers.windows(4).any(|part| part == b"\r\n\r\n") {
                            break;
                        }
                        assert!(headers.len() < 16384);
                    }
                    assert!(headers.starts_with(b"GET /json/list "));
                    let body = json!([{"id":"one","type":"page","url":"https://chatgpt.com/c/owned","webSocketDebuggerUrl":format!("ws://127.0.0.1:{ws_port}/test")},{"id":"two","type":"page"}]).to_string();
                    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                }
            });
            let ws_server = tokio::spawn(async move {
                for _ in 0..3 {
                    let (stream, _) = websocket.accept().await.unwrap();
                    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                    while let Some(Ok(Message::Text(raw))) = socket.next().await {
                        let request: Value = serde_json::from_str(&raw).unwrap();
                        let result = match request["method"].as_str().unwrap() {
                            "Browser.getWindowForTarget" => {
                                json!({"windowId":7,"bounds":{"windowState":if minimized.load(Ordering::SeqCst) {"minimized"} else {"maximized"}}})
                            }
                            "Browser.setWindowBounds" => {
                                assert_eq!(
                                    request["params"],
                                    json!({"windowId":7,"bounds":{"windowState":"minimized"}})
                                );
                                writes.fetch_add(1, Ordering::SeqCst);
                                minimized.store(true, Ordering::SeqCst);
                                json!({})
                            }
                            "Emulation.setFocusEmulationEnabled" => {
                                assert_eq!(request["params"], json!({"enabled":true}));
                                json!({})
                            }
                            other => panic!("Unexpected foreground/window command: {other}"),
                        };
                        socket
                            .send(Message::Text(
                                json!({"id":request["id"],"result":result})
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .unwrap();
                    }
                }
            });
            let chrome = Chrome {
                port,
                http: reqwest::Client::builder().no_proxy().build()?,
                websocket: format!("ws://127.0.0.1:{ws_port}/test"),
                activity: None,
            };
            chrome.minimize().await?;
            chrome.minimize().await?;
            let page = chrome
                .reconnect(Some("one"), Some("https://chatgpt.com/c/owned"))
                .await?;
            assert_eq!(page.id, "one");
            drop(page);
            assert_eq!(
                updates.load(Ordering::SeqCst),
                usize::from(!initially_minimized)
            );
            http_server.await?;
            ws_server.await?;
        }
        Ok(())
    }
}
#[test]
fn launches_normal_chrome_without_test_automation_switches() {
    let args = launch_arguments("profile", 9223, "about:blank#marker");
    assert!(args.contains(&"--remote-debugging-port=9223".to_owned()));
    assert!(!args.iter().any(|arg| arg.contains("enable-automation")
        || arg == "--remote-debugging-port=0"
        || arg.starts_with("--headless")));
}
