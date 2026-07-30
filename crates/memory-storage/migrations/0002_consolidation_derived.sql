CREATE TABLE surfaces_v2 (
    id TEXT NOT NULL,
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
    PRIMARY KEY (id, version),
    FOREIGN KEY (model_version_id) REFERENCES model_versions(id) ON DELETE RESTRICT,
    CHECK (valid_to_us IS NULL OR valid_to_us >= valid_from_us)
) STRICT;

INSERT INTO surfaces_v2 SELECT * FROM surfaces;

CREATE TABLE surface_members_v2 (
    surface_id TEXT NOT NULL,
    point_id TEXT NOT NULL,
    member_weight REAL NOT NULL CHECK (member_weight >= 0.0 AND member_weight <= 1.0),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_at_us INTEGER NOT NULL,
    PRIMARY KEY (surface_id, point_id, version),
    FOREIGN KEY (surface_id, version) REFERENCES surfaces_v2(id, version) ON DELETE CASCADE,
    FOREIGN KEY (point_id) REFERENCES points(id) ON DELETE CASCADE
) STRICT;

INSERT INTO surface_members_v2 SELECT * FROM surface_members;
DROP TABLE surface_members;
DROP TABLE surfaces;
ALTER TABLE surfaces_v2 RENAME TO surfaces;
ALTER TABLE surface_members_v2 RENAME TO surface_members;

CREATE TABLE consolidation_artifacts (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    kind TEXT NOT NULL,
    subject_key TEXT NOT NULL,
    source_cursor INTEGER NOT NULL CHECK (source_cursor >= 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    evidence_json TEXT NOT NULL CHECK (json_valid(evidence_json)),
    created_at_us INTEGER NOT NULL,
    UNIQUE (scope_key, kind, subject_key, source_cursor)
) STRICT;

CREATE TABLE cold_tier_entries (
    fragment_id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    tier TEXT NOT NULL CHECK (tier IN ('warm', 'cold')),
    source_event_id TEXT NOT NULL,
    evidence_json TEXT NOT NULL CHECK (json_valid(evidence_json)),
    moved_at_us INTEGER NOT NULL,
    FOREIGN KEY (fragment_id) REFERENCES fragments(id) ON DELETE CASCADE,
    FOREIGN KEY (source_event_id) REFERENCES events(id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX idx_surfaces_scope_validity
    ON surfaces(scope_key, kind, status, valid_from_us, valid_to_us);
CREATE INDEX idx_surface_members_point
    ON surface_members(point_id, surface_id, version);
CREATE INDEX idx_consolidation_artifacts_scope_cursor
    ON consolidation_artifacts(scope_key, kind, source_cursor);
CREATE INDEX idx_cold_tier_entries_scope_tier
    ON cold_tier_entries(scope_key, tier, moved_at_us);
