CREATE TABLE IF NOT EXISTS schedule_blocks (
    id UUID PRIMARY KEY,
    task_id UUID NULL,
    title_snapshot TEXT,
    start_at TIMESTAMPTZ NOT NULL,
    end_at TIMESTAMPTZ NOT NULL,
    block_type TEXT NOT NULL,
    state TEXT NOT NULL,
    locked BOOLEAN NOT NULL,
    source TEXT NOT NULL,
    required_minutes INTEGER,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    FOREIGN KEY(task_id) REFERENCES tasks(id)
);

CREATE INDEX IF NOT EXISTS idx_schedule_blocks_start_at
    ON schedule_blocks(start_at);

CREATE INDEX IF NOT EXISTS idx_schedule_blocks_task_id
    ON schedule_blocks(task_id);
