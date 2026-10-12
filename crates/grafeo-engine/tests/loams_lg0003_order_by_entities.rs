//! LG-0003 (Loams fork, PATCHES.md): ORDER BY on a property the RETURN does not project keeps the
//! returned relationships and nodes. On upstream Grafeo 0.5.43 `RETURN r ORDER BY a.name` answers
//! `INT64 0` for every relationship (Loams GR1 ruling R7.6).
//!
//! ```bash
//! cargo test -p grafeo-engine --test loams_lg0003_order_by_entities
//! ```

#![cfg(feature = "gql")]

use std::collections::HashMap;

use grafeo_common::types::Value;
use grafeo_engine::GrafeoDB;

fn movies() -> GrafeoDB {
    let db = GrafeoDB::new_in_memory();
    let session = db.session();
    for statement in [
        "INSERT (:Person {name: 'Carol'})-[:ACTED_IN {role: 'c'}]->(:Movie {title: 'Z'})",
        "INSERT (:Person {name: 'Alix'})-[:ACTED_IN {role: 'a'}]->(:Movie {title: 'Y'})",
        "INSERT (:Person {name: 'Gus'})-[:ACTED_IN {role: 'g'}]->(:Movie {title: 'X'})",
    ] {
        session.execute(statement).expect(statement);
    }
    db
}

/// `name -> value of the column` from the same pattern without ORDER BY, the shape upstream
/// answers correctly.
fn by_name(db: &GrafeoDB, column: &str) -> HashMap<String, Value> {
    let statement = format!("MATCH (a:Person)-[r:ACTED_IN]->(m:Movie) RETURN a.name, {column}");
    db.session()
        .execute(&statement)
        .expect(&statement)
        .into_rows()
        .into_iter()
        .map(|row| {
            let Value::String(name) = &row[0] else {
                panic!("{row:?}")
            };
            (name.to_string(), row[1].clone())
        })
        .collect()
}

fn ordered(db: &GrafeoDB, statement: &str) -> Vec<Value> {
    db.session()
        .execute(statement)
        .expect(statement)
        .into_rows()
        .into_iter()
        .map(|mut row| row.remove(0))
        .collect()
}

#[test]
fn relationship_ordered_by_an_unreturned_property() {
    let db = movies();
    let want = by_name(&db, "r");
    assert!(
        want.values().all(|v| !matches!(v, Value::Int64(_))),
        "unordered relationships are entities: {want:?}"
    );
    assert_eq!(
        ordered(
            &db,
            "MATCH (a:Person)-[r:ACTED_IN]->(m:Movie) RETURN r ORDER BY a.name"
        ),
        vec![
            want["Alix"].clone(),
            want["Carol"].clone(),
            want["Gus"].clone()
        ]
    );
    assert_eq!(
        ordered(
            &db,
            "MATCH (a:Person)-[r:ACTED_IN]->(m:Movie) RETURN r ORDER BY a.name DESC"
        ),
        vec![
            want["Gus"].clone(),
            want["Carol"].clone(),
            want["Alix"].clone()
        ]
    );
}

#[test]
fn node_ordered_by_an_unreturned_property() {
    let db = movies();
    let want = by_name(&db, "m");
    assert_eq!(
        ordered(
            &db,
            "MATCH (a:Person)-[r:ACTED_IN]->(m:Movie) RETURN m ORDER BY a.name"
        ),
        vec![
            want["Alix"].clone(),
            want["Carol"].clone(),
            want["Gus"].clone()
        ]
    );
}

#[test]
fn relationship_ordered_by_its_own_property() {
    let db = movies();
    let want = by_name(&db, "r");
    assert_eq!(
        ordered(
            &db,
            "MATCH (a:Person)-[r:ACTED_IN]->(m:Movie) RETURN r ORDER BY r.role DESC"
        ),
        vec![
            want["Gus"].clone(),
            want["Carol"].clone(),
            want["Alix"].clone()
        ]
    );
}
