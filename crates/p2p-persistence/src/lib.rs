#![forbid(unsafe_code)]

mod backup;
mod error;
mod hash;
mod model;
mod schema;
mod store;

pub use error::{PersistenceError, Result};
pub use model::{
    AnnotationInput, BackupEntry, BackupManifest, BackupOutcome, CatalogPairInput, ClearOutcome,
    ClearScope, CostProfileInput, CostVersionOutcome, NamedViewInput, PruneOutcome,
    PublicationInput, PublicationOutcome, RestoreOutcome, RetentionPolicy, RollupInput,
    RollupPeriod, RuntimeVersions, SnapshotContext, StoredAdVersion, StoredSnapshot, SummaryInput,
};
pub use schema::{
    DATABASE_FILE_NAME, DATABASE_SCHEMA_VERSION, DEFAULT_DETAIL_RETENTION_MS,
    DEFAULT_MANAGED_CAP_BYTES, DEFAULT_ROLLUP_RETENTION_MS, DEFAULT_SUMMARY_RETENTION_MS,
    IDENTITY_KEY_FILE_NAME,
};
pub use store::PersistenceStore;
