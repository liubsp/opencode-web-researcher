use anyhow::{Context, Result, ensure};
use research_core::{PROTOCOL, ServiceDescriptor};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
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
    // Health probes consume time too. Bound the complete bootstrap below the
    // plugin's existing 120-second connect timeout, not just its sleep loops.
    tokio::time::timeout(
        Duration::from_secs(110),
        ensure_running_inner(dir, executable),
    )
    .await
    .context("Research service startup deadline exceeded")?
}

pub fn preferred_executable(dir: &Path, fallback: &Path) -> Result<PathBuf> {
    let bytes = match std::fs::read(dir.join("preferred-server.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(fallback.into()),
        Err(error) => return Err(error.into()),
    };
    let value: Value = serde_json::from_slice(&bytes)?;
    let binary = PathBuf::from(
        value["binary"]
            .as_str()
            .context("Invalid preferred server pointer")?,
    );
    ensure!(
        binary.is_absolute() && binary.is_file() && binary.file_name() == fallback.file_name(),
        "Preferred research server is missing or invalid; refusing to start an older build"
    );
    Ok(binary)
}

/// Installers share the bootstrap lock. Record the preferred build, drain the old
/// daemon, and register the replacement before an older client can bootstrap it.
pub async fn activate(dir: PathBuf, executable: &Path) -> Result<ServiceDescriptor> {
    tokio::time::timeout(Duration::from_secs(110), async {
        crate::secure_directory(&dir)?;
        let _startup = loop {
            if let Ok(lock) = crate::lock(&dir.join("startup.lock")) {
                break lock;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let binary = std::fs::canonicalize(executable)?;
        let tmp = dir.join("preferred-server.json.tmp");
        let mut file = std::fs::File::create(&tmp)?;
        use std::io::Write;
        file.write_all(
            serde_json::to_string(
                &serde_json::json!({"binary":binary,"build_id":research_core::BUILD_ID}),
            )?
            .as_bytes(),
        )?;
        file.sync_all()?;
        std::fs::rename(tmp, dir.join("preferred-server.json"))?;
        if let Ok(service) = discover(&dir).await {
            let health = rpc(&service, serde_json::json!({"op":"health"}), 3).await?;
            if health["build_id"] == research_core::BUILD_ID {
                return Ok(service);
            }
            rpc(&service, serde_json::json!({"op":"shutdown"}), 5).await?;
        }
        let lifetime = loop {
            if let Ok(lock) = crate::lock(&dir.join("service.lock")) {
                break lock;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        drop(lifetime);
        let mut child = crate::launch::spawn(&dir, &binary)
            .context("Could not start preferred research daemon")?;
        loop {
            if let Ok(service) = discover(&dir).await {
                let health = rpc(&service, serde_json::json!({"op":"health"}), 3).await?;
                ensure!(
                    health["build_id"] == research_core::BUILD_ID,
                    "Another research build became active during installation"
                );
                return Ok(service);
            }
            if child.try_wait()?.is_some() {
                anyhow::bail!("Preferred research daemon exited; inspect service.log");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .context("Research server activation deadline exceeded")?
}

async fn ensure_running_inner(dir: PathBuf, executable: &Path) -> Result<ServiceDescriptor> {
    if let Ok(service) = discover(&dir).await {
        return Ok(service);
    }
    crate::secure_directory(&dir)?;
    // Every bootstrapper uses the same short-lived startup lock; the daemon uses a separate lifetime lock.
    let mut guard = None;
    for _ in 0..900 {
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
    let executable = preferred_executable(&dir, executable)?;
    let mut child =
        crate::launch::spawn(&dir, &executable).context("Could not spawn research daemon")?;
    for attempt in 0..900 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(service) = discover(&dir).await {
            return Ok(service);
        }
        // An old daemon can still hold its lifetime lock while long-poll requests drain.
        // Retry a failed child under the startup lock instead of requiring a second install.
        if attempt % 10 == 9 && child.try_wait()?.is_some() {
            child = crate::launch::spawn(&dir, &executable)
                .context("Could not restart research daemon")?;
        }
    }
    anyhow::bail!(
        "Daemon did not become ready; inspect {}",
        dir.join("service.log").display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn older_launchers_resolve_the_preferred_build_and_never_silently_downgrade() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let old = dir.path().join("old").join("research.exe");
        let new = dir.path().join("new").join("research.exe");
        std::fs::create_dir_all(new.parent().unwrap())?;
        std::fs::write(&new, "synthetic binary")?;
        assert_eq!(preferred_executable(dir.path(), &old)?, old);
        std::fs::write(
            dir.path().join("preferred-server.json"),
            serde_json::to_vec(&serde_json::json!({"binary":new}))?,
        )?;
        assert_eq!(preferred_executable(dir.path(), &old)?, new);
        std::fs::remove_file(new)?;
        assert!(preferred_executable(dir.path(), &old).is_err());
        Ok(())
    }
}
