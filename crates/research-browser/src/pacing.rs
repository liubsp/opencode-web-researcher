//! Persistent service-wide admission. Reading or denying a slot never moves its deadline.
use anyhow::{Result, ensure};
use fs2::FileExt;
use research_core::{Config, now};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interaction {
    Ui,
    Account,
    Navigation,
}

#[derive(Debug)]
pub struct Deferred {
    pub next_at: i64,
}
impl std::fmt::Display for Deferred {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Browser interaction deferred until {}", self.next_at)
    }
}
impl std::error::Error for Deferred {}

#[derive(Default, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    pub next_ui_at_ms: i64,
    pub next_account_at_ms: i64,
    pub next_load_at_ms: i64,
    pub last_action_at_ms: i64,
    pub last_action: Option<Interaction>,
    pub document_loads: u64,
    pub network_failures: u64,
    pub http_errors: u64,
    pub throttled_responses: u64,
    pub last_http_error: Option<u16>,
    pub last_throttled_at: i64,
    pub observer_connected: bool,
    pub observer_last_event_at: i64,
    /// Local CDP readiness only; omitted from health and diagnostic logs.
    pub observer_ready_targets: Vec<String>,
}
impl Status {
    fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.next_ui_at_ms,
                self.next_account_at_ms,
                self.next_load_at_ms,
                self.last_action_at_ms
            ]
            .iter()
            .all(|at| (0..=i64::MAX - 86_400_000).contains(at)),
            "Invalid pacing timestamp"
        );
        Ok(())
    }
    pub fn next_at(&self, action: Interaction) -> i64 {
        (self.next_at_ms(action) + 999) / 1000
    }
    fn next_at_ms(&self, action: Interaction) -> i64 {
        match action {
            Interaction::Ui => self.next_ui_at_ms,
            Interaction::Account => self.next_ui_at_ms.max(self.next_account_at_ms),
            Interaction::Navigation => self
                .next_ui_at_ms
                .max(self.next_account_at_ms)
                .max(self.next_load_at_ms),
        }
    }
}

