use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, MAIN_DB};
use sha2::{Digest, Sha256};
use tempfile::{NamedTempFile, TempDir};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{PersistenceError, Result};
use crate::hash::sha256_bytes;
use crate::model::{BackupEntry, BackupManifest, BackupOutcome, RuntimeVersions};
use crate::schema::{AUTOMATIC_BACKUP_LIMIT, DATABASE_SCHEMA_VERSION};

const DATABASE_ENTRY: &str = "database.sqlite3";
const IDENTITY_KEY_ENTRY: &str = "identity.key";
const MANIFEST_ENTRY: &str = "manifest.json";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_DATABASE_BACKUP_BYTES: u64 = 16 * 1024 * 1024 * 1024;

pub(crate) struct ValidatedBackup {
    _directory: TempDir,
    pub database_path: PathBuf,
    pub identity_key_path: PathBuf,
    pub manifest: BackupManifest,
}

pub(crate) fn create_backup_archive(
    connection: &Connection,
    identity_key_path: &Path,
    destination: &Path,
    created_at_ms: i64,
    versions: &RuntimeVersions,
) -> Result<BackupOutcome> {
    if destination.exists() {
        return Err(PersistenceError::InvalidInput(
            "backup destination already exists".to_owned(),
        ));
    }
    let parent = destination.parent().ok_or_else(|| {
        PersistenceError::InvalidInput("backup destination requires a parent directory".to_owned())
    })?;
    fs::create_dir_all(parent)?;

    let temp_database = NamedTempFile::new_in(parent)?;
    connection
        .backup(MAIN_DB, temp_database.path(), None)
        .map_err(PersistenceError::from)?;
    temp_database.as_file().sync_all()?;

    let (database_size, database_sha256) = hash_file(temp_database.path())?;
    let identity_key = fs::read(identity_key_path)?;
    if identity_key.len() != 32 {
        return Err(PersistenceError::InvalidIdentityKey);
    }

    let entries = vec![
        BackupEntry {
            name: DATABASE_ENTRY.to_owned(),
            size_bytes: database_size,
            sha256: database_sha256,
        },
        BackupEntry {
            name: IDENTITY_KEY_ENTRY.to_owned(),
            size_bytes: identity_key.len() as u64,
            sha256: sha256_bytes(&identity_key),
        },
    ];
    let schema_version = schema_version(connection)?;
    let manifest = BackupManifest::new(created_at_ms, schema_version, versions.clone(), entries);
    manifest.validate()?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;

    let mut temp_archive = NamedTempFile::new_in(parent)?;
    {
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o600);
        let mut archive = ZipWriter::new(temp_archive.as_file_mut());
        archive.start_file(DATABASE_ENTRY, options)?;
        std::io::copy(&mut File::open(temp_database.path())?, &mut archive)?;
        archive.start_file(IDENTITY_KEY_ENTRY, options)?;
        archive.write_all(&identity_key)?;
        archive.start_file(MANIFEST_ENTRY, options)?;
        archive.write_all(&manifest_bytes)?;
        archive.finish()?;
    }
    temp_archive.as_file().sync_all()?;
    let persisted = temp_archive
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    persisted.sync_all()?;
    sync_parent_directory(destination)?;

    let (archive_size, archive_sha256) = hash_file(destination)?;
    Ok(BackupOutcome {
        path: destination.to_string_lossy().into_owned(),
        sha256: archive_sha256,
        size_bytes: archive_size,
        manifest,
    })
}

