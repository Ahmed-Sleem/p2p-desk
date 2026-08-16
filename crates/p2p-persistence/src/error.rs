use rusqlite::ErrorCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("local persistence input is invalid: {0}")]
    InvalidInput(String),
    #[error("the acquisition is not a complete publishable two-side snapshot: {0}")]
    NotPublishable(String),
    #[error("the database is busy or locked")]
    Busy,
    #[error("the managed database or filesystem is full")]
    DiskFull,
    #[error("the SQLite database is corrupt or is not a database")]
    Corrupt,
    #[error("database integrity validation failed: {0}")]
    Integrity(String),
    #[error("the stored schema or backup is incompatible: {0}")]
    Incompatible(String),
    #[error("backup validation failed: {0}")]
    InvalidBackup(String),
    #[error("restore failed after replacement; the original database was restored: {0}")]
    RestoreRolledBack(String),
    #[error("restore failed and rollback could not restore the original database: {0}")]
    RestoreRollbackFailed(String),
    #[error("the pseudonymous identity key is missing or invalid")]
    InvalidIdentityKey,
    #[error("secure operating-system entropy is unavailable: {0}")]
    Entropy(String),
    #[error("an injected verification fault interrupted the operation")]
    FaultInjected,
    #[error("SQLite operation failed: {0}")]
    Sqlite(String),
    #[error("local filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP operation failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<rusqlite::Error> for PersistenceError {
    fn from(error: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(details, _) = &error {
            return match details.code {
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Self::Busy,
                ErrorCode::DiskFull => Self::DiskFull,
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => Self::Corrupt,
                _ => Self::Sqlite(error.to_string()),
            };
        }
        Self::Sqlite(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PersistenceError>;
