-- Initial PostgreSQL schema for Mnema.

CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    start_date DATE,
    end_date DATE,
    default_status_set_id UUID,
    archived_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS status_groups (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS statuses (
    id UUID PRIMARY KEY,
    project_id UUID NULL,
    name TEXT NOT NULL,
    group_id UUID NOT NULL,
    "order" INTEGER NOT NULL,
    FOREIGN KEY(group_id) REFERENCES status_groups(id),
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS lists (
    id UUID PRIMARY KEY,
    project_id UUID NULL,
    name TEXT NOT NULL,
    is_system BOOLEAN NOT NULL,
    kind TEXT NOT NULL,
    view_type TEXT NOT NULL,
    "order" INTEGER NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS milestones (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    target_date DATE NOT NULL,
    status TEXT NOT NULL,
    dependency_task_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS tasks (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    project_id UUID NULL,
    list_id UUID NULL,
    status_id UUID NOT NULL,
    due_date DATE,
    start_date DATE,
    estimated_minutes INTEGER,
    cost_points INTEGER,
    dependencies JSONB NOT NULL DEFAULT '[]'::jsonb,
    milestone_id UUID NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    deleted_at TIMESTAMPTZ,
    FOREIGN KEY(project_id) REFERENCES projects(id),
    FOREIGN KEY(list_id) REFERENCES lists(id),
    FOREIGN KEY(status_id) REFERENCES statuses(id),
    FOREIGN KEY(milestone_id) REFERENCES milestones(id)
);

CREATE TABLE IF NOT EXISTS user_settings (
    user_id UUID PRIMARY KEY,
    provider TEXT NOT NULL,
    model_for_planning TEXT,
    model_for_routine TEXT,
    automation JSONB NOT NULL,
    weekly_review JSONB NOT NULL
);
