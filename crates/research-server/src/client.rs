use anyhow::{Context, Result, ensure};
use research_core::{PROTOCOL, ServiceDescriptor};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub async fn discover(dir: &Path) -> Result<ServiceDescriptor> {
    let descriptor: ServiceDescriptor =
        serde_json::from_slice(&std::fs::read(dir.join("service.json"))?)?;
    ensure!(
        descriptor.protocol == PROTOCOL,
        "Incompatible service protocol; stop the old daemon first"
    );
    let result = rpc(&descriptor, serde_json::json!({"op":"health"}), 3).await?;
    ensure!(
        result["instance"] == descriptor.instance,
        "Service instance changed"
    );
    Ok(descriptor)
}

pub async fn rpc(service: &ServiceDescriptor, input: Value, timeout: u64) -> Result<Value> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(timeout))
        .build()?
        .post(format!("http://127.0.0.1:{}/v1/rpc", service.port))
        .bearer_auth(&service.token)
        .json(&input)
        .send()
        .await?;
    let status = response.status();
    let value: Value = response.json().await?;
    ensure!(
        status.is_success(),
        "{}",
        value["error"].as_str().unwrap_or("Research API error")
    );
    Ok(value)
}

pub async fn ensure_running(dir: PathBuf, executable: &Path) -> Result<ServiceDescriptor> {
    if let Ok(service) = discover(&dir).await {
        return Ok(service);
    }
    crate::secure_directory(&dir)?;
    // Every bootstrapper uses the same short-lived startup lock; the daemon uses a separate lifetime lock.
    let mut guard = None;
    for _ in 0..100 {
        if let Ok(lock) = crate::lock(&dir.join("startup.lock")) {
            guard = Some(lock);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(service) = discover(&dir).await {
            return Ok(service);
        }
    }
    let _guard = guard.context("Timed out acquiring service startup lock")?;
    if let Ok(service) = discover(&dir).await {
        return Ok(service);
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("service.log"))?;
    let mut command = Command::new(executable);
    command
        .arg("serve")
        .env("WEB_RESEARCH_HOME", &dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x00000200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().context("Could not spawn research daemon")?;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(service) = discover(&dir).await {
            return Ok(service);
        }
    }
    anyhow::bail!(
        "Daemon did not become ready; inspect {}",
        dir.join("service.log").display()
    )
}