pub(crate) fn validate_and_extract_backup(
    archive_path: &Path,
    temporary_root: &Path,
) -> Result<ValidatedBackup> {
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    if archive.len() != 3 {
        return Err(PersistenceError::InvalidBackup(
            "archive must contain exactly the database, identity key, and manifest".to_owned(),
        ));
    }

    fs::create_dir_all(temporary_root)?;
    let directory = tempfile::Builder::new()
        .prefix("restore-validate-")
        .tempdir_in(temporary_root)?;
    let database_path = directory.path().join(DATABASE_ENTRY);
    let identity_key_path = directory.path().join(IDENTITY_KEY_ENTRY);
    let manifest_path = directory.path().join(MANIFEST_ENTRY);
    let mut seen = BTreeSet::new();

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        if !seen.insert(name.clone()) {
            return Err(PersistenceError::InvalidBackup(
                "archive contains a duplicate member".to_owned(),
            ));
        }
        let (path, limit) = match name.as_str() {
            DATABASE_ENTRY => (&database_path, MAX_DATABASE_BACKUP_BYTES),
            IDENTITY_KEY_ENTRY => (&identity_key_path, 32),
            MANIFEST_ENTRY => (&manifest_path, MAX_MANIFEST_BYTES),
            _ => {
                return Err(PersistenceError::InvalidBackup(
                    "archive contains an unexpected member".to_owned(),
                ));
            }
        };
        if entry.is_dir() || entry.size() > limit {
            return Err(PersistenceError::InvalidBackup(
                "archive member has an invalid kind or size".to_owned(),
            ));
        }
        let mut output = File::create(path)?;
        let copied = std::io::copy(
            &mut entry.by_ref().take(limit.saturating_add(1)),
            &mut output,
        )?;
        if copied > limit || copied != entry.size() {
            return Err(PersistenceError::InvalidBackup(
                "archive member size is inconsistent".to_owned(),
            ));
        }
        output.sync_all()?;
    }

    let manifest_bytes = fs::read(&manifest_path)?;
    let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes)?;
    manifest.validate()?;

    let expected = manifest
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    if expected.len() != 2
        || !expected.contains_key(DATABASE_ENTRY)
        || !expected.contains_key(IDENTITY_KEY_ENTRY)
    {
        return Err(PersistenceError::InvalidBackup(
            "manifest entry inventory is incomplete or duplicated".to_owned(),
        ));
    }
    for (name, path) in [
        (DATABASE_ENTRY, database_path.as_path()),
        (IDENTITY_KEY_ENTRY, identity_key_path.as_path()),
    ] {
        let (size, hash) = hash_file(path)?;
        let expected_entry = expected[name];
        if size != expected_entry.size_bytes || hash != expected_entry.sha256 {
            return Err(PersistenceError::InvalidBackup(format!(
                "hash or size mismatch for {name}"
            )));
        }
    }
    if fs::metadata(&identity_key_path)?.len() != 32 {
        return Err(PersistenceError::InvalidIdentityKey);
    }

    Ok(ValidatedBackup {
        _directory: directory,
        database_path,
        identity_key_path,
        manifest,
    })
}

pub(crate) fn retain_latest_automatic_backups(directory: &Path) -> Result<()> {
    let mut backups = fs::read_dir(directory)?
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let name = entry.file_name().into_string().ok()?;
            let timestamp = name
                .strip_prefix("migration-backup-")
                .or_else(|| name.strip_prefix("restore-safety-"))
                .and_then(|suffix| suffix.get(..20))
                .and_then(|value| value.parse::<i64>().ok());
            (file_type.is_file() && name.ends_with(".zip"))
                .then_some(timestamp)
                .flatten()
                .map(|timestamp| (timestamp, name, entry.path()))
        })
        .collect::<Vec<_>>();
    backups.sort_by(|left, right| (right.0, &right.1).cmp(&(left.0, &left.1)));
    for (_, _, path) in backups.into_iter().skip(AUTOMATIC_BACKUP_LIMIT) {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub(crate) fn automatic_backup_path(
    directory: &Path,
    created_at_ms: i64,
    old_schema_version: u32,
) -> PathBuf {
    directory.join(format!(
        "migration-backup-{created_at_ms:020}-schema-{old_schema_version}.zip"
    ))
}

fn sync_parent_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let parent = path.parent().ok_or_else(|| {
            PersistenceError::InvalidInput("backup path has no parent directory".to_owned())
        })?;
        File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(u64, String)> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        total = total.saturating_add(read as u64);
    }
    Ok((total, crate::schema::hex_lower(&digest.finalize())))
}

pub(crate) fn schema_version(connection: &Connection) -> Result<u32> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(PersistenceError::from)?;
    u32::try_from(version).map_err(|_| {
        PersistenceError::Incompatible(
            "database schema version is negative or too large".to_owned(),
        )
    })
}

pub(crate) fn ensure_supported_schema(version: u32) -> Result<()> {
    if version > DATABASE_SCHEMA_VERSION {
        return Err(PersistenceError::Incompatible(format!(
            "database schema {version} is newer than supported schema {DATABASE_SCHEMA_VERSION}"
        )));
    }
    Ok(())
}
