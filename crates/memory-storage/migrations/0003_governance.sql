ALTER TABLE audit_logs
    ADD COLUMN target_digest TEXT NOT NULL
    DEFAULT '0000000000000000000000000000000000000000000000000000000000000000'
    CHECK (length(target_digest) = 64);

ALTER TABLE audit_logs
    ADD COLUMN request_id TEXT NOT NULL DEFAULT '';

ALTER TABLE audit_logs
    ADD COLUMN result TEXT NOT NULL DEFAULT 'unknown'
    CHECK (result IN ('succeeded', 'denied', 'failed', 'unknown'));

CREATE TABLE governance_receipts (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    operation TEXT NOT NULL,
    target_digest TEXT NOT NULL CHECK (length(target_digest) = 64),
    request_id TEXT NOT NULL,
    result TEXT NOT NULL CHECK (result IN ('succeeded', 'failed')),
    receipt_json TEXT NOT NULL CHECK (json_valid(receipt_json)),
    created_at_us INTEGER NOT NULL,
    UNIQUE (request_id, operation)
) STRICT;

CREATE INDEX idx_governance_receipts_scope_created
    ON governance_receipts(scope_key, created_at_us DESC);

CREATE TABLE retention_plans (
    id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    cutoff_us INTEGER NOT NULL,
    plan_digest TEXT NOT NULL CHECK (length(plan_digest) = 64),
    plan_json TEXT NOT NULL CHECK (json_valid(plan_json)),
    progress_json TEXT NOT NULL CHECK (json_valid(progress_json)),
    status TEXT NOT NULL CHECK (status IN ('planned', 'executing', 'completed', 'failed')),
    created_at_us INTEGER NOT NULL,
    updated_at_us INTEGER NOT NULL,
    CHECK (updated_at_us >= created_at_us)
) STRICT;

CREATE INDEX idx_retention_plans_scope_status
    ON retention_plans(scope_key, status, created_at_us DESC);
