# Jiangcheng Memory Design

**Status:** Approved design for implementation planning  
**Date:** 2026-07-29  
**Product:** 疆诚记忆 / Jiangcheng Memory  
**Repository:** `jiangcheng-memory` (hosted in the existing `jc-memory` directory)  
**CLI:** `jcmem`  
**Daemon:** `jcmemd`

## 1. Purpose

Jiangcheng Memory is a model-independent, memory-native agent kernel and
neurocognitive memory database. It preserves an agent's identity, evidence,
experience, preferences, project state, and learned procedures independently
of any particular LLM.

The responsibility boundary is strict:

- Memory preserves the past and learns conservative, evidence-linked patterns.
- An LLM proposes hypotheses, predictions, plans, and action candidates.
- The runtime applies policy, obtains approval, selects actions, and commits
  verified outcomes.
- Tools affect the external environment and return observable results.

Changing model providers must not change the authoritative memory database or
promote model output into verified fact.

## 2. Product And Release Boundary

The current directory is the repository root. Internal package and publication
identifiers use `jiangcheng-memory` and the `jc-memory-*` crate prefix.

The project is private source for the initial release:

- `LICENSE` reserves all rights.
- Documentation does not describe the project as open source.
- Release workflows are manually triggered and produce private artifacts by
  default.
- No workflow publishes automatically to crates.io, npm, PyPI, or a public
  GitHub release.

The system is local-first and runs without Python, an external LLM, or a cloud
vector database. Python is required only for model training, evaluation, and
ONNX export.

## 3. Delivery Strategy

Implementation uses staged vertical slices. Each milestone must provide an
integrated, runnable, and tested capability rather than disconnected
scaffolding.

1. Protocol, domain types, immutable events, SQLite/FTS5, BlobStore, and
   migrations.
2. Point, Edge, Surface, state versions, pattern separation, bounded recall,
   Explain, and Memory Packet.
3. Consolidation, deterministic neural fallback, ACL/isolation/audit, snapshot,
   restore, and rebuild.
4. Agent Runtime, capability registries, model/tool loop, HTTP, gRPC, Sidecar,
   and MCP.
5. Node, Python, Go, Kotlin, Swift, and Java SDKs; Hermes, OpenClaw, Mem0,
   Graphiti, and Dify adapters; examples and contract tests.
6. ONNX toolchain, benchmark harnesses, CI, private release artifacts, and
   documentation closure.

Every milestone keeps the repository compilable and tests the modules it adds.
Directories are created when they gain real code, tests, public extension
interfaces, fixtures, documentation, or build configuration.

## 4. Architecture

Dependencies flow in one direction:

```text
protocol and domain types
        |
immutable events + BlobStore + SQLite
        |
episodic / semantic / graph / surface / activation
        |
recall + explain + consolidation + security
        |
memory-core public embedded facade
        |
runtime / sidecar / server / MCP
        |
SDKs / adapters / examples / benchmarks
```

`memory-core` is the high-level embedded API. Lower-level crates never depend
on services, SDKs, or adapters. Protocol types shared across process boundaries
are versioned in `memory-protocol` and Protobuf definitions.

The initial physical store is SQLite in WAL mode. FTS5 supplies local full-text
retrieval. A content-addressed BlobStore holds large and binary content. Neural
components are optional plugins with deterministic local fallbacks.

## 5. Temporal Cognitive Model

The implementation distinguishes these stages in types, persistence, and
documentation:

1. verifiable past evidence;
2. current observation;
3. LLM future hypotheses;
4. runtime decision;
5. tool action;
6. actual result;
7. result verification.

An observed interaction produces an immutable transition record containing the
observation, recalled evidence, model hypotheses, runtime selection, tool
action, actual result, and verification status. Consolidation adds structure to
that record; it never overwrites it.

## 6. Authoritative Domain Model

`CognitiveFragment` is the minimum input unit. It includes namespace and
optional tenant, user, agent, project, session, and trace scopes; kind, content,
source, perspective, occurrence and observation times; confidence, importance,
validity, security label, evidence, and metadata.

Fragment kinds cover observations, claims, inferences, predictions, decisions,
actions, tool calls, tool results, outcomes, corrections, constraints,
preferences, facts, failures, successes, plans, approvals, rejections, and state
transitions. Validity distinguishes unverified, verified, disputed, superseded,
invalidated, and quarantined data.

The authoritative records are:

- original fragments and events;
- content blobs and evidence references;
- explicit corrections and verification events;
- scopes, permissions, security labels, and audit records;
- state transitions and versions;
- human approvals and rejections.

These records cannot be silently replaced by summaries, embeddings, graph
relationships, or surfaces.

## 7. Point, Edge, And Surface

A Point is an atomic cognitive change backed by one or more fragments. An Edge
is a typed, directed, time-aware relationship with weight, confidence,
provenance, model version, and human/tool verification flags. Co-occurrence may
create a low-confidence candidate association but never a verified causal edge.

