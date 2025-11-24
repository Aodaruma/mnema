-- tasks, projects, lists, milestones, statuses, status_groups, user_settings

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    start_date TEXT,
    end_date TEXT,
    default_status_set_id TEXT,
    archived_at TEXT
);

CREATE TABLE IF NOT EXISTS status_groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS statuses (
    id TEXT PRIMARY KEY,
    project_id TEXT NULL,
    name TEXT NOT NULL,
    group_id TEXT NOT NULL,
    "order" INTEGER NOT NULL,
    FOREIGN KEY(group_id) REFERENCES status_groups(id),
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS lists (
    id TEXT PRIMARY KEY,
    project_id TEXT NULL,
    name TEXT NOT NULL,
    is_system INTEGER NOT NULL,
    kind TEXT NOT NULL,
    view_type TEXT NOT NULL,
    "order" INTEGER NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS milestones (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    target_date TEXT NOT NULL,
    status TEXT NOT NULL,
    dependency_task_ids TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    project_id TEXT NULL,
    list_id TEXT NULL,
    status_id TEXT NOT NULL,
    due_date TEXT,
    start_date TEXT,
    estimated_minutes INTEGER,
    cost_points INTEGER,
    dependencies TEXT NOT NULL,
    milestone_id TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    FOREIGN KEY(project_id) REFERENCES projects(id),
    FOREIGN KEY(list_id) REFERENCES lists(id),
    FOREIGN KEY(status_id) REFERENCES statuses(id),
    FOREIGN KEY(milestone_id) REFERENCES milestones(id)
);

CREATE TABLE IF NOT EXISTS user_settings (
    user_id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    model_for_planning TEXT,
    model_for_routine TEXT,
    automation TEXT NOT NULL,
    weekly_review TEXT NOT NULL
);
