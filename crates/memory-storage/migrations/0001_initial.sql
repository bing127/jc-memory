CREATE TABLE fragment_contents (
    id TEXT PRIMARY KEY NOT NULL,
    storage_kind TEXT NOT NULL CHECK (storage_kind IN ('inline_text', 'inline_json', 'blob')),
    inline_value BLOB,
    blob_id TEXT,
    media_type TEXT NOT NULL CHECK (length(media_type) > 0),
    byte_len INTEGER NOT NULL CHECK (byte_len >= 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    created_at_us INTEGER NOT NULL,
    UNIQUE (sha256, media_type),
    CHECK (
        (storage_kind = 'blob' AND blob_id IS NOT NULL AND inline_value IS NULL)
        OR (storage_kind != 'blob' AND blob_id IS NULL AND inline_value IS NOT NULL)
    )
) STRICT;

CREATE TABLE fragments (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    namespace_id TEXT NOT NULL,
    tenant_id TEXT,
    user_id TEXT,
    agent_id TEXT,
    project_id TEXT,
    session_id TEXT,
    trace_id TEXT,
    kind TEXT NOT NULL,
    content_id TEXT NOT NULL,
    source_json TEXT NOT NULL CHECK (json_valid(source_json)),
    perspective TEXT NOT NULL,
    occurred_at_us INTEGER NOT NULL,
    observed_at_us INTEGER NOT NULL,
    confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    importance REAL NOT NULL CHECK (importance >= 0.0 AND importance <= 1.0),
    validity TEXT NOT NULL,
    security_label TEXT NOT NULL,
    metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json)),
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (content_id) REFERENCES fragment_contents(id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE events (
    id TEXT PRIMARY KEY NOT NULL,
    fragment_id TEXT NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    namespace_id TEXT NOT NULL,
    tenant_id TEXT,
    stage TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    recorded_at_us INTEGER NOT NULL,
    content_hash TEXT NOT NULL CHECK (length(content_hash) = 64),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    FOREIGN KEY (fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT,
    UNIQUE (scope_key, sequence)
) STRICT;

CREATE TABLE scope_sequences (
    scope_key TEXT PRIMARY KEY NOT NULL CHECK (length(scope_key) = 64),
    next_sequence INTEGER NOT NULL CHECK (next_sequence >= 0)
) STRICT;

CREATE TABLE model_versions (
    id TEXT PRIMARY KEY NOT NULL,
    provider TEXT NOT NULL,
    model_name TEXT NOT NULL,
    version TEXT NOT NULL,
    manifest_json TEXT NOT NULL CHECK (json_valid(manifest_json)),
    checksum TEXT NOT NULL CHECK (length(checksum) = 64),
    created_at_us INTEGER NOT NULL,
    UNIQUE (provider, model_name, version)
) STRICT;

CREATE TABLE memory_models (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    version TEXT NOT NULL,
    manifest_json TEXT NOT NULL CHECK (json_valid(manifest_json)),
    checksum TEXT NOT NULL CHECK (length(checksum) = 64),
    status TEXT NOT NULL,
    created_at_us INTEGER NOT NULL,
    UNIQUE (kind, version)
) STRICT;

CREATE TABLE points (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    kind TEXT NOT NULL,
    source_fragment_id TEXT NOT NULL,
    valid_from_us INTEGER NOT NULL,
    valid_to_us INTEGER,
    confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (source_fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT,
    CHECK (valid_to_us IS NULL OR valid_to_us >= valid_from_us)
) STRICT;

CREATE TABLE edges (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    source_point_id TEXT NOT NULL,
    target_point_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    weight REAL NOT NULL CHECK (weight >= 0.0 AND weight <= 1.0),
    confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    source_fragment_id TEXT NOT NULL,
    valid_from_us INTEGER NOT NULL,
    valid_to_us INTEGER,
    model_version_id TEXT,
    human_confirmed INTEGER NOT NULL DEFAULT 0 CHECK (human_confirmed IN (0, 1)),
    tool_verified INTEGER NOT NULL DEFAULT 0 CHECK (tool_verified IN (0, 1)),
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (source_point_id) REFERENCES points(id) ON DELETE CASCADE,
    FOREIGN KEY (target_point_id) REFERENCES points(id) ON DELETE CASCADE,
    FOREIGN KEY (source_fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT,
    FOREIGN KEY (model_version_id) REFERENCES model_versions(id) ON DELETE RESTRICT,
    CHECK (source_point_id != target_point_id),
    CHECK (valid_to_us IS NULL OR valid_to_us >= valid_from_us)
) STRICT;

CREATE TABLE surfaces (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    semantic_center_json TEXT NOT NULL CHECK (json_valid(semantic_center_json)),
    activation_threshold REAL NOT NULL CHECK (activation_threshold >= 0.0 AND activation_threshold <= 1.0),
    confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    model_version_id TEXT,
    version INTEGER NOT NULL CHECK (version >= 1),
    valid_from_us INTEGER NOT NULL,
    valid_to_us INTEGER,
    source_evidence_json TEXT NOT NULL CHECK (json_valid(source_evidence_json)),
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (model_version_id) REFERENCES model_versions(id) ON DELETE RESTRICT,
    CHECK (valid_to_us IS NULL OR valid_to_us >= valid_from_us),
    UNIQUE (id, version)
) STRICT;

CREATE TABLE surface_members (
    surface_id TEXT NOT NULL,
    point_id TEXT NOT NULL,
    member_weight REAL NOT NULL CHECK (member_weight >= 0.0 AND member_weight <= 1.0),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_at_us INTEGER NOT NULL,
    PRIMARY KEY (surface_id, point_id, version),
    FOREIGN KEY (surface_id) REFERENCES surfaces(id) ON DELETE CASCADE,
    FOREIGN KEY (point_id) REFERENCES points(id) ON DELETE CASCADE
) STRICT;

CREATE TABLE entities (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    entity_type TEXT NOT NULL,
    canonical_name TEXT NOT NULL CHECK (length(canonical_name) > 0),
    source_fragment_id TEXT NOT NULL,
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (source_fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT,
    UNIQUE (scope_key, entity_type, canonical_name)
) STRICT;

CREATE TABLE entity_aliases (
    entity_id TEXT NOT NULL,
    alias TEXT NOT NULL CHECK (length(alias) > 0),
    source_fragment_id TEXT NOT NULL,
    created_at_us INTEGER NOT NULL,
    PRIMARY KEY (entity_id, alias),
    FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE,
    FOREIGN KEY (source_fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE states (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    state_key TEXT NOT NULL CHECK (length(state_key) > 0),
    current_version_id TEXT,
    created_at_us INTEGER NOT NULL,
    UNIQUE (scope_key, state_key),
    FOREIGN KEY (current_version_id) REFERENCES state_versions(id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE state_versions (
    id TEXT PRIMARY KEY NOT NULL,
    state_id TEXT NOT NULL,
    value_json TEXT NOT NULL CHECK (json_valid(value_json)),
    valid_from_us INTEGER NOT NULL,
    valid_to_us INTEGER,
    validity TEXT NOT NULL,
    source_fragment_id TEXT NOT NULL,
    previous_version_id TEXT,
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (state_id) REFERENCES states(id) ON DELETE CASCADE,
    FOREIGN KEY (source_fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT,
    FOREIGN KEY (previous_version_id) REFERENCES state_versions(id) ON DELETE RESTRICT,
    CHECK (valid_to_us IS NULL OR valid_to_us >= valid_from_us)
) STRICT;

CREATE TABLE evidence_refs (
    id TEXT PRIMARY KEY NOT NULL,
    owner_fragment_id TEXT NOT NULL,
    referenced_fragment_id TEXT,
    referenced_event_id TEXT,
    verification_method TEXT NOT NULL,
    note TEXT,
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (owner_fragment_id) REFERENCES fragments(id) ON DELETE CASCADE,
    FOREIGN KEY (referenced_fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT,
    FOREIGN KEY (referenced_event_id) REFERENCES events(id) ON DELETE RESTRICT,
    CHECK ((referenced_fragment_id IS NOT NULL) != (referenced_event_id IS NOT NULL))
) STRICT;

CREATE TABLE recall_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    intent TEXT NOT NULL,
    query_json TEXT NOT NULL CHECK (json_valid(query_json)),
    limits_json TEXT NOT NULL CHECK (json_valid(limits_json)),
    started_at_us INTEGER NOT NULL,
    completed_at_us INTEGER,
    status TEXT NOT NULL,
    CHECK (completed_at_us IS NULL OR completed_at_us >= started_at_us)
) STRICT;

CREATE TABLE recall_activations (
    recall_session_id TEXT NOT NULL,
    point_id TEXT NOT NULL,
    activation REAL NOT NULL CHECK (activation >= 0.0),
    depth INTEGER NOT NULL CHECK (depth >= 0),
    via_edge_id TEXT,
    created_at_us INTEGER NOT NULL,
    PRIMARY KEY (recall_session_id, point_id, depth),
    FOREIGN KEY (recall_session_id) REFERENCES recall_sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (point_id) REFERENCES points(id) ON DELETE CASCADE,
    FOREIGN KEY (via_edge_id) REFERENCES edges(id) ON DELETE SET NULL
) STRICT;

CREATE TABLE transitions (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    trace_id TEXT NOT NULL,
    from_stage TEXT NOT NULL,
    to_stage TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    created_at_us INTEGER NOT NULL
) STRICT;

CREATE TABLE hypotheses (
    id TEXT PRIMARY KEY NOT NULL,
    transition_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    validity TEXT NOT NULL,
    source_fragment_id TEXT NOT NULL,
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (transition_id) REFERENCES transitions(id) ON DELETE CASCADE,
    FOREIGN KEY (source_fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE capabilities (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    kind TEXT NOT NULL,
    version TEXT NOT NULL,
    source TEXT NOT NULL,
    permissions_json TEXT NOT NULL CHECK (json_valid(permissions_json)),
    input_schema_json TEXT NOT NULL CHECK (json_valid(input_schema_json)),
    output_schema_json TEXT NOT NULL CHECK (json_valid(output_schema_json)),
    risk_level TEXT NOT NULL,
    requires_approval INTEGER NOT NULL CHECK (requires_approval IN (0, 1)),
    created_at_us INTEGER NOT NULL,
    UNIQUE (id, version)
) STRICT;

CREATE TABLE approvals (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    transition_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    status TEXT NOT NULL,
    reason TEXT,
    created_at_us INTEGER NOT NULL,
    decided_at_us INTEGER,
    FOREIGN KEY (transition_id) REFERENCES transitions(id) ON DELETE CASCADE,
    CHECK (decided_at_us IS NULL OR decided_at_us >= created_at_us)
) STRICT;

CREATE TABLE decisions (
    id TEXT PRIMARY KEY NOT NULL,
    transition_id TEXT NOT NULL,
    candidate_id TEXT NOT NULL,
    policy_result_json TEXT NOT NULL CHECK (json_valid(policy_result_json)),
    approval_id TEXT,
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (transition_id) REFERENCES transitions(id) ON DELETE CASCADE,
    FOREIGN KEY (candidate_id) REFERENCES hypotheses(id) ON DELETE RESTRICT,
    FOREIGN KEY (approval_id) REFERENCES approvals(id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE actions (
    id TEXT PRIMARY KEY NOT NULL,
    transition_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    request_json TEXT NOT NULL CHECK (json_valid(request_json)),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) > 0),
    status TEXT NOT NULL,
    started_at_us INTEGER NOT NULL,
    completed_at_us INTEGER,
    FOREIGN KEY (transition_id) REFERENCES transitions(id) ON DELETE CASCADE,
    FOREIGN KEY (capability_id) REFERENCES capabilities(id) ON DELETE RESTRICT,
    UNIQUE (capability_id, idempotency_key),
    CHECK (completed_at_us IS NULL OR completed_at_us >= started_at_us)
) STRICT;

CREATE TABLE outcomes (
    id TEXT PRIMARY KEY NOT NULL,
    action_id TEXT NOT NULL,
    result_json TEXT NOT NULL CHECK (json_valid(result_json)),
    validity TEXT NOT NULL,
    source_fragment_id TEXT NOT NULL,
    created_at_us INTEGER NOT NULL,
    FOREIGN KEY (action_id) REFERENCES actions(id) ON DELETE CASCADE,
    FOREIGN KEY (source_fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE capability_runs (
    id TEXT PRIMARY KEY NOT NULL,
    capability_id TEXT NOT NULL,
    capability_version TEXT NOT NULL,
    transition_id TEXT NOT NULL,
    status TEXT NOT NULL,
    failure_mode TEXT,
    started_at_us INTEGER NOT NULL,
    completed_at_us INTEGER,
    FOREIGN KEY (capability_id, capability_version) REFERENCES capabilities(id, version) ON DELETE RESTRICT,
    FOREIGN KEY (transition_id) REFERENCES transitions(id) ON DELETE CASCADE,
    CHECK (completed_at_us IS NULL OR completed_at_us >= started_at_us)
) STRICT;

CREATE TABLE audit_logs (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    principal_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    detail_digest TEXT NOT NULL CHECK (length(detail_digest) = 64),
    created_at_us INTEGER NOT NULL
) STRICT;

CREATE TABLE snapshots (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    schema_version INTEGER NOT NULL CHECK (schema_version >= 1),
    manifest_json TEXT NOT NULL CHECK (json_valid(manifest_json)),
    checksum TEXT NOT NULL CHECK (length(checksum) = 64),
    created_at_us INTEGER NOT NULL
) STRICT;

CREATE TABLE quarantine (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    fragment_id TEXT NOT NULL,
    reason_code TEXT NOT NULL,
    status TEXT NOT NULL,
    reviewed_by TEXT,
    created_at_us INTEGER NOT NULL,
    reviewed_at_us INTEGER,
    FOREIGN KEY (fragment_id) REFERENCES fragments(id) ON DELETE RESTRICT,
    CHECK (reviewed_at_us IS NULL OR reviewed_at_us >= created_at_us)
) STRICT;

CREATE TABLE blob_refs (
    blob_id TEXT NOT NULL CHECK (length(blob_id) = 64),
    owner_type TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    ref_count INTEGER NOT NULL CHECK (ref_count >= 0),
    created_at_us INTEGER NOT NULL,
    updated_at_us INTEGER NOT NULL,
    PRIMARY KEY (blob_id, owner_type, owner_id),
    CHECK (updated_at_us >= created_at_us)
) STRICT;

CREATE TABLE consolidation_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    checkpoint_json TEXT NOT NULL CHECK (json_valid(checkpoint_json)),
    attempts INTEGER NOT NULL CHECK (attempts >= 0),
    resource_limits_json TEXT NOT NULL CHECK (json_valid(resource_limits_json)),
    created_at_us INTEGER NOT NULL,
    updated_at_us INTEGER NOT NULL,
    CHECK (updated_at_us >= created_at_us)
) STRICT;

CREATE VIRTUAL TABLE fragment_fts USING fts5(
    fragment_id UNINDEXED,
    scope_key UNINDEXED,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE INDEX idx_fragments_scope_occurred
    ON fragments(scope_key, occurred_at_us DESC);
CREATE INDEX idx_fragments_scope_observed
    ON fragments(scope_key, observed_at_us DESC);
CREATE INDEX idx_fragments_kind_validity
    ON fragments(scope_key, kind, validity, observed_at_us DESC);
CREATE INDEX idx_events_scope_sequence
    ON events(scope_key, sequence);
CREATE INDEX idx_events_fragment
    ON events(fragment_id, sequence);
CREATE INDEX idx_points_scope_validity
    ON points(scope_key, valid_from_us, valid_to_us);
CREATE INDEX idx_points_source_fragment
    ON points(source_fragment_id);
CREATE INDEX idx_edges_source_kind_validity
    ON edges(source_point_id, kind, valid_from_us, valid_to_us);
CREATE INDEX idx_edges_target_kind_validity
    ON edges(target_point_id, kind, valid_from_us, valid_to_us);
CREATE INDEX idx_edges_scope_model
    ON edges(scope_key, model_version_id, created_at_us);
CREATE INDEX idx_surfaces_scope_validity
    ON surfaces(scope_key, kind, status, valid_from_us, valid_to_us);
CREATE INDEX idx_surface_members_point
    ON surface_members(point_id, surface_id, version);
CREATE INDEX idx_entities_scope_name
    ON entities(scope_key, canonical_name);
CREATE INDEX idx_entity_aliases_alias
    ON entity_aliases(alias, entity_id);
CREATE INDEX idx_states_current_version
    ON states(current_version_id);
CREATE INDEX idx_state_versions_state_validity
    ON state_versions(state_id, valid_from_us, valid_to_us);
CREATE INDEX idx_evidence_refs_owner
    ON evidence_refs(owner_fragment_id, created_at_us);
CREATE INDEX idx_evidence_refs_fragment
    ON evidence_refs(referenced_fragment_id);
CREATE INDEX idx_evidence_refs_event
    ON evidence_refs(referenced_event_id);
CREATE INDEX idx_recall_sessions_scope_started
    ON recall_sessions(scope_key, started_at_us DESC);
CREATE INDEX idx_recall_sessions_query
    ON recall_sessions(scope_key, intent, status, started_at_us DESC);
CREATE INDEX idx_recall_activations_point
    ON recall_activations(point_id, activation DESC);
CREATE INDEX idx_transitions_trace
    ON transitions(scope_key, trace_id, created_at_us);
CREATE INDEX idx_hypotheses_transition
    ON hypotheses(transition_id, validity);
CREATE INDEX idx_decisions_transition
    ON decisions(transition_id, created_at_us);
CREATE INDEX idx_actions_transition_status
    ON actions(transition_id, status);
CREATE INDEX idx_outcomes_action
    ON outcomes(action_id, created_at_us);
CREATE INDEX idx_capabilities_scope_kind_version
    ON capabilities(scope_key, kind, version);
CREATE INDEX idx_capability_runs_transition
    ON capability_runs(transition_id, started_at_us);
CREATE INDEX idx_approvals_scope_status
    ON approvals(scope_key, status, created_at_us);
CREATE INDEX idx_audit_logs_scope_created
    ON audit_logs(scope_key, created_at_us DESC);
CREATE INDEX idx_snapshots_scope_created
    ON snapshots(scope_key, created_at_us DESC);
CREATE INDEX idx_quarantine_scope_status
    ON quarantine(scope_key, status, created_at_us);
CREATE INDEX idx_blob_refs_blob
    ON blob_refs(blob_id, ref_count);
CREATE INDEX idx_blob_refs_owner
    ON blob_refs(owner_type, owner_id);
CREATE INDEX idx_consolidation_jobs_scope_status
    ON consolidation_jobs(scope_key, status, updated_at_us);
