//! Port of `configurator/configdb.py`: a SQLite-backed key/value store for
//! HiFiBerry OS configuration, with optional Fernet encryption for secure
//! values and Flask-style HTTP handler methods.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use fernet::Fernet;
use rusqlite::Connection;
use serde_json::Value;

pub const DEFAULT_DB_PATH: &str = "/var/hifiberry/config.sqlite";
pub const DEFAULT_KEY_FILE: &str = "/etc/configdb.key";

/// Parse flexible boolean inputs from API/CLI payloads (mirrors `_parse_bool`).
pub fn parse_bool(value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => match n.as_i64() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err("Boolean integer values must be 0 or 1".to_string()),
        },
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(true),
            "false" | "0" | "no" | "off" | "" => Ok(false),
            _ => Err("Invalid boolean string".to_string()),
        },
        _ => Err("Invalid boolean value type".to_string()),
    }
}

/// A SQLite-backed key/value configuration store.
pub struct ConfigDb {
    conn: Connection,
    key_file: PathBuf,
}

impl ConfigDb {
    /// Open the config database at the default system paths, falling back
    /// to a temp-directory database if those aren't writable (e.g. running
    /// as a non-root user in tests/CI) rather than panicking.
    pub fn open_default() -> Self {
        if let Ok(db) = Self::open(Path::new(DEFAULT_DB_PATH), Path::new(DEFAULT_KEY_FILE)) {
            return db;
        }
        let fallback_dir = std::env::temp_dir().join("hbos-configdb");
        Self::open(&fallback_dir.join("config.sqlite"), &fallback_dir.join("configdb.key")).expect("failed to open fallback config database")
    }

