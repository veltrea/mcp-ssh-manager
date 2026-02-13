use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use rusqlite::{Connection, params};
use rust_ssh::SecurityManager;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Machine {
    pub id: Option<i64>,
    pub name: String,
    pub ip_address: String,
    pub purpose: String,
    pub ownership: String, // "company", "personal"
    pub os_type: String,
    pub status: String, // "active", "broken", "maintenance"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Account {
    pub id: Option<i64>,
    pub machine_id: i64,
    pub username: String,
    pub auth_type: String,  // "password", "key"
    pub credential: String, // password or key path
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandLog {
    pub id: i64,
    pub machine_id: i64,
    pub machine_name: String,
    pub username: String,
    pub command: String,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub exit_code: Option<i32>,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Constraint {
    pub id: Option<i64>,
    pub machine_id: i64,
    pub rule_text: String,
}

pub struct DbHandler {
    path: PathBuf,
    security: SecurityManager,
    master_key: RwLock<[u8; 32]>,
}

impl DbHandler {
    pub fn new() -> Result<Self> {
        let path = Self::get_db_path()?;
        let conn = Connection::open(&path)?;
        let security = SecurityManager::new("mcp-ssh-manager");
        let master_key = security
            .get_or_create_master_key()
            .context("Failed to initialize master key from keyring")?;

        let handler = DbHandler {
            path,
            security,
            master_key: RwLock::new(master_key),
        };
        handler.init_schema(&conn)?;
        handler.migrate_credentials()?; // Phase 11 Task 5
        Ok(handler)
    }

    fn migrate_credentials(&self) -> Result<()> {
        let mut conn = self.get_conn()?;
        let tx = conn.transaction()?;

        let items: Vec<(i64, String)> = {
            let mut stmt = tx.prepare("SELECT id, credential FROM accounts")?;
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, rusqlite::Error>>()?
        };

        for (id, cred) in items {
            // If it can't be decrypted, it's likely plain text (or encrypted with another key - unlikely for now)
            let master_key = *self.master_key.read().unwrap();
            if self.security.decrypt(&master_key, &cred).is_err() {
                let encrypted = self.security.encrypt(&master_key, &cred)?;
                tx.execute(
                    "UPDATE accounts SET credential = ?1 WHERE id = ?2",
                    params![encrypted, id],
                )?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn get_conn(&self) -> Result<Connection> {
        Ok(Connection::open(&self.path)?)
    }

    fn get_db_path() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "veltrea", "mcp-ssh-manager")
            .ok_or_else(|| anyhow!("Could not determine project directories"))?;
        let db_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(db_dir)?;
        Ok(db_dir.join("manager.db"))
    }

    fn init_schema(&self, conn: &Connection) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS machines (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                ip_address TEXT NOT NULL,
                purpose TEXT NOT NULL,
                ownership TEXT NOT NULL,
                os_type TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active'
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                machine_id INTEGER NOT NULL,
                username TEXT NOT NULL,
                auth_type TEXT NOT NULL,
                credential TEXT NOT NULL,
                FOREIGN KEY(machine_id) REFERENCES machines(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS command_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                machine_id INTEGER NOT NULL,
                username TEXT NOT NULL,
                command TEXT NOT NULL,
                stdout TEXT,
                stderr TEXT,
                exit_code INTEGER,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(machine_id) REFERENCES machines(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS constraints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                machine_id INTEGER NOT NULL,
                rule_text TEXT NOT NULL,
                FOREIGN KEY(machine_id) REFERENCES machines(id)
            )",
            [],
        )?;
        Ok(())
    }

    pub fn add_machine(&self, machine: Machine) -> Result<i64> {
        let conn = self.get_conn()?;
        conn.execute(
            "INSERT INTO machines (name, ip_address, purpose, ownership, os_type, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                machine.name,
                machine.ip_address,
                machine.purpose,
                machine.ownership,
                machine.os_type,
                machine.status,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_machine(&self, id: i64) -> Result<()> {
        let conn = self.get_conn()?;

        // Delete associated accounts and constraints first
        conn.execute("DELETE FROM accounts WHERE machine_id = ?1", params![id])?;
        conn.execute("DELETE FROM constraints WHERE machine_id = ?1", params![id])?;
        conn.execute("DELETE FROM machines WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn add_account(&self, mut account: Account) -> Result<i64> {
        // Encrypt the credential before saving
        let encrypted = {
            let key = self.master_key.read().unwrap();
            self.security.encrypt(&key, &account.credential)?
        };
        account.credential = encrypted;

        let conn = self.get_conn()?;
        conn.execute(
            "INSERT INTO accounts (machine_id, username, auth_type, credential)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                account.machine_id,
                account.username,
                account.auth_type,
                account.credential,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_machines(&self) -> Result<Vec<Machine>> {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, ip_address, purpose, ownership, os_type, status FROM machines",
        )?;
        let machines = stmt
            .query_map([], |row| {
                Ok(Machine {
                    id: Some(row.get(0)?),
                    name: row.get(1)?,
                    ip_address: row.get(2)?,
                    purpose: row.get(3)?,
                    ownership: row.get(4)?,
                    os_type: row.get(5)?,
                    status: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(machines)
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        let conn = self.get_conn()?;
        let mut stmt =
            conn.prepare("SELECT id, machine_id, username, auth_type, credential FROM accounts")?;
        let accounts = stmt
            .query_map([], |row| {
                Ok(Account {
                    id: Some(row.get(0)?),
                    machine_id: row.get(1)?,
                    username: row.get(2)?,
                    auth_type: row.get(3)?,
                    // CRITICAL: AI Hiding Verification (Task 9).
                    // Do not decrypt or show raw/encrypted credential in general listing to prevent leakage.
                    credential: "[ENCRYPTED/RESTRICTED]".to_string(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(accounts)
    }

    pub fn update_account_credential(&self, account_id: i64, new_credential: &str) -> Result<()> {
        let encrypted = {
            let key = self.master_key.read().unwrap();
            self.security.encrypt(&key, new_credential)?
        };

        let conn = self.get_conn()?;
        conn.execute(
            "UPDATE accounts SET credential = ?1 WHERE id = ?2",
            params![encrypted, account_id],
        )?;
        Ok(())
    }

    pub fn list_logs(&self) -> Result<Vec<CommandLog>> {
        let conn = self.get_conn()?;
        let mut stmt = conn.prepare(
            "SELECT l.id, l.machine_id, m.name, l.username, l.command, l.stdout, l.stderr, l.exit_code, l.timestamp 
             FROM command_logs l
             JOIN machines m ON l.machine_id = m.id
             ORDER BY l.timestamp DESC"
        )?;
        let logs = stmt
            .query_map([], |row| {
                Ok(CommandLog {
                    id: row.get(0)?,
                    machine_id: row.get(1)?,
                    machine_name: row.get(2)?,
                    username: row.get(3)?,
                    command: row.get(4)?,
                    stdout: row.get(5)?,
                    stderr: row.get(6)?,
                    exit_code: row.get(7)?,
                    timestamp: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    pub fn get_constraints(&self, machine_id: i64) -> Result<Vec<Constraint>> {
        let conn = self.get_conn()?;
        let mut stmt = conn
            .prepare("SELECT id, machine_id, rule_text FROM constraints WHERE machine_id = ?1")?;
        let rules = stmt
            .query_map(params![machine_id], |row| {
                Ok(Constraint {
                    id: Some(row.get(0)?),
                    machine_id: row.get(1)?,
                    rule_text: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<Constraint>, _>>()?;
        Ok(rules)
    }

    pub fn delete_constraint(&self, id: i64) -> Result<()> {
        let conn = self.get_conn()?;
        conn.execute("DELETE FROM constraints WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn backup_db(&self, backup_path: &std::path::Path) -> Result<()> {
        let conn = self.get_conn()?;
        conn.backup(rusqlite::DatabaseName::Main, backup_path, None)?;
        Ok(())
    }

    pub fn rotate_keys(&self) -> Result<()> {
        let mut conn = self.get_conn()?;
        let tx = conn.transaction()?;

        // 1. Fetch all accounts
        let items: Vec<(i64, String)> = {
            let mut stmt = tx.prepare("SELECT id, credential FROM accounts")?;
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, rusqlite::Error>>()?
        };

        // 2. Generate NEW key
        let new_key = self.security.generate_new_master_key();

        // 3. Re-encrypt all credentials
        {
            let old_key = self.master_key.read().unwrap();
            for (id, old_cred) in items {
                // Decrypt with OLD key
                let plaintext = self
                    .security
                    .decrypt(&old_key, &old_cred)
                    .context(format!("Failed to decrypt credential for account {}", id))?;

                // Encrypt with NEW key
                let new_cred = self.security.encrypt(&new_key, &plaintext)?;

                // Update DB (in transaction)
                tx.execute(
                    "UPDATE accounts SET credential = ?1 WHERE id = ?2",
                    params![new_cred, id],
                )?;
            }
        }

        // 4. Commit DB Transaction
        tx.commit()
            .context("Failed to commit database transaction during key rotation")?;

        // 5. Update Keyring
        let hex_key = hex::encode(new_key);
        if let Err(e) = self.security.store_secret("master_key", &hex_key) {
            eprintln!("CRITICAL: Database rotated but Keyring update failed!");
            eprintln!("NEW KEY HEX: {}", hex_key);
            return Err(anyhow!("Keyring update failed: {}", e));
        }

        // 6. Update Memory
        let mut key_guard = self.master_key.write().unwrap();
        *key_guard = new_key;

        Ok(())
    }
}
