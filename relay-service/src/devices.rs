use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::{OsRng, RngCore};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

const SESSION_DURATION_SECONDS: i64 = 60 * 60 * 24 * 14;

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub device_id: String,
    pub device_name: String,
    pub paired: bool,
    pub revoked: bool,
    pub owner_user_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub email: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceListItem {
    pub device_id: String,
    pub device_name: String,
    pub paired: bool,
    pub connected: bool,
}

#[derive(Debug, Deserialize)]
pub struct PairingStartRequest {
    #[serde(rename = "deviceId")]
    pub device_id: String,
    #[serde(rename = "deviceName")]
    pub device_name: String,
    #[serde(rename = "deviceSecret")]
    pub device_secret: String,
    #[serde(rename = "pairingCode")]
    pub pairing_code: String,
}

#[derive(Debug, Deserialize)]
pub struct ConfirmPairingRequest {
    #[serde(rename = "pairingCode")]
    pub pairing_code: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthRequest {
    pub email: String,
    pub password: String,
}

pub struct DeviceStore {
    connection: Mutex<Connection>,
}

impl DeviceStore {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, DeviceStoreError> {
        let connection = Connection::open(path).map_err(DeviceStoreError::Database)?;
        connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS users (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    email TEXT NOT NULL UNIQUE,
                    password_hash TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    user_id INTEGER NOT NULL,
                    expires_at INTEGER NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    FOREIGN KEY(user_id) REFERENCES users(id)
                );
                CREATE TABLE IF NOT EXISTS devices (
                    device_id TEXT PRIMARY KEY,
                    device_name TEXT NOT NULL,
                    device_secret TEXT NOT NULL,
                    pairing_code TEXT NOT NULL,
                    paired INTEGER NOT NULL DEFAULT 0,
                    revoked INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_devices_pairing_code
                    ON devices(pairing_code);
                "#,
            )
            .map_err(DeviceStoreError::Database)?;

        ensure_column(
            &connection,
            "devices",
            "owner_user_id",
            "INTEGER REFERENCES users(id)",
        )?;

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn start_pairing(&self, request: &PairingStartRequest) -> Result<(), DeviceStoreError> {
        validate_public_id("device id", &request.device_id)?;
        validate_required("device name", &request.device_name)?;
        validate_required("device secret", &request.device_secret)?;
        validate_required("pairing code", &request.pairing_code)?;

        let connection = self.connection.lock().expect("device store lock poisoned");
        connection
            .execute(
                r#"
                INSERT INTO devices (
                    device_id,
                    device_name,
                    device_secret,
                    pairing_code,
                    paired,
                    revoked,
                    owner_user_id,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, 0, 0, NULL, CURRENT_TIMESTAMP)
                ON CONFLICT(device_id) DO UPDATE SET
                    device_name = excluded.device_name,
                    device_secret = excluded.device_secret,
                    pairing_code = excluded.pairing_code,
                    paired = 0,
                    revoked = 0,
                    owner_user_id = NULL,
                    updated_at = CURRENT_TIMESTAMP
                "#,
                params![
                    request.device_id,
                    request.device_name.trim(),
                    request.device_secret,
                    request.pairing_code.trim().to_ascii_uppercase()
                ],
            )
            .map_err(DeviceStoreError::Database)?;

        Ok(())
    }

    pub fn confirm_pairing(
        &self,
        pairing_code: &str,
        user_id: i64,
    ) -> Result<Device, DeviceStoreError> {
        validate_required("pairing code", pairing_code)?;
        let normalized_code = pairing_code.trim().to_ascii_uppercase();
        let connection = self.connection.lock().expect("device store lock poisoned");
        let affected = connection
            .execute(
                r#"
                UPDATE devices
                SET paired = 1,
                    revoked = 0,
                    owner_user_id = ?2,
                    updated_at = CURRENT_TIMESTAMP
                WHERE pairing_code = ?1
                "#,
                params![normalized_code, user_id],
            )
            .map_err(DeviceStoreError::Database)?;

        if affected == 0 {
            return Err(DeviceStoreError::NotFound(
                "Pairing code was not found.".to_string(),
            ));
        }

        Self::device_with_connection(
            &connection,
            &device_id_for_pairing(&connection, &normalized_code)?,
        )
    }