Online edge updates are bounded and auditable. Reinforcement and weakening
record the triggering success or failure evidence and clip weights to a defined
range.

A Surface is a versioned dynamic cognitive space with semantic center, member
points and edges, activation threshold, time range, current and historical
states, confidence, boundaries, evidence, and model version. Points may belong
to multiple surfaces. Merge, split, freeze, deactivate, conflict detection, and
incremental member/center updates preserve surface history.

## 8. Storage And Transaction Semantics

SQLite enables WAL, foreign keys, busy timeout, migrations, transactions,
indexes, FTS5, and integrity checks. Logical tables cover the entities named in
the product requirements, including fragments, events, graph data, state
versions, recalls, transitions, capabilities, approvals, audit logs, snapshots,
quarantine, and migrations.

An authoritative write follows this sequence:

```text
validate scope and ACL
-> persist fragment and content reference
-> persist event or transition
-> persist evidence references
-> commit the SQLite transaction
-> incrementally build or queue rebuildable derivatives
```

Blob writes use a temporary file, SHA-256 verification, atomic rename, and then
a database reference. A failed database transaction can leave only an
unreferenced blob, which auditable garbage collection may remove. Startup
recovery checks references, checksums, and interrupted jobs.

Corrections, contradictions, supersession, and ordinary forgetting append new
events and relationships. Governance-authorized hard deletion removes
authoritative records, blobs, and derivatives and leaves a content-free deletion
receipt in the audit trail.

Rebuildable data includes embeddings, FTS/vector indexes, candidate edges,
surface membership, summaries, and caches.

## 9. State And Conflict Semantics

State uses an immutable version chain with valid-time intervals and supporting
evidence. `CurrentState` returns the latest valid, visible version.
`HistoricalState` evaluates the requested time. If visible evidence conflicts
at the same point in time, recall returns the conflict and missing evidence
rather than silently selecting a fact.

Predictions remain predictions. A prediction can be validated or disproved only
by a later verification event linked to actual evidence.

## 10. Recall And Explain

All recall modes use the same bounded pipeline:

```text
scope/ACL pre-filter
-> exact, FTS, and vector seed retrieval
-> bounded candidate set
-> intent-specific edge selection
-> bounded local spreading activation
-> surface activation
-> evidence completeness check
-> rerank and minimal evidence subgraph
-> scope/ACL output recheck
-> Memory Packet and Explain trace
```

Configuration limits seed count, neighbors per layer, hop count, total nodes,
and execution time. Recall never performs unbounded propagation over the entire
graph.

The deterministic fallback provides stable lexical vectors, rule-based routing,
classification, and reranking for offline operation and repeatable tests. ONNX
models may improve candidate selection and ranking but cannot change
authoritative validity.

Memory Packet is model-independent. Explain records seeds, traversed paths,
activated surfaces, evidence, inferred relationships, verified facts,
conflicts, coverage, and missing evidence.

## 11. Consolidation And Neural Plugins

Consolidation runs resumable, checkpointed, resource-bounded jobs for replay,
deduplication, edge adjustment, surface discovery/merge/split, semantic
promotion, conflict and stale-state detection, cold-tier movement, and rebuild.
Jobs update local regions incrementally and are conservative by default.

Neural interfaces cover fragment encoding, event classification, importance,
edge prediction, query routing, reranking, surface assignment, and contradiction
detection. Each implementation declares manifest, version, checksum, input and
output schema, device policy, batching, timeout, and circuit-breaker behavior.

The repository includes a deterministic Rust fallback and a small reproducible
ONNX fixture. Python training and export code must validate ONNX output parity;
it must not ship fabricated large-model weights.

## 12. Runtime And Capabilities

The agent runtime is an explicit recoverable state machine:

```text
Observing -> Recalling -> Reasoning -> AwaitingApproval -> Executing
          -> Committing -> Completed
```

Additional states cover WaitingUser, Paused, Failed, Cancelled, retries, and
timeouts. Each step records a transition, referenced inputs and outputs, retry
count, timeout, and trace. Recovery resumes after the latest committed step and
does not repeat a non-idempotent tool action without an explicit recovery
decision.

Model output is parsed into Hypothesis, Prediction, Plan, DecisionCandidate,
ToolRequest, or FinalResponse. Policy and approval are applied before tool
execution. Only verified outcomes can create factual memory.

Prompt, Skill, Tool, MCPTool, Workflow, and Policy implement one versioned
Capability contract. Execution statistics form procedural memory and influence
ranking, while remaining statistical evidence rather than fact.

## 13. Security And Governance

Every core operation requires an `AccessContext` containing namespace, tenant,
user, agent, project, principal, and grants. The storage query layer applies
authorization before retrieval; packet construction checks it again before
return. Services cannot bypass the core filter.

