-- 保存 Runtime 控制层检查点，不复制 Fragment、Event 或 EvidenceRef 权威数据。
CREATE TABLE runtime_control_checkpoints (
    run_id TEXT PRIMARY KEY NOT NULL,
    control_state TEXT NOT NULL CHECK (control_state IN ('running', 'waiting_user', 'paused', 'cancelled')),
    resume_state TEXT NOT NULL CHECK (resume_state IN (
        'observing', 'recalling', 'reasoning', 'awaiting_approval', 'executing',
        'committing', 'waiting_user', 'paused', 'completed', 'failed', 'cancelled'
    )),
    user_input TEXT,
    next_attempt_at_us INTEGER,
    attempt INTEGER NOT NULL CHECK (attempt >= 0),
    updated_at_us INTEGER NOT NULL,
    FOREIGN KEY (run_id) REFERENCES runtime_runs(run_id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER protect_runtime_control_checkpoint_delete
BEFORE DELETE ON runtime_control_checkpoints
BEGIN
    SELECT RAISE(ABORT, 'runtime control checkpoint cannot be deleted');
END;
