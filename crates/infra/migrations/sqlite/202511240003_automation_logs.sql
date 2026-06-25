CREATE TABLE IF NOT EXISTS automation_logs (
    id TEXT PRIMARY KEY,
    task_id TEXT NULL,
    project_id TEXT NULL,
    list_id TEXT NULL,
    assistant_id TEXT NOT NULL,
    action_type TEXT NOT NULL,
    before_state TEXT,
    after_state TEXT,
    created_at TEXT NOT NULL,
    explanation TEXT,
    FOREIGN KEY(task_id) REFERENCES tasks(id),
    FOREIGN KEY(project_id) REFERENCES projects(id),
    FOREIGN KEY(list_id) REFERENCES lists(id)
);

CREATE INDEX IF NOT EXISTS idx_automation_logs_created_at
    ON automation_logs(created_at);

CREATE INDEX IF NOT EXISTS idx_automation_logs_task_id
    ON automation_logs(task_id);
