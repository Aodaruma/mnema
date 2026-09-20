CREATE TABLE IF NOT EXISTS calendar_accounts (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_account_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    email TEXT,
    access_mode TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    selected_calendar_ids TEXT NOT NULL DEFAULT '[]',
    managed_calendar_id TEXT,
    timezone TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(provider, provider_account_id)
);

CREATE INDEX IF NOT EXISTS idx_calendar_accounts_user_enabled
    ON calendar_accounts(user_id, enabled);

CREATE TABLE IF NOT EXISTS calendar_sync_cursors (
    account_id TEXT NOT NULL,
    calendar_id TEXT NOT NULL,
    sync_token TEXT,
    last_full_sync_at TEXT,
    last_incremental_sync_at TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(account_id, calendar_id),
    FOREIGN KEY(account_id) REFERENCES calendar_accounts(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS external_events (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    calendar_id TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    recurring_event_id TEXT,
    original_start_at TEXT,
    title TEXT NOT NULL,
    description TEXT,
    location TEXT,
    start_at TEXT NOT NULL,
    end_at TEXT NOT NULL,
    all_day INTEGER NOT NULL,
    timezone TEXT,
    status TEXT NOT NULL,
    transparency TEXT NOT NULL,
    needs_travel INTEGER,
    travel_before_minutes INTEGER,
    travel_after_minutes INTEGER,
    etag TEXT,
    provider_updated_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(account_id) REFERENCES calendar_accounts(id) ON DELETE CASCADE,
    UNIQUE(account_id, calendar_id, provider_event_id),
    CHECK(end_at > start_at),
    CHECK(travel_before_minutes IS NULL OR travel_before_minutes >= 0),
    CHECK(travel_after_minutes IS NULL OR travel_after_minutes >= 0)
);

CREATE INDEX IF NOT EXISTS idx_external_events_overlap
    ON external_events(start_at, end_at);

CREATE INDEX IF NOT EXISTS idx_external_events_account_overlap
    ON external_events(account_id, start_at, end_at);

CREATE TABLE IF NOT EXISTS managed_calendar_event_links (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    schedule_block_id TEXT NOT NULL,
    calendar_id TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    etag TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(account_id) REFERENCES calendar_accounts(id) ON DELETE CASCADE,
    FOREIGN KEY(schedule_block_id) REFERENCES schedule_blocks(id) ON DELETE CASCADE,
    UNIQUE(account_id, schedule_block_id),
    UNIQUE(account_id, calendar_id, provider_event_id)
);

CREATE TABLE IF NOT EXISTS habits (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    title TEXT NOT NULL,
    schedule TEXT NOT NULL,
    duration_minutes INTEGER NOT NULL,
    preferred_window TEXT,
    flexibility TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    disabled_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(duration_minutes > 0)
);

CREATE INDEX IF NOT EXISTS idx_habits_user_enabled
    ON habits(user_id, enabled);

CREATE TABLE IF NOT EXISTS habit_occurrences (
    id TEXT PRIMARY KEY,
    habit_id TEXT NOT NULL,
    occurrence_date TEXT NOT NULL,
    state TEXT NOT NULL,
    scheduled_start_at TEXT,
    scheduled_end_at TEXT,
    snoozed_until TEXT,
    skip_reason TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(habit_id) REFERENCES habits(id) ON DELETE CASCADE,
    UNIQUE(habit_id, occurrence_date),
    CHECK(
        scheduled_start_at IS NULL
        OR scheduled_end_at IS NULL
        OR scheduled_end_at > scheduled_start_at
    )
);

CREATE INDEX IF NOT EXISTS idx_habit_occurrences_habit_date
    ON habit_occurrences(habit_id, occurrence_date);

CREATE TABLE IF NOT EXISTS scheduling_preferences (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL UNIQUE,
    timezone TEXT NOT NULL,
    named_hours TEXT NOT NULL,
    sleep TEXT,
    default_travel_buffer_minutes INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(default_travel_buffer_minutes >= 0)
);

ALTER TABLE schedule_blocks
    ADD COLUMN habit_occurrence_id TEXT REFERENCES habit_occurrences(id);

-- SQLite compares RFC3339 timestamps lexicographically. Normalize legacy
-- offset-bearing rows so overlap predicates remain chronological across zones.
UPDATE schedule_blocks
SET start_at = strftime('%Y-%m-%dT%H:%M:%fZ', start_at),
    end_at = strftime('%Y-%m-%dT%H:%M:%fZ', end_at),
    created_at = strftime('%Y-%m-%dT%H:%M:%fZ', created_at),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', updated_at);

CREATE INDEX IF NOT EXISTS idx_schedule_blocks_habit_occurrence_id
    ON schedule_blocks(habit_occurrence_id);
