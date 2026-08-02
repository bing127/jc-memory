ALTER TABLE state_versions ADD COLUMN reason TEXT;
ALTER TABLE state_versions ADD COLUMN source_event_id TEXT REFERENCES events(id) ON DELETE RESTRICT;

CREATE TABLE projection_checkpoints (
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    projector TEXT NOT NULL CHECK (projector IN ('state', 'graph')),
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    updated_at_us INTEGER NOT NULL,
    PRIMARY KEY (scope_key, projector)
) STRICT;

CREATE INDEX idx_state_versions_source_fragment
    ON state_versions(source_fragment_id);
CREATE INDEX idx_state_versions_source_event
    ON state_versions(source_event_id);
CREATE INDEX idx_state_versions_state_effective
    ON state_versions(state_id, valid_from_us DESC, valid_to_us, validity);
CREATE INDEX idx_points_scope_source_fragment
    ON points(scope_key, source_fragment_id);