    /// Open (creating if necessary) the config database at `db_path`, using
    /// `key_file` for the Fernet encryption key.
    pub fn open(db_path: &Path, key_file: &Path) -> Result<Self, String> {
        if let Some(dir) = db_path.parent() {
            if !dir.as_os_str().is_empty() && !dir.exists() {
                fs::create_dir_all(dir).map_err(|e| format!("Couldn't create directory {}: {e}", dir.display()))?;
            }
        }

        let conn = Connection::open(db_path).map_err(|e| format!("Couldn't initialize database: {e}"))?;
        // Concurrent test/handler instances may share the same on-disk file
        // (e.g. the temp-dir fallback); wait for locks instead of failing
        // immediately, and use WAL mode for better multi-connection behavior.
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| format!("Couldn't set busy timeout: {e}"))?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| format!("Couldn't set journal mode: {e}"))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                modified_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            (),
        )
        .map_err(|e| format!("Couldn't initialize database: {e}"))?;

        Ok(Self { conn, key_file: key_file.to_path_buf() })
    }

    fn get_encryption_key(&self) -> Result<String, String> {
        if !self.key_file.exists() {
            let key = Fernet::generate_key();
            fs::write(&self.key_file, &key).map_err(|e| format!("Couldn't write key file: {e}"))?;
            let mut perms = fs::metadata(&self.key_file).map_err(|e| e.to_string())?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&self.key_file, perms).map_err(|e| e.to_string())?;
            Ok(key)
        } else {
            fs::read_to_string(&self.key_file).map_err(|e| format!("Couldn't read key file: {e}"))
        }
    }

    /// Encrypt a value using the encryption key.
    pub fn encrypt_value(&self, value: &str) -> Result<String, String> {
        let key = self.get_encryption_key()?;
        let fernet = Fernet::new(key.trim()).ok_or("Invalid encryption key")?;
        Ok(fernet.encrypt(value.as_bytes()))
    }

    /// Decrypt an encrypted value using the encryption key.
    pub fn decrypt_value(&self, encrypted_value: &str) -> Result<String, String> {
        let key = self.get_encryption_key()?;
        let fernet = Fernet::new(key.trim()).ok_or("Invalid encryption key")?;
        let decrypted = fernet.decrypt(encrypted_value).map_err(|_| "Failed to decrypt value: invalid token".to_string())?;
        String::from_utf8(decrypted).map_err(|e| e.to_string())
    }

    fn raw_get(&self, key: &str) -> Option<String> {
        self.conn.query_row("SELECT value FROM config WHERE key = ?1", [key], |row| row.get(0)).ok()
    }

    /// Get a value from the database, optionally decrypting it if `secure` is `true`.
    pub fn get(&self, key: &str, default: Option<&str>, secure: bool) -> Option<String> {
        match self.raw_get(key) {
            Some(value) => {
                if secure {
                    self.decrypt_value(&value).ok().or_else(|| default.map(|s| s.to_string()))
                } else {
                    Some(value)
                }
            }
            None => default.map(|s| s.to_string()),
        }
    }

    /// Store a key/value pair, optionally encrypting it if `secure` is `true`.
    ///
    /// Skips the write (returning `true`) if the stored value already
    /// matches, comparing on the unencrypted value.
    pub fn set(&mut self, key: &str, value: &str, secure: bool) -> bool {
        let current_value = self.raw_get(key);

        if let Some(current) = &current_value {
            let decrypted_current = if secure { self.decrypt_value(current).ok() } else { Some(current.clone()) };
            if decrypted_current.as_deref() == Some(value) {
                return true;
            }
        }

        let encrypted_value = if secure {
            match self.encrypt_value(value) {
                Ok(v) => v,
                Err(_) => return false,
            }
        } else {
            value.to_string()
        };

        self.conn
            .execute("INSERT OR REPLACE INTO config (key, value, modified_at) VALUES (?1, ?2, CURRENT_TIMESTAMP)", (key, &encrypted_value))
            .is_ok()
    }

    /// Delete a key from the database.
    pub fn delete(&mut self, key: &str) -> bool {
        self.conn.execute("DELETE FROM config WHERE key = ?1", [key]).is_ok()
    }

    /// List all keys, optionally filtered by prefix.
    pub fn list_keys(&self, prefix: Option<&str>) -> Vec<String> {
        let result = match prefix {
            Some(p) => {
                let mut stmt = match self.conn.prepare("SELECT key FROM config WHERE key LIKE ?1") {
                    Ok(s) => s,
                    Err(_) => return Vec::new(),
                };
                stmt.query_map([format!("{p}%")], |row| row.get(0)).and_then(Iterator::collect::<Result<Vec<String>, _>>)
            }
            None => {
                let mut stmt = match self.conn.prepare("SELECT key FROM config") {
                    Ok(s) => s,
                    Err(_) => return Vec::new(),
                };
                stmt.query_map([], |row| row.get(0)).and_then(Iterator::collect::<Result<Vec<String>, _>>)
            }
        };
        result.unwrap_or_default()
    }

    /// Delete all keys from the database.
    pub fn clear_all(&mut self) -> bool {
        self.conn.execute("DELETE FROM config", ()).is_ok()
    }

    /// Get all key/value pairs, optionally filtered by prefix.
    pub fn get_all(&self, prefix: Option<&str>) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        let query_result = match prefix {
            Some(p) => {
                let mut stmt = match self.conn.prepare("SELECT key, value FROM config WHERE key LIKE ?1") {
                    Ok(s) => s,
                    Err(_) => return map,
                };
                let rows = stmt.query_map([format!("{p}%")], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)));
                rows.and_then(Iterator::collect::<Result<Vec<(String, String)>, _>>)
            }
            None => {
                let mut stmt = match self.conn.prepare("SELECT key, value FROM config") {
                    Ok(s) => s,
                    Err(_) => return map,
                };
                let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)));
                rows.and_then(Iterator::collect::<Result<Vec<(String, String)>, _>>)
            }
        };
        if let Ok(pairs) = query_result {
            map.extend(pairs);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, ConfigDb) {
        let dir = tempfile::tempdir().unwrap();
        let db = ConfigDb::open(&dir.path().join("config.sqlite"), &dir.path().join("configdb.key")).unwrap();
        (dir, db)
    }

    #[test]
    fn set_then_get_roundtrip() {
        let (_dir, mut db) = fixture();
        assert!(db.set("a.b", "hello", false));
        assert_eq!(db.get("a.b", None, false), Some("hello".to_string()));
    }

    #[test]
    fn get_missing_key_returns_default() {
        let (_dir, db) = fixture();
        assert_eq!(db.get("missing", Some("fallback"), false), Some("fallback".to_string()));
        assert_eq!(db.get("missing", None, false), None);
    }

    #[test]
    fn set_secure_encrypts_and_get_secure_decrypts() {
        let (_dir, mut db) = fixture();
        assert!(db.set("secret.key", "s3cr3t", true));
        // Raw stored value should not equal the plaintext.
        assert_ne!(db.raw_get("secret.key"), Some("s3cr3t".to_string()));
        assert_eq!(db.get("secret.key", None, true), Some("s3cr3t".to_string()));
    }

    #[test]
    fn set_skips_write_when_value_unchanged() {
        let (_dir, mut db) = fixture();
        assert!(db.set("a.b", "same", false));
        let before = db.raw_get("a.b");
        assert!(db.set("a.b", "same", false));
        assert_eq!(db.raw_get("a.b"), before);
    }

    #[test]
    fn set_secure_skips_write_when_decrypted_value_unchanged() {
        let (_dir, mut db) = fixture();
        assert!(db.set("secret.key", "value1", true));
        let before = db.raw_get("secret.key");
        assert!(db.set("secret.key", "value1", true));
        assert_eq!(db.raw_get("secret.key"), before);
    }

    #[test]
    fn delete_removes_key() {
        let (_dir, mut db) = fixture();
        db.set("a.b", "x", false);
        assert!(db.delete("a.b"));
        assert_eq!(db.get("a.b", None, false), None);
    }

    #[test]
    fn list_keys_filters_by_prefix() {
        let (_dir, mut db) = fixture();
        db.set("app.one", "1", false);
        db.set("app.two", "2", false);
        db.set("other.three", "3", false);

        let mut keys = db.list_keys(Some("app."));
        keys.sort();
        assert_eq!(keys, vec!["app.one".to_string(), "app.two".to_string()]);
        assert_eq!(db.list_keys(None).len(), 3);
    }

    #[test]
    fn get_all_filters_by_prefix() {
        let (_dir, mut db) = fixture();
        db.set("app.one", "1", false);
        db.set("other.two", "2", false);

        let all = db.get_all(Some("app."));
        assert_eq!(all.len(), 1);
        assert_eq!(all.get("app.one"), Some(&"1".to_string()));
    }

    #[test]
    fn clear_all_removes_every_key() {
        let (_dir, mut db) = fixture();
        db.set("a", "1", false);
        db.set("b", "2", false);
        assert!(db.clear_all());
        assert!(db.list_keys(None).is_empty());
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let (_dir, db) = fixture();
        let encrypted = db.encrypt_value("plain text").unwrap();
        assert_ne!(encrypted, "plain text");
        assert_eq!(db.decrypt_value(&encrypted).unwrap(), "plain text");
    }

    #[test]
    fn decrypt_invalid_token_returns_error() {
        let (_dir, db) = fixture();
        assert!(db.decrypt_value("not-a-valid-token").is_err());
    }

    #[test]
    fn parse_bool_accepts_various_representations() {
        assert_eq!(parse_bool(&Value::Bool(true)), Ok(true));
        assert_eq!(parse_bool(&Value::from(1)), Ok(true));
        assert_eq!(parse_bool(&Value::from(0)), Ok(false));
        assert_eq!(parse_bool(&Value::String("yes".to_string())), Ok(true));
        assert_eq!(parse_bool(&Value::String("Off".to_string())), Ok(false));
        assert_eq!(parse_bool(&Value::String("".to_string())), Ok(false));
    }

    #[test]
    fn parse_bool_rejects_invalid_values() {
        assert!(parse_bool(&Value::from(2)).is_err());
        assert!(parse_bool(&Value::String("maybe".to_string())).is_err());
        assert!(parse_bool(&Value::Null).is_err());
    }
}
