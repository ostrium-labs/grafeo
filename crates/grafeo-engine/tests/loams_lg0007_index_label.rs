//! LG-0007: an indexed equality lookup on a labelled scan checks the label per
//! candidate instead of collecting every node of the label.
//!
//! `MATCH (p:Person {uid: $id})` with a property index on `uid` looked the
//! candidates up in the index, then intersected them with a set built from
//! `nodes_by_label("Person")` on every query: one lookup cost O(persons)
//! (about 30 ms at 100,000 persons in Loams' BQ1 graph benchmark).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use grafeo_common::types::Value;
use grafeo_engine::GrafeoDB;

fn graph(persons: i64) -> GrafeoDB {
    let db = GrafeoDB::new_in_memory();
    let session = db.session();
    for uid in 0..persons {
        session
            .create_node_with_props(
                &["Person"],
                [
                    ("uid", Value::Int64(uid)),
                    ("name", Value::from(format!("p{uid}"))),
                ],
            )
            .unwrap();
    }
    // A node of another label with an indexed value a Person also has.
    session
        .create_node_with_props(
            &["Company"],
            [("uid", Value::Int64(1)), ("name", Value::from("c1"))],
        )
        .unwrap();
    session
        .execute("CREATE INDEX person_uid FOR (n:Person) ON (n.uid)")
        .unwrap();
    db
}

fn lookup(db: &GrafeoDB, uid: i64) -> Vec<Vec<Value>> {
    let mut params = HashMap::new();
    params.insert("id".to_string(), Value::Int64(uid));
    db.session()
        .execute_with_params("MATCH (p:Person {uid: $id}) RETURN p.name", params)
        .unwrap()
        .rows()
        .to_vec()
}

#[test]
fn indexed_lookup_keeps_only_the_scanned_label() {
    let db = graph(10);
    assert_eq!(lookup(&db, 1), vec![vec![Value::from("p1")]]);
    let rows = db
        .session()
        .execute("MATCH (c:Company {uid: 1}) RETURN c.name")
        .unwrap();
    assert_eq!(rows.rows().to_vec(), vec![vec![Value::from("c1")]]);
}

#[test]
fn indexed_lookup_sees_a_label_removed_in_the_transaction() {
    let db = graph(10);
    let mut session = db.session();
    session.begin_transaction().unwrap();
    session
        .execute("MATCH (p:Person {uid: 2}) REMOVE p:Person")
        .unwrap();
    let rows = session
        .execute("MATCH (p:Person {uid: 2}) RETURN p.name")
        .unwrap();
    assert_eq!(rows.row_count(), 0, "the transaction removed the label");
    session.rollback().unwrap();
    assert_eq!(lookup(&db, 2), vec![vec![Value::from("p2")]]);
}

/// Median time of `n` lookups.
fn median_lookup(db: &GrafeoDB, persons: i64, n: usize) -> Duration {
    let mut times: Vec<Duration> = (0..n)
        .map(|i| {
            let start = Instant::now();
            let rows = lookup(db, (i as i64 * 7919) % persons);
            assert_eq!(rows.len(), 1);
            start.elapsed()
        })
        .collect();
    times.sort();
    times[n / 2]
}

#[test]
fn indexed_lookup_cost_does_not_grow_with_the_label() {
    // 100 times more persons. Before LG-0007 the median lookup grew with
    // them (it built a set of every Person); now it stays flat. The bound is
    // loose (10x) so a busy machine does not fail it.
    let small = graph(500);
    let large = graph(50_000);
    let t_small = median_lookup(&small, 500, 201);
    let t_large = median_lookup(&large, 50_000, 201);
    assert!(
        t_large < t_small * 10,
        "lookup at 50,000 persons took {t_large:?}, at 500 {t_small:?}"
    );
}
