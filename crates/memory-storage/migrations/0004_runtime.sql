-- 为现有 Transition 审计表追加 Runtime 字段，旧行保持兼容。
ALTER TABLE transitions ADD COLUMN run_id TEXT;
ALTER TABLE transitions ADD COLUMN step INTEGER CHECK (step IS NULL OR step >= 0);
ALTER TABLE transitions ADD COLUMN input_refs_json TEXT
    CHECK (input_refs_json IS NULL OR json_valid(input_refs_json));
ALTER TABLE transitions ADD COLUMN output_refs_json TEXT
    CHECK (output_refs_json IS NULL OR json_valid(output_refs_json));
ALTER TABLE transitions ADD COLUMN attempt INTEGER CHECK (attempt IS NULL OR attempt > 0);
ALTER TABLE transitions ADD COLUMN deadline_at_us INTEGER;

-- 当前快照只保存运行控制状态，不复制 Fragment、Event 或 EvidenceRef 权威记录。
CREATE TABLE runtime_runs (
    run_id TEXT PRIMARY KEY NOT NULL,
    scope_key TEXT NOT NULL CHECK (length(scope_key) = 64),
    scope_json TEXT NOT NULL CHECK (json_valid(scope_json)),
    state TEXT NOT NULL CHECK (state IN (
        'observing', 'recalling', 'reasoning', 'awaiting_approval', 'executing',
        'committing', 'waiting_user', 'paused', 'completed', 'failed', 'cancelled'
    )),
    next_step INTEGER NOT NULL CHECK (next_step >= 0),
    version INTEGER NOT NULL CHECK (version >= 0),
    created_at_us INTEGER NOT NULL,
    updated_at_us INTEGER NOT NULL CHECK (updated_at_us >= created_at_us)
) STRICT;

CREATE UNIQUE INDEX idx_transitions_runtime_run_step
    ON transitions(run_id, step)
    WHERE run_id IS NOT NULL;

CREATE INDEX idx_runtime_runs_scope_state
    ON runtime_runs(scope_key, state, updated_at_us);

-- Runtime 写入必须提供完整字段，并且只能关联同一 Scope 的现有 run。
CREATE TRIGGER validate_runtime_transition_insert
BEFORE INSERT ON transitions
WHEN NEW.run_id IS NOT NULL AND (
    NEW.step IS NULL
    OR NEW.input_refs_json IS NULL
    OR NEW.output_refs_json IS NULL
    OR NEW.attempt IS NULL
    OR NEW.deadline_at_us IS NULL
    OR NOT EXISTS (
        SELECT 1 FROM runtime_runs
        WHERE run_id = NEW.run_id AND scope_key = NEW.scope_key
    )
)
BEGIN
    SELECT RAISE(ABORT, 'incomplete or orphan runtime transition');
END;

CREATE TRIGGER protect_runtime_transition_update
BEFORE UPDATE ON transitions
WHEN OLD.run_id IS NOT NULL OR NEW.run_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'runtime transition is immutable');
END;

CREATE TRIGGER protect_runtime_transition_delete
BEFORE DELETE ON transitions
WHEN OLD.run_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'runtime transition is immutable');
END;

-- Run 身份与 Scope 不可变，但 Runner 可以推进状态、步骤和版本。
CREATE TRIGGER protect_runtime_run_identity
BEFORE UPDATE ON runtime_runs
WHEN NEW.run_id != OLD.run_id
    OR NEW.scope_key != OLD.scope_key
    OR NEW.scope_json != OLD.scope_json
BEGIN
    SELECT RAISE(ABORT, 'runtime run identity is immutable');
END;

CREATE TRIGGER protect_runtime_run_delete
BEFORE DELETE ON runtime_runs
BEGIN
    SELECT RAISE(ABORT, 'runtime run cannot be deleted');
END;
