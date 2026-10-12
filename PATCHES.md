# Loams Grafeo patches

`ostrium-labs/grafeo` is the Grafeo that Loams Graph (GR1) embeds. It is a
fork of `GrafeoDB/grafeo` that tracks upstream releases: each upstream
release Loams adopts gets a branch `loams/v<version>-patches`, started at the
upstream tag, and the patches below are carried onto it. Releases are tagged
`v<version>-loams.<n>` on that branch, and Loams pins the tag's commit by
`git` + `rev`.

Every change to Grafeo is a patch with an id `LG-<nnnn>` and a row below.
Each commit of a patch carries the trailer `Loams-Patch: LG-<nnnn>` and a
`Signed-off-by` (DCO).

How to add a patch:

1. Take the next free id.
2. Add the row in the same pull request as the patch's first commit.
3. Each patch is its own commit, with a test that fails before it and passes
   after it. A patch that changes no code (this file, NOTICE, a design note
   under `docs/loams/`) needs no test. Put `Loams-Patch: LG-<nnnn>` in the
   commit message (`git commit -s` adds the sign-off below it).
4. Open the pull request into `loams/v<version>-patches` and merge it with a
   merge commit, so the commits and trailers reach the branch unchanged.
5. Tag the merge `v<version>-loams.<n>` (tags are immutable), then bump the
   pin in Loams.

On a new upstream release: start `loams/v<new>-patches` at the upstream tag,
cherry-pick (`-x`) every row whose upstream PR has not been released, keep the
ids, and update the rows. A patch upstream has released is dropped (its row
moves to `## Dropped`).

Branches `main` and `loams/**` take no force-push and no deletion, and change
only through pull requests (merge commits). Tags `v*-loams.*` cannot be moved
or deleted. GitHub Actions are disabled on the fork for now; each patch's
tests are run locally before its pull request (see the pull request body).

Columns:

- **Why**: the Loams finding that needs it (rulings in Loams'
  `docs/plans/2026-10-08-gr1-graph-production.md`).
- **Upstreamable**: `yes` when the change is a general fix upstream could take
  as is, `no` when it is Loams' own behaviour.
- **Upstream PR**: the pull request offered to `GrafeoDB/grafeo`, or `-`.
  Offering a patch upstream is the owner's call.

## Patches

| Id | Title | Why | Upstreamable | Upstream PR |
|---|---|---|---|---|
| LG-0000 | Fork governance: this file, the NOTICE line | Loams owns a fork of Grafeo (owner, 2026-10-12) | no | - |
| LG-0001 | Commit listener: `Session::set_commit_listener` hands each commit's epoch and CDC events to the caller | Loams captures each commit's change set; reading it back with `changes_between` races CDC eviction and costs a scan of the whole log (GR1 R9.9, R9.10 item 1, R0.5) | yes | - |
| LG-0002 | `query_timeout` applies to statements with parameters: the query processor gets the session's deadline | `execute_with_params` had no deadline, so a parameterised statement ran past the timeout (GR1 R0.8, R5.8, Q679) | yes | - |
| LG-0003 | ORDER BY keeps returned relationships: a Sort or Top-K over a `RETURN` types its columns `Any` | `RETURN r ORDER BY a.name` answered `INT64 0` for every relationship: the resolved relationship was pushed into an `EdgeId` vector (GR1 R7.6) | yes | - |
| LG-0004 | `DETACH DELETE` (and `DELETE`) see the transaction's own uncommitted nodes and edges | A transaction that inserted an edge and then detached its endpoint committed a dangling edge; deleting a node created in the same transaction deleted nothing (GR1 R9.8) | yes | - |

## Planned

Ids taken by work that has a design but no code yet. A planned patch moves to
the table above when its first code commit lands.

| Id | Title | Why | Upstreamable | Design |
|---|---|---|---|---|
| LG-0005 | Snapshot isolation: a pinned session sees no later or uncommitted property, label or delete change, and `count(n)` agrees with the pin | Loams serialises reads behind a per-graph gate while a write is pending (GR1 R9.3) | yes | [docs/loams/LG-0005-snapshot-isolation.md](docs/loams/LG-0005-snapshot-isolation.md) |

## Dropped

| Id | Title | Dropped in | Why |
|---|---|---|---|
