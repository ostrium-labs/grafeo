//! LG-0004 (Loams fork, PATCHES.md): `DETACH DELETE` deletes the edges the transaction itself
//! created, and `DELETE` refuses a node that still has one. On upstream Grafeo 0.5.43 a
//! transaction that inserts an edge and then deletes one of its endpoints commits the edge with
//! its endpoint gone (Loams GR1 ruling R9.8).
//!
//! ```bash
//! cargo test -p grafeo-engine --test loams_lg0004_detach_delete
//! ```

#![cfg(feature = "gql")]

use grafeo_common::types::Value;
use grafeo_engine::{GrafeoDB, Session};

fn count(session: &Session, statement: &str) -> i64 {
    let result = session.execute(statement).expect(statement);
    match result.rows() {
        [row] => match &row[0] {
            Value::Int64(n) => *n,
            other => panic!("{other:?}"),
        },
        rows => panic!("{rows:?}"),
    }
}

/// Every live edge has live endpoints, read through the public API.
fn assert_no_dangling_edge(db: &GrafeoDB) {
    for edge in db.store().all_edges() {
        assert!(
            db.get_node(edge.src).is_some() && db.get_node(edge.dst).is_some(),
            "edge {:?} ({:?} -> {:?}) has a deleted endpoint",
            edge.id,
            edge.src,
            edge.dst
        );
    }
}

fn seeded() -> GrafeoDB {
    let db = GrafeoDB::new_in_memory();
    let session = db.session();
    session.execute("INSERT (:A {k: 1})").expect("seed");
    session.execute("INSERT (:B {k: 2})").expect("seed");
    db
}

#[test]
fn detach_delete_takes_an_edge_created_in_the_same_transaction() {
    let db = seeded();
    let mut session = db.session();
    session.begin_transaction().expect("begin");
    session
        .execute("MATCH (a {k: 1}), (b {k: 2}) INSERT (a)-[:R]->(b)")
        .expect("edge");
    session
        .execute("MATCH (n {k: 2}) DETACH DELETE n")
        .expect("detach delete");
    session.commit().expect("commit");

    assert_no_dangling_edge(&db);
    let reader = db.session();
    assert_eq!(count(&reader, "MATCH (n) RETURN count(n)"), 1);
    assert_eq!(count(&reader, "MATCH ()-[r]->() RETURN count(r)"), 0);
    assert_eq!(db.edge_count(), 0);
}

#[test]
fn detach_delete_takes_an_incoming_edge_created_in_the_same_transaction() {
    let db = seeded();
    let mut session = db.session();
    session.begin_transaction().expect("begin");
    session
        .execute("MATCH (a {k: 1}), (b {k: 2}) INSERT (a)-[:R]->(b)")
        .expect("edge");
    session
        .execute("MATCH (n {k: 1}) DETACH DELETE n")
        .expect("detach delete");
    session.commit().expect("commit");

    assert_no_dangling_edge(&db);
    assert_eq!(db.edge_count(), 0);
    assert_eq!(db.node_count(), 1);
}

#[test]
fn detach_delete_of_a_node_created_in_the_same_transaction() {
    let db = GrafeoDB::new_in_memory();
    let mut session = db.session();
    session.begin_transaction().expect("begin");
    session
        .execute("INSERT (:A {k: 1})-[:R]->(:B {k: 2})")
        .expect("insert");
    session
        .execute("MATCH (n {k: 2}) DETACH DELETE n")
        .expect("detach delete");
    session.commit().expect("commit");

    assert_no_dangling_edge(&db);
    let reader = db.session();
    assert_eq!(count(&reader, "MATCH (n) RETURN count(n)"), 1);
    assert_eq!(count(&reader, "MATCH ()-[r]->() RETURN count(r)"), 0);
}

#[test]
fn delete_refuses_a_node_with_an_edge_created_in_the_same_transaction() {
    let db = seeded();
    let mut session = db.session();
    session.begin_transaction().expect("begin");
    session
        .execute("MATCH (a {k: 1}), (b {k: 2}) INSERT (a)-[:R]->(b)")
        .expect("edge");
    let err = session
        .execute("MATCH (n {k: 2}) DELETE n")
        .expect_err("a node with an edge needs DETACH DELETE");
    assert!(err.to_string().contains("DETACH"), "{err}");
    session.rollback().expect("rollback");
    assert_no_dangling_edge(&db);
    assert_eq!((db.node_count(), db.edge_count()), (2, 0));
}

#[test]
fn rolled_back_detach_delete_restores_the_edge() {
    let db = seeded();
    db.session()
        .execute("MATCH (a {k: 1}), (b {k: 2}) INSERT (a)-[:R]->(b)")
        .expect("edge");
    let mut session = db.session();
    session.begin_transaction().expect("begin");
    session
        .execute("MATCH (n {k: 2}) DETACH DELETE n")
        .expect("detach delete");
    session.rollback().expect("rollback");
    assert_eq!((db.node_count(), db.edge_count()), (2, 1));
    assert_no_dangling_edge(&db);
}