#[derive(Clone)]
pub struct Pacer {
    dir: PathBuf,
}
impl Pacer {
    pub fn new(dir: &Path) -> Self {
        Self { dir: dir.into() }
    }
    fn update<T>(&self, update: impl FnOnce(&mut Status) -> Result<T>) -> Result<T> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.dir.join("browser-pacing.lock"))?;
        lock.lock_exclusive()?;
        let path = self.dir.join("browser-pacing.json");
        let (mut file, created) = match OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => (file, true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                (OpenOptions::new().read(true).write(true).open(path)?, false)
            }
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        // Corruption fails closed instead of silently authorizing another interaction.
        let mut status: Status = if bytes.is_empty() && created {
            Status::default()
        } else {
            serde_json::from_slice(&bytes)?
        };
        status.validate()?;
        let result = update(&mut status)?;
        let bytes = serde_json::to_vec(&status)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&bytes)?;
        file.set_len(bytes.len() as u64)?;
        file.sync_all()?;
        Ok(result)
    }
    pub fn status(&self) -> Result<Status> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.dir.join("browser-pacing.lock"))?;
        FileExt::lock_shared(&lock)?;
        let mut file = match OpenOptions::new()
            .read(true)
            .open(self.dir.join("browser-pacing.json"))
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Status::default());
            }
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let status: Status = serde_json::from_slice(&bytes)?;
        status.validate()?;
        Ok(status)
    }
    pub fn ready(&self, action: Interaction) -> Result<()> {
        let next_ms = self.status()?.next_at_ms(action);
        if precise_now() < next_ms {
            return Err(Deferred {
                next_at: (next_ms + 999) / 1000,
            }
            .into());
        }
        Ok(())
    }
    pub fn admit(&self, action: Interaction) -> Result<()> {
        let c = Config::load(&self.dir)?;
        let sample = |min: u64, max: u64| {
            min + (uuid::Uuid::new_v4().as_u128() % (max - min + 1) as u128) as u64
        };
        self.admit_at(
            action,
            precise_now(),
            [
                sample(c.ui_action_delay_min_seconds, c.ui_action_delay_max_seconds),
                sample(
                    c.account_action_delay_min_seconds,
                    c.account_action_delay_max_seconds,
                ),
                sample(c.page_load_delay_min_seconds, c.page_load_delay_max_seconds),
            ],
        )
    }
    fn admit_at(&self, action: Interaction, at: i64, delays: [u64; 3]) -> Result<()> {
        ensure!(
            delays.iter().all(|delay| (1..=3600).contains(delay)),
            "Invalid interaction interval"
        );
        self.update(|s| {
            let next_at = s.next_at_ms(action);
            if at < next_at {
                return Err(Deferred {
                    next_at: (next_at + 999) / 1000,
                }
                .into());
            }
            s.last_action = Some(action);
            s.last_action_at_ms = at;
            s.next_ui_at_ms = at + delays[0] as i64 * 1000;
            if matches!(action, Interaction::Account | Interaction::Navigation) {
                s.next_account_at_ms = at + delays[1] as i64 * 1000;
            }
            if matches!(action, Interaction::Navigation) {
                s.next_load_at_ms = at + delays[2] as i64 * 1000;
            }
            Ok(())
        })?;
        self.log("interaction_admitted", json!({"kind":action}));
        Ok(())
    }
    pub fn observed(
        &self,
        event: &str,
        status: Option<u16>,
        retry_after: Option<u64>,
    ) -> Result<()> {
        self.update(|s| {
            s.observer_last_event_at = now();
            match event {
                "observer_connected" => s.observer_connected = true,
                "observer_disconnected" => {
                    s.observer_connected = false;
                    s.observer_ready_targets.clear();
                }
                "document_request" => s.document_loads = s.document_loads.saturating_add(1),
                "network_failure" => s.network_failures = s.network_failures.saturating_add(1),
                "http_error" => {
                    s.http_errors = s.http_errors.saturating_add(1);
                    s.last_http_error = status;
                    if status == Some(429) {
                        s.throttled_responses = s.throttled_responses.saturating_add(1);
                        s.last_throttled_at = now();
                        let until = precise_now()
                            + retry_after.unwrap_or(900).clamp(60, 86400) as i64 * 1000;
                        s.next_account_at_ms = s.next_account_at_ms.max(until);
                        s.next_load_at_ms = s.next_load_at_ms.max(until);
                        s.next_ui_at_ms = s.next_ui_at_ms.max(until);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        self.log(event, json!({"status":status}));
        Ok(())
    }
    /// Callers supply static categories and sanitized fields, never CDP payloads or URLs.
    pub fn observer_target(&self, target: &str, ready: bool) -> Result<()> {
        self.update(|s| {
            s.observer_ready_targets.retain(|id| id != target);
            if ready {
                s.observer_ready_targets.push(target.to_owned());
            }
            Ok(())
        })
    }

    pub fn log(&self, event: &str, detail: Value) {
        let append = || -> Result<()> {
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(self.dir.join("browser-events.lock"))?;
            lock.lock_exclusive()?;
            let path = self.dir.join("browser-events.jsonl");
            if std::fs::metadata(&path).is_ok_and(|m| m.len() >= 1_048_576) {
                let old = self.dir.join("browser-events.previous.jsonl");
                if old.exists() {
                    std::fs::remove_file(&old)?;
                }
                std::fs::rename(&path, old)?;
            }
            let mut file = OpenOptions::new().create(true).append(true).open(path)?;
            writeln!(
                file,
                "{}",
                json!({"at_ms":precise_now(),"event":event,"detail":detail})
            )?;
            Ok(())
        };
        if append().is_err() {
            eprintln!("Sanitized browser event log could not be written");
        }
    }
}

fn precise_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn racing_workers_share_one_slot_and_corrupt_state_fails_closed() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let successes = std::thread::scope(|scope| {
            let calls: Vec<_> = (0..8)
                .map(|_| {
                    let p = Pacer::new(dir.path());
                    scope.spawn(move || {
                        p.admit_at(Interaction::Navigation, 100000, [2, 60, 300])
                            .is_ok()
                    })
                })
                .collect();
            calls
                .into_iter()
                .map(|call| call.join().unwrap() as usize)
                .sum::<usize>()
        });
        assert_eq!(successes, 1);
        std::fs::write(dir.path().join("browser-pacing.json"), b"")?;
        assert!(Pacer::new(dir.path()).admit(Interaction::Ui).is_err());
        assert!(Pacer::new(dir.path()).status().is_err());
        Ok(())
    }
    #[test]
    fn shared_deadlines_overlap_survive_restart_and_denial_never_resamples() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let p = Pacer::new(dir.path());
        p.admit_at(Interaction::Navigation, 100000, [3, 120, 600])?;
        p.admit_at(Interaction::Ui, 103000, [2, 180, 300])?;
        let other = Pacer::new(dir.path());
        for at in [104, 150, 699] {
            assert_eq!(
                other
                    .admit_at(Interaction::Navigation, at * 1000, [5, 60, 300])
                    .unwrap_err()
                    .downcast_ref::<Deferred>()
                    .unwrap()
                    .next_at,
                700
            );
        }
        assert_eq!(other.status()?.next_account_at_ms, 220000);
        assert_eq!(other.status()?.next_load_at_ms, 700000);
        other.admit_at(Interaction::Account, 220000, [5, 180, 300])?;
        assert_eq!(other.status()?.next_load_at_ms, 700000);
        other.admit_at(Interaction::Navigation, 700000, [5, 60, 300])?;
        assert_eq!(other.status()?.next_load_at_ms, 1000000);
        Ok(())
    }
    #[test]
    fn defaults_are_randomized_and_throttling_pauses_every_action_class() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let p = Pacer::new(dir.path());
        for _ in 0..10 {
            p.update(|s| {
                *s = Status::default();
                Ok(())
            })?;
            p.admit(Interaction::Navigation)?;
            let s = p.status()?;
            assert!((2000..=5000).contains(&(s.next_ui_at_ms - s.last_action_at_ms)));
            assert!((60000..=180000).contains(&(s.next_account_at_ms - s.last_action_at_ms)));
            assert!((300000..=600000).contains(&(s.next_load_at_ms - s.last_action_at_ms)));
        }
        p.observed("http_error", Some(429), Some(1200))?;
        for kind in [
            Interaction::Ui,
            Interaction::Account,
            Interaction::Navigation,
        ] {
            assert!(p.admit(kind).is_err());
        }
        assert_eq!(p.status()?.throttled_responses, 1);
        Ok(())
    }
}
