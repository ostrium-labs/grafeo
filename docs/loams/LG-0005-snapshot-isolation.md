# LG-0005: snapshot isolation for pinned sessions (design note, not implemented)

Status: **not implemented** in `v0.5.43-loams.1`. This note records what full
snapshot isolation needs, and why it was not done as one patch. Loams keeps its
per-graph reader/writer gate (Loams GR1 ruling R9.3) until it lands.

## The requirement

A session pinned at epoch `E` (`Session::set_viewing_epoch(E)`, or a
transaction's start epoch) must see the database exactly as it was committed at
`E`:

1. no node or edge created after `E` (already true);
2. no property set or removed, label added or removed, or node or edge deleted
   by a transaction that committed after `E`;
3. no write of a transaction that has not committed, whatever `E` is;
4. aggregates (`count(n)`) and index-driven plans agree with 1-3.

## What 0.5.43 does (without the `temporal` feature)

Measured on this branch (Loams `capture_spike.rs`,
`viewing_epoch_hides_later_commits` and `uncommitted_writes_are_visible_to_pinned_sessions`,
plus a probe on a CDC database with a `ReadOnly` session pinned before an
`INSERT` and a `DETACH DELETE`):

| Read | Answer at the pin |
|---|---|
| `MATCH (n) RETURN n.k` | the deleted node is still returned, with its properties gone (`NULL`); the new node is hidden |
| `MATCH (n:P) RETURN n.k` | the deleted node is not returned; the new node is hidden |
| `MATCH (n) RETURN count(n)` | counts the deleted node (Loams' "count ignores the pin": 4 = three live nodes plus the deleted one, not the hidden creation) |
| `MATCH (n:P) RETURN count(n)` | does not count it |
| a property or label changed after the pin | the new value |
| an uncommitted property set, label add or delete | visible |

Causes, by storage structure:

- **Element existence** is a `VersionChain` per node and edge
  (`grafeo-common/src/mvcc.rs`). A creation is `PENDING` until commit, so it is
  hidden correctly. A delete calls `VersionInfo::mark_deleted(epoch, tx)` with
  the deleting transaction's *viewing* epoch (its start epoch, from
  `DeleteNodeOperator`), not `PENDING` and not the commit epoch. So a session
  pinned before that epoch still sees the element (right), a session pinned at
  or after it does not, even while the delete is uncommitted (wrong, rule 3),
  and a session pinned between the start and the commit epoch sees the delete
  early (wrong, rule 2).
- **Properties** live in place in `PropertyStorage` columns
  (`node_properties`, `edge_properties`) with an undo log for rollback
  (`property_ops.rs`, `PropertyUndoEntry`). There is one value per key; there
  is nothing to read "as of `E`". `delete_node_transactional` also removes the
  properties in place, which is why the deleted node reads as `NULL`.
- **Labels** live in place in `node_labels` and in the `label_index`, which a
  delete also updates in place. A labelled scan (`nodes_by_label`) therefore
  hides a deleted node at once, while an unlabelled scan (`all_node_ids` +
  `filter_visible_node_ids(E)`) goes by the version chain and still returns it:
  this is the `count(n)` disagreement.
- **Indexes** (property, text, vector) hold current values only, so an
  index-driven plan for a pinned session finds elements by their newest values.

## Why `temporal` is not the answer as it stands

`temporal` versions properties and labels (`VersionLog` per key), which covers
part of rule 2, but Loams measured four defects (GR1 R9.6):

1. `GrafeoDB::gc` prunes versions below the oldest *transaction*, not the
   oldest pinned session, so a pinned read later returns `NULL`;
2. `get_node_at_epoch`'s fast path (`epoch >= current_epoch` reads `latest()`)
   returns uncommitted (`PENDING`) values to a session pinned at the current
   epoch (rule 3);
3. setting a property and deleting its node in one transaction panicked in
   `VersionLog::append` (a leftover `PENDING` entry). LG-0004 now appends the
   delete's tombstone at `PENDING` behind such entries, so it commits in order;
   a *rollback* of that transaction is still wrong under `temporal`: the
   rollback restores the deleted element (`restore_deleted_node`/`_edge`,
   properties at the current epoch) before it pops the `PENDING` entries, so
   it appends an older epoch after the `PENDING` tombstone (the same debug
   panic, now on rollback instead of on the delete) and would restore the
   transaction's own uncommitted values. The fix belongs with point 2 below:
   pop the element's `PENDING` entries first, then restore only the keys the
   transaction did not set;
4. `finalize_pending` at commit finalises every pending version, not only the
   committing transaction's.

It also does nothing for deletes or for indexes, and it roughly doubles
property memory.

## Design

A complete fix is these changes together; each one alone leaves rule 1-4
broken in a different way, which is why this was not landed piecemeal.

1. **Deletes are versions.** `mark_deleted` records `PENDING` with
   `deleted_by = tx`; commit's `finalize_version_epochs` turns it into the
   commit epoch (`VersionChain::finalize_epochs` handles `deleted_epoch` as it
   handles `created_epoch`); rollback unmarks it (already there). Visibility:
   `is_visible_at(E)` treats a `PENDING` delete as not yet happened;
   `is_visible_to(E, tx)` hides an element the viewer deleted. A delete no
   longer removes properties, labels or index entries in place: GC does that
   once no pinned session or transaction is below the delete epoch.
2. **Versioned properties and labels in every build**, by fixing `temporal`
   rather than adding a second mechanism: each `VersionLog` entry carries
   `(epoch, tx)` with `PENDING` until commit; `finalize_pending(tx, epoch)`
   finalises only `tx`'s entries (defect 4); a read at `E` skips `PENDING`
   entries except the reader's own, and the fast path applies only when the
   reader is not pinned and the log has no `PENDING` tail (defect 2); a
   transaction's second write to a key replaces its own `PENDING` entry
   instead of appending, and rollback of a delete restores the last committed
   values, not the ones captured at the delete (defect 3).
3. **GC honours pins.** The session registers its viewing epoch with the
   `TransactionManager` (a pin count per epoch); `min_active_epoch` is the
   minimum over active transactions *and* pins; GC never prunes a version, a
   delete or a CDC event a pin can still see (defect 1).
4. **Scans and counts read one structure.** `nodes_by_label` and the property,
   text and vector indexes become candidate generators only: every candidate
   is re-checked against the version chain and the versioned labels and
   properties at `E` (`filter_visible_node_ids` plus a label/property check),
   and index maintenance on delete moves to GC (point 1). Planner statistics
   (`node_count`, `nodes_by_label_count`) stay current-epoch estimates; only
   operators produce answers.
5. **Parameterised statements honour the pin.** `Session::execute_with_params`
   passes the viewing epoch to `QueryProcessor` only inside a transaction
   (`with_transaction_context`), so a pinned session's parameterised statement
   reads the current epoch. The processor needs the epoch without a
   transaction too.

Files: `grafeo-common/src/mvcc.rs`, `grafeo-core/src/graph/lpg/store/`
(`node_ops.rs`, `edge_ops.rs`, `property_ops.rs`, `versioning.rs`,
`traversal.rs`, `temporal.rs`, the index modules),
`grafeo-core/src/execution/operators/{scan,expand,mutation}.rs`,
`grafeo-engine/src/transaction/manager.rs`, `grafeo-engine/src/session/mod.rs`,
`grafeo-engine/src/query/processor.rs`, and the snapshot and section formats
(property history in every build).

## Tests that define done

Run on a CDC database with `ReadOnly` sessions, as Loams pins them:

- a session pinned at `E` reads byte-identical answers before and after later
  commits of every write kind (property set and remove, `SET n = {}` and
  `+= {}`, label add and remove, `DELETE`, `DETACH DELETE`, edge property
  changes), for labelled and unlabelled scans, `count(n)`, index lookups
  (`MATCH (n:P {k: 1})` with an index on `k`) and parameterised statements;
- while a transaction is open with each write kind, a session pinned at the
  current epoch reads the pre-transaction state; after rollback, the same;
- `GrafeoDB::gc` with a pinned session changes nothing it reads; after the pin
  is dropped, gc reclaims the versions;
- set-then-delete in one transaction commits; a proptest over Loams'
  `capture_spike` workloads with a reader pinned before each transaction
  asserts the reader's dump equals the model's state at the pin.

When these pass, Loams can drop the gate's shared side for reads (R9.3) and
flip `viewing_epoch_hides_later_commits` and
`uncommitted_writes_are_visible_to_pinned_sessions`.

## Estimate and risk

Roughly 1 500-3 000 lines across the storage layer, the operators and the
formats, plus a memory cost for keeping property and label history until GC.
It changes the semantics every read path relies on, so it needs the upstream
test suite plus the tests above, and is best done with upstream (it is the
`temporal` feature's own open problem).