    pub fn device(&self, device_id: &str) -> Result<Device, DeviceStoreError> {
        validate_public_id("device id", device_id)?;
        let connection = self.connection.lock().expect("device store lock poisoned");
        Self::device_with_connection(&connection, device_id)
    }

    pub fn owned_device(&self, device_id: &str, user_id: i64) -> Result<Device, DeviceStoreError> {
        let device = self.device(device_id)?;
        if !device.paired || device.revoked {
            return Err(DeviceStoreError::NotFound(
                "Remote device is not available.".to_string(),
            ));
        }
        if device.owner_user_id != Some(user_id) {
            return Err(DeviceStoreError::Unauthorized(
                "You do not have access to this device.".to_string(),
            ));
        }
        Ok(device)
    }

    pub fn list_devices_for_user(
        &self,
        user_id: i64,
    ) -> Result<Vec<DeviceListItem>, DeviceStoreError> {
        let connection = self.connection.lock().expect("device store lock poisoned");
        let mut statement = connection
            .prepare(
                r#"
                SELECT device_id, device_name, paired
                FROM devices
                WHERE owner_user_id = ?1 AND revoked = 0
                ORDER BY updated_at DESC, device_name ASC
                "#,
            )
            .map_err(DeviceStoreError::Database)?;
        let rows = statement
            .query_map(params![user_id], |row| {
                Ok(DeviceListItem {
                    device_id: row.get(0)?,
                    device_name: row.get(1)?,
                    paired: row.get::<_, i64>(2)? != 0,
                    connected: false,
                })
            })
            .map_err(DeviceStoreError::Database)?;

        let mut devices = Vec::new();
        for row in rows {
            devices.push(row.map_err(DeviceStoreError::Database)?);
        }
        Ok(devices)
    }

    pub fn create_user(&self, request: &AuthRequest) -> Result<User, DeviceStoreError> {
        let email = normalize_email(&request.email)?;
        validate_password(&request.password)?;
        let password_hash = hash_password(&request.password)?;

        let connection = self.connection.lock().expect("device store lock poisoned");
        connection
            .execute(
                r#"
                INSERT INTO users (email, password_hash, updated_at)
                VALUES (?1, ?2, CURRENT_TIMESTAMP)
                "#,
                params![email, password_hash],
            )
            .map_err(map_user_insert_error)?;

        let user_id = connection.last_insert_rowid();
        Ok(User { id: user_id, email })
    }

