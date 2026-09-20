CREATE TABLE IF NOT EXISTS calendar_accounts (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    provider TEXT NOT NULL,
    provider_account_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    email TEXT,
    access_mode TEXT NOT NULL,
    enabled BOOLEAN NOT NULL,
    selected_calendar_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    managed_calendar_id TEXT,
    timezone TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    UNIQUE(provider, provider_account_id)
);

CREATE INDEX IF NOT EXISTS idx_calendar_accounts_user_enabled
    ON calendar_accounts(user_id, enabled);

CREATE TABLE IF NOT EXISTS calendar_sync_cursors (
    account_id UUID NOT NULL REFERENCES calendar_accounts(id) ON DELETE CASCADE,
    calendar_id TEXT NOT NULL,
    sync_token TEXT,
    last_full_sync_at TIMESTAMPTZ,
    last_incremental_sync_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY(account_id, calendar_id)
);

CREATE TABLE IF NOT EXISTS external_events (
    id UUID PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES calendar_accounts(id) ON DELETE CASCADE,
    calendar_id TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    recurring_event_id TEXT,
    original_start_at TIMESTAMPTZ,
    title TEXT NOT NULL,
    description TEXT,
    location TEXT,
    start_at TIMESTAMPTZ NOT NULL,
    end_at TIMESTAMPTZ NOT NULL,
    all_day BOOLEAN NOT NULL,
    timezone TEXT,
    status TEXT NOT NULL,
    transparency TEXT NOT NULL,
    needs_travel BOOLEAN,
    travel_before_minutes INTEGER,
    travel_after_minutes INTEGER,
    etag TEXT,
    provider_updated_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
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
    id UUID PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES calendar_accounts(id) ON DELETE CASCADE,
    schedule_block_id UUID NOT NULL REFERENCES schedule_blocks(id) ON DELETE CASCADE,
    calendar_id TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    etag TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    UNIQUE(account_id, schedule_block_id),
    UNIQUE(account_id, calendar_id, provider_event_id)
);

CREATE TABLE IF NOT EXISTS habits (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    title TEXT NOT NULL,
    schedule JSONB NOT NULL,
    duration_minutes INTEGER NOT NULL,
    preferred_window JSONB,
    flexibility TEXT NOT NULL,
    enabled BOOLEAN NOT NULL,
    disabled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CHECK(duration_minutes > 0)
);

CREATE INDEX IF NOT EXISTS idx_habits_user_enabled
    ON habits(user_id, enabled);

CREATE TABLE IF NOT EXISTS habit_occurrences (
    id UUID PRIMARY KEY,
    habit_id UUID NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
    occurrence_date DATE NOT NULL,
    state TEXT NOT NULL,
    scheduled_start_at TIMESTAMPTZ,
    scheduled_end_at TIMESTAMPTZ,
    snoozed_until TIMESTAMPTZ,
    skip_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
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
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL UNIQUE,
    timezone TEXT NOT NULL,
    named_hours JSONB NOT NULL,
    sleep JSONB,
    default_travel_buffer_minutes INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CHECK(default_travel_buffer_minutes >= 0)
);

ALTER TABLE schedule_blocks
    ADD COLUMN IF NOT EXISTS habit_occurrence_id UUID REFERENCES habit_occurrences(id);

CREATE INDEX IF NOT EXISTS idx_schedule_blocks_habit_occurrence_id
    ON schedule_blocks(habit_occurrence_id);
