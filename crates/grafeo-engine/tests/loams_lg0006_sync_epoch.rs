//! LG-0006 (Loams fork, PATCHES.md): `GrafeoDB::sync_epoch` moves a database's epoch forward. A
//! caller that rebuilds a database by direct store writes (which take no epoch) from its own
//! log uses it to continue that log's epochs (Loams GR1 ruling R9.5).
//!
//! ```bash
//! cargo test -p grafeo-engine --test loams_lg0006_sync_epoch
//! ```

#![cfg(feature = "gql")]

use grafeo_common::types::EpochId;
use grafeo_engine::GrafeoDB;

#[test]
fn sync_epoch_carries_a_replayed_database_forward() {
    let db = GrafeoDB::new_in_memory();
    // A replay: direct store writes, outside any transaction.
    let a = db.create_node(&["P"]);
    let b = db.create_node(&["P"]);
    db.create_edge(a, b, "R");
    assert_eq!(db.current_epoch(), EpochId::new(0));

    db.sync_epoch(EpochId::new(41)).expect("sync");
    assert_eq!(db.current_epoch(), EpochId::new(41));
    let session = db.session();
    let count = session
        .execute("MATCH (n:P) RETURN count(n)")
        .expect("count");
    assert_eq!(count.rows()[0][0], grafeo_common::types::Value::Int64(2));

    session.execute("INSERT (:P)").expect("insert");
    assert_eq!(
        db.current_epoch(),
        EpochId::new(42),
        "the next commit follows the log"
    );
    assert_eq!(db.node_count(), 3);

    // Never backwards.
    db.sync_epoch(EpochId::new(7)).expect("sync");
    assert_eq!(db.current_epoch(), EpochId::new(42));
}
