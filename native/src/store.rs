use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub database_path: String,
    pub athlete_id: String,
    pub years: u32,
    pub months_per_request: u32,
    pub time_zone: String,
    pub codex_path: String,
    pub claude_path: String,
}

impl Settings {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("settings.json");
        if path.exists() {
            return serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string());
        }
        Ok(Self {
            database_path: std::env::var("TRAININGPEAKS_DATABASE")
                .unwrap_or_else(|_| root.join("trainingpeaks.sqlite").to_string_lossy().into()),
            athlete_id: String::new(),
            years: 5,
            months_per_request: 3,
            time_zone: "America/Menominee".into(),
            codex_path: "codex".into(),
            claude_path: "claude".into(),
        })
    }
    pub fn validate(&self) -> Result<()> {
        if !(1..=50).contains(&self.years) || !(1..=12).contains(&self.months_per_request) {
            return Err("History must be 1–50 years and export windows 1–12 months".into());
        }
        if !PathBuf::from(&self.database_path).is_absolute() {
            return Err("Database path must be absolute".into());
        }
        if !self.athlete_id.is_empty() && !numeric_id(&self.athlete_id) {
            return Err("Athlete ID must be numeric".into());
        }
        if self.time_zone.parse::<chrono_tz::Tz>().is_err() {
            return Err("Use a valid IANA timezone, such as America/Menominee".into());
        }
        if self.time_zone.is_empty() || self.codex_path.is_empty() || self.claude_path.is_empty() {
            return Err("Timezone and CLI paths are required".into());
        }
        Ok(())
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        let pending = root.join("settings.pending.json");
        std::fs::write(
            &pending,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        // Windows cannot replace a destination with rename.
        #[cfg(windows)]
        if root.join("settings.json").exists() {
            std::fs::remove_file(root.join("settings.json")).map_err(|e| e.to_string())?;
        }
        std::fs::rename(pending, root.join("settings.json")).map_err(|e| e.to_string())
    }
}

pub fn numeric_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|b| b.is_ascii_digit())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub provider: String,
    pub provider_session_id: Option<String>,
    pub database_path: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub status: String,
}

pub struct ChatStore {
    connection: Connection,
}

impl ChatStore {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path).map_err(|e| e.to_string())?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY, provider TEXT NOT NULL, provider_session_id TEXT,
                database_path TEXT NOT NULL, title TEXT NOT NULL DEFAULT 'New chat',
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')));
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id),
                role TEXT NOT NULL, content TEXT NOT NULL, status TEXT NOT NULL);
            UPDATE messages SET status='interrupted' WHERE status='running';",
            )
            .map_err(|e| e.to_string())?;
        Ok(Self { connection })
    }
    fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
        Ok(Session {
            id: row.get(0)?,
            provider: row.get(1)?,
            provider_session_id: row.get(2)?,
            database_path: row.get(3)?,
            title: row.get(4)?,
            updated_at: row.get(5)?,
        })
    }
    pub fn list(&self) -> Result<Vec<Session>> {
        let mut query = self.connection.prepare("SELECT id,provider,provider_session_id,database_path,title,updated_at FROM sessions ORDER BY updated_at DESC, rowid DESC").map_err(|e| e.to_string())?;
        let rows = query.query_map([], Self::row).map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())
    }
    pub fn get(&self, id: &str) -> Result<Session> {
        self.connection.query_row("SELECT id,provider,provider_session_id,database_path,title,updated_at FROM sessions WHERE id=?", [id], Self::row).map_err(|_| "Chat session not found".into())
    }
    pub fn create(&self, provider: &str, database: &str) -> Result<Session> {
        if !["codex", "claude"].contains(&provider) {
            return Err("Choose Codex or Claude".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.connection
            .execute(
                "INSERT INTO sessions(id,provider,database_path) VALUES (?,?,?)",
                params![id, provider, database],
            )
            .map_err(|e| e.to_string())?;
        self.get(&id)
    }
    pub fn set_provider_id(&self, id: &str, provider_id: &str) -> Result<()> {
        uuid::Uuid::parse_str(provider_id)
            .map_err(|_| "CLI returned an invalid session ID".to_string())?;
        self.connection
            .execute(
                "UPDATE sessions SET provider_session_id=? WHERE id=?",
                params![provider_id, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn add(&self, id: &str, role: &str, content: &str, status: &str) -> Result<i64> {
        self.get(id)?;
        self.connection
            .execute(
                "INSERT INTO messages(session_id,role,content,status) VALUES (?,?,?,?)",
                params![id, role, content, status],
            )
            .map_err(|e| e.to_string())?;
        let message_id = self.connection.last_insert_rowid();
        self.connection.execute("UPDATE sessions SET updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'), title=CASE WHEN title='New chat' AND ?='user' THEN substr(?,1,60) ELSE title END WHERE id=?", params![role,content,id]).map_err(|e| e.to_string())?;
        Ok(message_id)
    }
    pub fn finish(&self, message: i64, content: &str, status: &str) -> Result<()> {
        self.connection
            .execute(
                "UPDATE messages SET content=?,status=? WHERE id=?",
                params![content, status, message],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn messages(&self, id: &str) -> Result<Vec<Message>> {
        self.get(id)?;
        let mut query = self.connection.prepare("SELECT id,session_id,role,content,status FROM messages WHERE session_id=? ORDER BY id").map_err(|e| e.to_string())?;
        let rows = query
            .query_map([id], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    status: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sessions_keep_provider_database_and_resume_id() {
        let store = ChatStore::open(Path::new(":memory:")).unwrap();
        let a = store.create("codex", "/tmp/a.sqlite").unwrap();
        let b = store.create("claude", "/tmp/b.sqlite").unwrap();
        let uuid = uuid::Uuid::new_v4().to_string();
        store.set_provider_id(&a.id, &uuid).unwrap();
        let message = store.add(&a.id, "assistant", "", "running").unwrap();
        store.finish(message, "Answer", "complete").unwrap();
        assert_eq!(
            store.get(&a.id).unwrap().provider_session_id.as_deref(),
            Some(uuid.as_str())
        );
        assert_eq!(store.get(&b.id).unwrap().database_path, "/tmp/b.sqlite");
        assert!(store.messages(&b.id).unwrap().is_empty());
        assert_eq!(store.messages(&a.id).unwrap()[0].content, "Answer");
        assert!(store.create("other", "/tmp/c.sqlite").is_err());
    }
}
