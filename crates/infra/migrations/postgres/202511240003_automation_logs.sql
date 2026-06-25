CREATE TABLE IF NOT EXISTS automation_logs (
    id UUID PRIMARY KEY,
    task_id UUID NULL,
    project_id UUID NULL,
    list_id UUID NULL,
    assistant_id UUID NOT NULL,
    action_type TEXT NOT NULL,
    before_state JSONB,
    after_state JSONB,
    created_at TIMESTAMPTZ NOT NULL,
    explanation TEXT,
    FOREIGN KEY(task_id) REFERENCES tasks(id),
    FOREIGN KEY(project_id) REFERENCES projects(id),
    FOREIGN KEY(list_id) REFERENCES lists(id)
);

CREATE INDEX IF NOT EXISTS idx_automation_logs_created_at
    ON automation_logs(created_at);

CREATE INDEX IF NOT EXISTS idx_automation_logs_task_id
    ON automation_logs(task_id);