Unverified model output, untrusted imports, suspicious tool output, prompt
injection, and poisoning indicators may be quarantined. Quarantined content is
excluded from normal recall and semantic promotion unless an authorized review
changes its status.

Security labels, pluggable key providers, optional AES-GCM blob encryption,
retention policies, export, hard deletion, derivative cleanup, and
reconsolidation are first-class operations. Audit records avoid copying
sensitive bodies.

## 14. Services And Protocols

- Embedded Rust is the semantic reference implementation.
- Sidecar reuses the core. Unix-like systems use Unix domain sockets. Windows
  initially uses token-protected loopback behind a transport abstraction; Named
  Pipe support is an explicit compatible extension and is not claimed until
  built and tested.
- Server uses Axum for REST, Tonic for gRPC, generated OpenAPI, API keys,
  multi-tenancy, request/trace IDs, rate limits, metrics, structured logs, TLS
  configuration, and graceful shutdown.
- MCP implements standard protocol framing and exposes observe, recall, commit,
  correct, explain, forget, and stats with complete schemas and structured
  Memory Packet results.

Protocol errors have stable codes and structured details. REST, gRPC, MCP, and
embedded calls execute common core commands so their semantics cannot drift.

## 15. SDKs And Adapters

Shared Protobuf/OpenAPI contracts and versioned fixtures define SDK behavior.
Node and Python provide embedded and remote modes; Go provides remote and C ABI
integration; Kotlin, Swift, and Java provide native binding and/or remote modes
as specified. SDK APIs expose typed async cancellation and errors appropriate to
their language.

Hermes, OpenClaw, Mem0, Graphiti, and Dify adapters map source records into
fragments while preserving provenance, original confidence, temporal validity,
and scope. Fixture and contract tests validate external integrations when the
real platform is unavailable. Documentation names the external versions tested
and never presents fixture compatibility as live verification.

## 16. Testing And Benchmarks

Testing is risk-layered:

- crate unit, property, concurrency, and error-path tests;
- transaction rollback, migration, corruption, crash recovery, and rebuild;
- all seven product acceptance scenarios as deterministic integration tests;
- scope matrix, cross-tenant leakage, injection, poisoning, and deletion;
- semantic parity across embedded, REST, gRPC, and MCP;
- shared SDK contract fixtures;
- deterministic fallback and ONNX export/runtime parity;
- benchmark smoke tests in normal CI and explicit million-fragment long runs.

The benchmark harness covers LongMemEval, LoCoMo, Cross-Year, Model Swap,
Memory Poisoning, and Million Fragments and reports the metrics required by the
product specification. External datasets are downloaded only through explicit,
checksum-verified setup steps; generated fixtures are deterministic.

Each milestone runs focused tests and all applicable workspace gates. Final
Rust gates are:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

SDK tests, dependency audit, license policy, Protobuf compatibility, benchmark
smoke tests, and `git diff --check` are additional gates.

## 17. CI And Private Release

CI defines Linux, macOS, and Windows coverage for Rust and supported SDKs.
Release workflows build the requested desktop/server binaries and language
packages through manual private runs. Mobile signing and store publication are
outside the first release; build scripts and separate workflows remain
required.

Final reports distinguish:

- commands run successfully on the current host;
- checks represented only by CI configuration;
- static or fixture-based compatibility;
- platform, signing, hardware, dataset, or external-service validation still
  required.

## 18. Documentation And Decisions

English and Chinese READMEs provide a five-minute local start. Architecture,
concept, protocol, SDK, deployment, security, benchmark, migration, recovery,
upgrade, threat-model, and tuning documentation is versioned with the code.

ADRs record at least the choices of Rust, SQLite, model-independent memory,
immutable original events, prediction/fact separation, fast/slow memory,
bounded local propagation, and rebuildable derivatives.

## 19. Acceptance

The implementation is complete only when it demonstrates:

1. cross-year evidence recall through bounded reconstruction;
2. current and historical answers across a state replacement;
3. a failed actual result disproving, rather than confirming, a prediction;
4. evidence-linked procedural learning from repeated outcomes;
5. memory consistency across model provider swaps;
6. zero cross-tenant recall in the isolation test suite;
7. consistent restart after injected failures around database, blob, index, and
   consolidation operations.

The final delivery inventory must state implemented modules, commands, schema,
protocols, SDK examples, tests and benchmarks actually run, platforms actually
built, external validation gaps, known risks, unresolved security or
consistency issues, and whether any placeholder remains.

## 20. Non-Goals And Guardrails

- The first version does not implement its own B-tree, WAL, or foundation LLM.
- It does not require cloud infrastructure or Python at runtime.
- It does not treat summaries, embeddings, predicted edges, or LLM text as
  authoritative evidence.
- It does not claim live compatibility with unavailable external platforms.
- It does not create unconnected files merely to mirror a desired tree.
- It does not use `unsafe` in core logic. Binding-related `unsafe`, if required,
  is isolated, documents invariants, and has boundary tests.
