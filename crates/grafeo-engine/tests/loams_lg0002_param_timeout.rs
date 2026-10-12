//! LG-0002 (Loams fork, PATCHES.md): `query_timeout` applies to statements with parameters. On
//! upstream Grafeo 0.5.43 `Session::execute_with_params` runs through the query processor, whose
//! executor has no deadline, so a parameterised statement runs past the timeout that stops the
//! same statement without parameters.
//!
//! ```bash
//! cargo test -p grafeo-engine --test loams_lg0002_param_timeout
//! ```

#![cfg(feature = "gql")]

use std::collections::HashMap;
use std::time::Duration;

use grafeo_common::types::Value;
use grafeo_engine::{Config, GrafeoDB};

/// 80 nodes, so the three-way cartesian product below is 512 000 rows: hundreds of chunks, far
/// more than 20 ms of work.
fn db() -> GrafeoDB {
    let db =
        GrafeoDB::with_config(Config::in_memory().with_query_timeout(Duration::from_millis(20)))
            .expect("db");
    db.session()
        .execute("UNWIND range(1, 80) AS i INSERT (:T {i: i})")
        .expect("insert");
    db
}

const PARAMETERISED: &str = "MATCH (a:T), (b:T), (c:T) WHERE a.i > $min RETURN a.i, b.i, c.i";

fn min() -> HashMap<String, Value> {
    [("min".to_string(), Value::Int64(-1))].into()
}

fn is_timeout(err: &grafeo_common::utils::error::Error) -> bool {
    err.to_string().contains("timeout") || err.to_string().contains("timed out")
}

#[test]
fn unparameterised_statement_is_stopped() {
    let db = db();
    let err = db
        .session()
        .execute("MATCH (a:T), (b:T), (c:T) WHERE a.i > -1 RETURN a.i, b.i, c.i")
        .expect_err("stopped by query_timeout");
    assert!(is_timeout(&err), "{err}");
}

#[test]
fn parameterised_statement_is_stopped() {
    let db = db();
    let err = db
        .session()
        .execute_with_params(PARAMETERISED, min())
        .expect_err("stopped by query_timeout");
    assert!(is_timeout(&err), "{err}");
}

#[test]
fn parameterised_statement_in_a_transaction_is_stopped() {
    let db = db();
    let mut session = db.session();
    session.begin_transaction().expect("begin");
    let err = session
        .execute_with_params(PARAMETERISED, min())
        .expect_err("stopped by query_timeout");
    assert!(is_timeout(&err), "{err}");
    session.rollback().expect("rollback");
}

#[test]
fn parameterised_statement_at_an_epoch_is_stopped() {
    let db = db();
    let epoch = db.current_epoch();
    let err = db
        .session()
        .execute_at_epoch_with_params(PARAMETERISED, epoch, Some(min()))
        .expect_err("stopped by query_timeout");
    assert!(is_timeout(&err), "{err}");
}

#[test]
fn small_parameterised_statement_still_answers() {
    let db = db();
    let result = db
        .session()
        .execute_with_params("MATCH (a:T) WHERE a.i > $min RETURN count(a)", min())
        .expect("answers within the timeout");
    assert_eq!(result.rows(), vec![vec![Value::Int64(80)]]);
}
