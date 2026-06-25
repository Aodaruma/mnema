CREATE TABLE IF NOT EXISTS schedule_blocks (
    id TEXT PRIMARY KEY,
    task_id TEXT NULL,
    title_snapshot TEXT,
    start_at TEXT NOT NULL,
    end_at TEXT NOT NULL,
    block_type TEXT NOT NULL,
    state TEXT NOT NULL,
    locked INTEGER NOT NULL,
    source TEXT NOT NULL,
    required_minutes INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(task_id) REFERENCES tasks(id)
);

CREATE INDEX IF NOT EXISTS idx_schedule_blocks_start_at
    ON schedule_blocks(start_at);

CREATE INDEX IF NOT EXISTS idx_schedule_blocks_task_id
    ON schedule_blocks(task_id);
