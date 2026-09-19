use super::*;
use research_core::{ChatReference, ReadRequest};

impl Store {
    pub fn submit_read(
        &mut self,
        project: &str,
        session: &str,
        key: &str,
        chats: Vec<ChatReference>,
        at: i64,
    ) -> Result<ReadRequest> {
        ensure!(
            !project.is_empty() && !session.is_empty() && !key.is_empty() && key.len() <= 512,
            "Invalid read request scope or key"
        );
        ensure!(
            (1..=10).contains(&chats.len()),
            "Supply 1–10 chats per read request"
        );
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT data FROM chat_reads WHERE project=? AND request_key=?",
                params![project, key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(prior) = prior {
            let prior: ReadRequest = serde_json::from_str(&prior)?;
            ensure!(
                prior.session == session && prior.chats == chats,
                "idempotency_conflict"
            );
            return Ok(prior);
        }
        let request = ReadRequest {
            id: format!("read-{}", Uuid::new_v4()),
            project: project.into(),
            session: session.into(),
            key: key.into(),
            chats,
            state: "queued".into(),
            created_at: at,
            updated_at: at,
            results: vec![],
        };
        tx.execute("INSERT INTO chat_reads (id,project,request_key,state,updated_at,data) VALUES (?,?,?,?,?,?)", params![request.id,project,key,request.state,at,serde_json::to_string(&request)?])?;
        tx.commit()?;
        Ok(request)
    }

    pub fn read_request(&self, id: &str) -> Result<ReadRequest> {
        let text: String =
            self.conn
                .query_row("SELECT data FROM chat_reads WHERE id=?", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save_read(&self, request: &ReadRequest) -> Result<()> {
        self.conn.execute(
            "UPDATE chat_reads SET state=?,updated_at=?,data=? WHERE id=?",
            params![
                request.state,
                request.updated_at,
                serde_json::to_string(request)?,
                request.id
            ],
        )?;
        Ok(())
    }

    pub fn next_read(&self) -> Result<Option<ReadRequest>> {
        let text: Option<String> = self.conn.query_row("SELECT data FROM chat_reads WHERE state IN ('queued','reading') ORDER BY rowid LIMIT 1", [], |r| r.get(0)).optional()?;
        text.map(|text| Ok(serde_json::from_str(&text)?))
            .transpose()
    }

    pub fn checkpoint_reads(&self, dir: &Path) -> Result<()> {
        let mut stmt = self.conn.prepare("SELECT data FROM chat_reads")?;
        for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let request: ReadRequest = serde_json::from_str(&row?)?;
            self.checkpoint_read(&request, dir)?;
        }
        Ok(())
    }

    pub fn checkpoint_read(&self, request: &ReadRequest, dir: &Path) -> Result<()> {
        let folder = dir.join("imports").join(&request.id);
        std::fs::create_dir_all(&folder)?;
        for (index, result) in request.results.iter().enumerate() {
            if let Some(markdown) = result["markdown"].as_str() {
                write_once(&folder.join(format!("{index}.md")), markdown.as_bytes())?;
            }
            remove_legacy_json(&folder.join(format!("{index}.json")))?;
        }
        Ok(())
    }

    pub fn purge_expired_reads(&mut self, dir: &Path, cutoff: i64) -> Result<()> {
        let ids = {
            let mut stmt = self.conn.prepare("SELECT id FROM chat_reads WHERE state IN ('completed','cancelled') AND updated_at < ?")?;
            stmt.query_map([cutoff], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in ids {
            match std::fs::remove_dir_all(dir.join("imports").join(&id)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            self.conn
                .execute("DELETE FROM chat_reads WHERE id=?", [id])?;
        }
        Ok(())
    }
}
