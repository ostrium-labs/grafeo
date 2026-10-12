//! LG-0001 (Loams fork, PATCHES.md): a commit listener hands each commit's change events to the
//! caller. These tests fail on upstream Grafeo 0.5.43 and pass with the patch.
//!
//! ```bash
//! cargo test -p grafeo-engine --features cdc --test loams_lg0001_commit_listener
//! ```

#![cfg(all(feature = "gql", feature = "cdc"))]

use grafeo_common::types::{EpochId, Value};
use grafeo_engine::{Config, GrafeoDB, Session};

fn rows(session: &Session, statement: &str) -> Vec<Vec<Value>> {
    session.execute(statement).expect(statement).into_rows()
}

mod commit_listener {
    use std::sync::Arc;

    use grafeo_engine::cdc::{CdcRetentionConfig, ChangeEvent, ChangeKind};
    use parking_lot::Mutex;

    use super::*;

    type Seen = Arc<Mutex<Vec<(EpochId, Vec<ChangeEvent>)>>>;

    fn listening(session: &Session) -> Seen {
        let seen: Seen = Arc::default();
        let sink = Arc::clone(&seen);
        session.set_commit_listener(Some(Arc::new(move |epoch, events: &[ChangeEvent]| {
            sink.lock().push((epoch, events.to_vec()));
        })));
        seen
    }

    #[test]
    fn listener_gets_each_commits_events_at_its_epoch() {
        let db = GrafeoDB::with_config(Config::in_memory().with_cdc()).expect("db");
        let mut session = db.session();
        let seen = listening(&session);

        session.begin_transaction().expect("begin");
        session.execute("INSERT (:P {k: 1})").expect("insert");
        session.execute("INSERT (:P {k: 2})").expect("insert");
        session.commit().expect("commit");
        // An autocommit statement commits through the same path.
        session
            .execute("MATCH (n:P {k: 1}) SET n.k = 10")
            .expect("set");
        // A rollback commits nothing and calls nothing.
        session.begin_transaction().expect("begin");
        session.execute("INSERT (:P {k: 3})").expect("insert");
        session.rollback().expect("rollback");

        let seen = seen.lock().clone();
        assert_eq!(seen.len(), 2, "{seen:?}");
        let (first, ref events) = seen[0];
        let creates = events
            .iter()
            .filter(|e| e.kind == ChangeKind::Create)
            .count();
        assert_eq!(creates, 2, "{events:?}");
        assert!(events.iter().all(|e| e.epoch == first), "{events:?}");
        // The listener's events are the ones the CDC log holds for that epoch.
        assert_eq!(
            db.changes_between(first, first).expect("cdc").len(),
            events.len()
        );
        let (second, ref events) = seen[1];
        assert!(second > first);
        assert!(!events.is_empty() && events.iter().all(|e| e.epoch == second));
        assert_eq!(second, db.current_epoch(), "the last commit's epoch");
    }

    /// The events reach the caller even when the CDC log prunes them before anyone reads it: the
    /// race R9.9 records (a capture that reads `changes_between` after the commit can find the
    /// commit's events gone).
    #[test]
    fn listener_events_do_not_depend_on_cdc_retention() {
        let mut config = Config::in_memory().with_cdc();
        config.cdc_retention = CdcRetentionConfig {
            max_epochs: None,
            max_events: Some(1),
        };
        let db = GrafeoDB::with_config(config).expect("db");
        let session = db.session();
        let seen = listening(&session);
        session
            .execute("INSERT (:P {k: 1})-[:R]->(:P {k: 2})")
            .expect("insert");
        db.gc();
        let seen = seen.lock().clone();
        let (epoch, ref events) = seen[0];
        assert!(
            db.changes_between(epoch, epoch).expect("cdc").len() < events.len(),
            "retention pruned the log"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == ChangeKind::Create)
                .count(),
            3,
            "{events:?}"
        );
    }

    /// A committed transaction that wrote nothing still reports its epoch (an explicit read-only
    /// transaction advances the epoch), and a session can stop listening.
    #[test]
    fn listener_sees_empty_commits_and_can_be_cleared() {
        let db = GrafeoDB::with_config(Config::in_memory().with_cdc()).expect("db");
        let mut session = db.session();
        let seen = listening(&session);
        session.begin_transaction().expect("begin");
        let _ = rows(&session, "MATCH (n) RETURN n");
        session.commit().expect("commit");
        session.set_commit_listener(None);
        session.execute("INSERT (:P)").expect("insert");
        let seen = seen.lock().clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert!(seen[0].1.is_empty());
        assert_eq!(seen[0].0, EpochId::new(1));
    }
}
