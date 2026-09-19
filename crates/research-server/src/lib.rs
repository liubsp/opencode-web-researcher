mod api;
pub mod client;
mod worker;

use anyhow::{Context, Result};
use fs2::FileExt;
use research_core::{Config, PROTOCOL, ServiceDescriptor};
use research_store::Store;
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::{Notify, watch};

#[derive(Clone)]
pub struct State {
    pub dir: PathBuf,
    pub store: Arc<Mutex<Store>>,
    pub descriptor: ServiceDescriptor,
    pub notify: Arc<Notify>,
    pub shutdown: watch::Sender<bool>,
}

pub fn secure_directory(dir: &Path) -> Result<()> {
    let new = !dir.exists();
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    if new {
        let user = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN")?,
            std::env::var("USERNAME")?
        );
        let result = std::process::Command::new("icacls")
            .arg(dir)
            .args(["/inheritance:r", "/grant:r", &format!("{user}:(OI)(CI)F")])
            .output()?;
        anyhow::ensure!(
            result.status.success(),
            "Cannot restrict research data directory permissions"
        );
    }
    #[cfg(not(windows))]
    let _ = new;
    Ok(())
}

pub fn lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock_exclusive()
        .context("Research service already running or startup in progress")?;
    Ok(file)
}

pub async fn serve(dir: PathBuf) -> Result<()> {
    secure_directory(&dir)?;
    let _lifetime = lock(&dir.join("service.lock"))?;
    Config::load(&dir)?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let descriptor = ServiceDescriptor {
        protocol: PROTOCOL,
        port: listener.local_addr()?.port(),
        token: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
        instance: uuid::Uuid::new_v4().to_string(),
        pid: std::process::id(),
    };
    let (shutdown, mut stopped) = watch::channel(false);
    let state = State {
        store: Arc::new(Mutex::new(Store::open(&dir.join("state.sqlite"))?)),
        dir: dir.clone(),
        descriptor: descriptor.clone(),
        notify: Arc::new(Notify::new()),
        shutdown,
    };
    // Atomic on Unix; Windows requires removing the stale destination while holding the lifetime lock.
    let tmp = dir.join("service.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&descriptor)?)?;
    if dir.join("service.json").exists() {
        std::fs::remove_file(dir.join("service.json"))?;
    }
    std::fs::rename(tmp, dir.join("service.json"))?;
    let worker = tokio::spawn(worker::run(state.clone()));
    let result = axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async move {
            tokio::select! { _ = stopped.changed() => {}, _ = tokio::signal::ctrl_c() => {} }
        })
        .await;
    worker.abort();
    let _ = worker.await;
    let _ = std::fs::remove_file(dir.join("service.json"));
    result?;
    Ok(())
}
