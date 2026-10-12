//! LG-0006 (Loams fork, PATCHES.md): the epoch survives a reopen. On upstream Grafeo 0.5.43
//! without the `temporal` feature a reopened database (WAL replay, checkpoint snapshot or an
//! imported snapshot) restarts at epoch 0, so epochs repeat across a restart (Loams GR1 ruling
//! R9.5).
//!
//! With `temporal`, WAL replay also advances the epoch once per replayed commit record, which
//! can land past the last committed epoch; there the guarantee is that epochs never repeat
//! ([`assert_continues`]).
//!
//! ```bash
//! cargo test -p grafeo-engine --test loams_lg0006_epoch_reopen
//! ```

#![cfg(all(feature = "gql", feature = "wal", feature = "grafeo-file"))]

use std::path::Path;

use grafeo_common::types::EpochId;
use grafeo_engine::config::{DurabilityMode, StorageFormat};
use grafeo_engine::{Config, GrafeoDB};

fn commit(db: &GrafeoDB, n: i64) {
    for i in 0..n {
        db.session()
            .execute(&format!("INSERT (:P {{i: {i}}})"))
            .expect("insert");
    }
}

fn config(path: &Path, format: StorageFormat) -> Config {
    Config::persistent(path)
        .with_storage_format(format)
        .with_wal_durability(DurabilityMode::Sync)
}

/// The reopened database's epoch: exactly the last committed one, or with `temporal` at least it.
fn assert_continues(actual: EpochId, committed: u64) {
    if cfg!(feature = "temporal") {
        assert!(
            actual >= EpochId::new(committed),
            "{actual:?} < {committed}"
        );
    } else {
        assert_eq!(actual, EpochId::new(committed), "the epoch survives");
    }
}

fn reopen_keeps_the_epoch(format: StorageFormat, name: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(name);
    {
        let db = GrafeoDB::with_config(config(&path, format)).expect("open");
        commit(&db, 3);
        assert_eq!(db.current_epoch(), EpochId::new(3));
        db.close().expect("close");
    }
    let db = GrafeoDB::with_config(config(&path, format)).expect("reopen");
    assert_eq!(db.node_count(), 3);
    assert_continues(db.current_epoch(), 3);
    let reopened = db.current_epoch();
    commit(&db, 1);
    assert_eq!(
        db.current_epoch(),
        EpochId::new(reopened.as_u64() + 1),
        "and continues"
    );
    assert_eq!(db.node_count(), 4);
}

#[test]
fn epoch_survives_reopen_of_a_wal_directory() {
    reopen_keeps_the_epoch(StorageFormat::WalDirectory, "db");
}

#[test]
fn epoch_survives_reopen_of_a_single_file() {
    reopen_keeps_the_epoch(StorageFormat::SingleFile, "db.grafeo");
}

#[test]
fn epoch_survives_a_read_only_open_of_a_single_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("db.grafeo");
    {
        let db = GrafeoDB::with_config(config(&path, StorageFormat::SingleFile)).expect("open");
        commit(&db, 3);
        db.close().expect("close");
    }
    let db = GrafeoDB::open_read_only(&path).expect("read-only");
    assert_eq!(db.node_count(), 3);
    assert_continues(db.current_epoch(), 3);
}

/// A crash: the database is never closed, and a copy of its directory as it is on disk is
/// opened, so the epoch has to come back from WAL replay.
#[test]
fn epoch_survives_wal_replay_after_a_crash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("db");
    let db = GrafeoDB::with_config(config(&path, StorageFormat::WalDirectory)).expect("open");
    commit(&db, 3);
    let copy = dir.path().join("copy");
    copy_dir(&path, &copy);
    std::mem::forget(db);

    let db = GrafeoDB::with_config(config(&copy, StorageFormat::WalDirectory)).expect("recover");
    assert_eq!(db.node_count(), 3);
    assert_continues(db.current_epoch(), 3);
    let recovered = db.current_epoch();
    commit(&db, 1);
    assert_eq!(db.current_epoch(), EpochId::new(recovered.as_u64() + 1));
}

#[test]
fn epoch_survives_export_and_import_of_a_snapshot() {
    let db = GrafeoDB::new_in_memory();
    commit(&db, 5);
    let bytes = db.export_snapshot().expect("export");
    let imported = GrafeoDB::import_snapshot(&bytes).expect("import");
    assert_eq!(imported.current_epoch(), EpochId::new(5));

    let restored = GrafeoDB::new_in_memory();
    restored.restore_snapshot(&bytes).expect("restore");
    assert_eq!(restored.current_epoch(), EpochId::new(5));
    commit(&restored, 1);
    assert_eq!(restored.current_epoch(), EpochId::new(6));
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}