    pub fn authenticate_user(&self, request: &AuthRequest) -> Result<User, DeviceStoreError> {
        let email = normalize_email(&request.email)?;
        validate_password(&request.password)?;

        let connection = self.connection.lock().expect("device store lock poisoned");
        let row = connection
            .query_row(
                "SELECT id, email, password_hash FROM users WHERE email = ?1",
                params![email],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(DeviceStoreError::Database)?
            .ok_or_else(|| {
                DeviceStoreError::Unauthorized("Invalid email or password.".to_string())
            })?;

        let parsed_hash = PasswordHash::new(&row.2).map_err(|_| {
            DeviceStoreError::Unauthorized("Invalid email or password.".to_string())
        })?;
        Argon2::default()
            .verify_password(request.password.as_bytes(), &parsed_hash)
            .map_err(|_| {
                DeviceStoreError::Unauthorized("Invalid email or password.".to_string())
            })?;

        Ok(User {
            id: row.0,
            email: row.1,
        })
    }

    pub fn create_session(&self, user_id: i64) -> Result<String, DeviceStoreError> {
        let session_id = random_token(32);
        let expires_at = now_unix() + SESSION_DURATION_SECONDS;
        let connection = self.connection.lock().expect("device store lock poisoned");
        connection
            .execute(
                "INSERT INTO sessions (id, user_id, expires_at) VALUES (?1, ?2, ?3)",
                params![session_id, user_id, expires_at],
            )
            .map_err(DeviceStoreError::Database)?;
        Ok(session_id)
    }

    pub fn user_for_session(&self, session_id: &str) -> Result<User, DeviceStoreError> {
        validate_required("session", session_id)?;
        let connection = self.connection.lock().expect("device store lock poisoned");
        connection
            .execute(
                "DELETE FROM sessions WHERE expires_at <= ?1",
                params![now_unix()],
            )
            .map_err(DeviceStoreError::Database)?;
        connection
            .query_row(
                r#"
                SELECT users.id, users.email
                FROM sessions
                JOIN users ON users.id = sessions.user_id
                WHERE sessions.id = ?1 AND sessions.expires_at > ?2
                "#,
                params![session_id, now_unix()],
                |row| {
                    Ok(User {
                        id: row.get(0)?,
                        email: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(DeviceStoreError::Database)?
            .ok_or_else(|| DeviceStoreError::Unauthorized("You must sign in first.".to_string()))
    }

    pub fn delete_session(&self, session_id: &str) -> Result<(), DeviceStoreError> {
        let connection = self.connection.lock().expect("device store lock poisoned");
        connection
            .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
            .map_err(DeviceStoreError::Database)?;
        Ok(())
    }

    pub fn authenticate(
        &self,
        device_id: &str,
        device_secret: &str,
    ) -> Result<(), DeviceStoreError> {
        validate_public_id("device id", device_id)?;
        validate_required("device secret", device_secret)?;

        let connection = self.connection.lock().expect("device store lock poisoned");
        let row = connection
            .query_row(
                r#"
                SELECT device_secret, paired, revoked
                FROM devices
                WHERE device_id = ?1
                "#,
                params![device_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)? != 0,
                        row.get::<_, i64>(2)? != 0,
                    ))
                },
            )
            .optional()
            .map_err(DeviceStoreError::Database)?
            .ok_or_else(|| DeviceStoreError::Unauthorized("Unknown device.".to_string()))?;

        let (stored_secret, paired, revoked) = row;
        if revoked {
            return Err(DeviceStoreError::Unauthorized(
                "Device has been revoked.".to_string(),
            ));
        }
        if !paired {
            return Err(DeviceStoreError::Unauthorized(
                "Device is not paired yet.".to_string(),
            ));
        }
        if stored_secret != device_secret {
            return Err(DeviceStoreError::Unauthorized(
                "Invalid device secret.".to_string(),
            ));
        }

        Ok(())
    }

    fn device_with_connection(
        connection: &Connection,
        device_id: &str,
    ) -> Result<Device, DeviceStoreError> {
        connection
            .query_row(
                r#"
                SELECT device_id, device_name, paired, revoked, owner_user_id
                FROM devices
                WHERE device_id = ?1
                "#,
                params![device_id],
                |row| {
                    Ok(Device {
                        device_id: row.get(0)?,
                        device_name: row.get(1)?,
                        paired: row.get::<_, i64>(2)? != 0,
                        revoked: row.get::<_, i64>(3)? != 0,
                        owner_user_id: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(DeviceStoreError::Database)?
            .ok_or_else(|| DeviceStoreError::NotFound("Device was not found.".to_string()))
    }
}

fn device_id_for_pairing(
    connection: &Connection,
    pairing_code: &str,
) -> Result<String, DeviceStoreError> {
    connection
        .query_row(
            "SELECT device_id FROM devices WHERE pairing_code = ?1",
            params![pairing_code],
            |row| row.get(0),
        )
        .map_err(DeviceStoreError::Database)
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), DeviceStoreError> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut statement = connection
        .prepare(&pragma)
        .map_err(DeviceStoreError::Database)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(DeviceStoreError::Database)?;
    for existing in columns {
        if existing.map_err(DeviceStoreError::Database)? == column {
            return Ok(());
        }
    }
    connection
        .execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(DeviceStoreError::Database)?;
    Ok(())
}

fn hash_password(password: &str) -> Result<String, DeviceStoreError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| DeviceStoreError::PasswordHash(error.to_string()))
}

fn random_token(byte_count: usize) -> String {
    let mut bytes = vec![0_u8; byte_count];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn normalize_email(email: &str) -> Result<String, DeviceStoreError> {
    let email = email.trim().to_ascii_lowercase();
    if email.is_empty() || !email.contains('@') || email.len() > 254 {
        return Err(DeviceStoreError::InvalidInput(
            "Enter a valid email address.".to_string(),
        ));
    }
    Ok(email)
}

fn validate_password(password: &str) -> Result<(), DeviceStoreError> {
    if password.len() < 8 {
        return Err(DeviceStoreError::InvalidInput(
            "Password must be at least 8 characters.".to_string(),
        ));
    }
    Ok(())
}

fn validate_required(label: &str, value: &str) -> Result<(), DeviceStoreError> {
    if value.trim().is_empty() {
        return Err(DeviceStoreError::InvalidInput(format!(
            "{label} is required."
        )));
    }
    Ok(())
}

fn validate_public_id(label: &str, value: &str) -> Result<(), DeviceStoreError> {
    validate_required(label, value)?;
    if value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(DeviceStoreError::InvalidInput(format!(
            "{label} can only contain letters, numbers, dashes, and underscores."
        )));
    }
    Ok(())
}

fn map_user_insert_error(error: rusqlite::Error) -> DeviceStoreError {
    let message = error.to_string();
    if message.contains("UNIQUE constraint failed: users.email") {
        DeviceStoreError::InvalidInput("An account with that email already exists.".to_string())
    } else {
        DeviceStoreError::Database(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceStoreError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    PasswordHash(String),
    #[error("{0}")]
    Database(rusqlite::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> DeviceStore {
        DeviceStore::open(":memory:").expect("store opens")
    }

    #[test]
    fn pairing_flow_authenticates_confirmed_device() {
        let store = store();
        let user = store
            .create_user(&AuthRequest {
                email: "user@example.com".into(),
                password: "password123".into(),
            })
            .expect("user created");
        store
            .start_pairing(&PairingStartRequest {
                device_id: "device_1".into(),
                device_name: "Desktop".into(),
                device_secret: "secret".into(),
                pairing_code: "ABCDE-12345".into(),
            })
            .expect("pairing starts");

        assert!(store.authenticate("device_1", "secret").is_err());

        let device = store
            .confirm_pairing("abcde-12345", user.id)
            .expect("pairing confirms");
        assert_eq!(device.device_id, "device_1");
        assert_eq!(device.owner_user_id, Some(user.id));
        store
            .authenticate("device_1", "secret")
            .expect("authenticates after confirm");
    }

    #[test]
    fn invalid_device_ids_are_rejected() {
        let store = store();
        let error = store
            .start_pairing(&PairingStartRequest {
                device_id: "../bad".into(),
                device_name: "Desktop".into(),
                device_secret: "secret".into(),
                pairing_code: "ABCDE".into(),
            })
            .expect_err("invalid id rejected");

        assert!(matches!(error, DeviceStoreError::InvalidInput(_)));
    }

    #[test]
    fn session_round_trip_loads_user() {
        let store = store();
        let user = store
            .create_user(&AuthRequest {
                email: "user@example.com".into(),
                password: "password123".into(),
            })
            .expect("user created");
        let session = store.create_session(user.id).expect("session created");
        let session_user = store.user_for_session(&session).expect("session user");
        assert_eq!(session_user.email, "user@example.com");
    }

    #[test]
    fn authenticate_user_rejects_wrong_password() {
        let store = store();
        store
            .create_user(&AuthRequest {
                email: "user@example.com".into(),
                password: "password123".into(),
            })
            .expect("user created");
        let error = store
            .authenticate_user(&AuthRequest {
                email: "user@example.com".into(),
                password: "wrongpass".into(),
            })
            .expect_err("wrong password rejected");
        assert!(matches!(error, DeviceStoreError::Unauthorized(_)));
    }
}
