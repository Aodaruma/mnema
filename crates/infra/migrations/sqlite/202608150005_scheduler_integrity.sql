-- Rebuild the link table without the schedule block foreign key. The account
-- relationship remains cascading, while a deleted local block leaves enough
-- remote identity for write-back cleanup.
CREATE TABLE managed_calendar_event_links_v2 (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    schedule_block_id TEXT NOT NULL,
    calendar_id TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    etag TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(account_id) REFERENCES calendar_accounts(id) ON DELETE CASCADE,
    UNIQUE(account_id, schedule_block_id),
    UNIQUE(account_id, calendar_id, provider_event_id)
);

INSERT INTO managed_calendar_event_links_v2 (
    id, account_id, schedule_block_id, calendar_id, provider_event_id, etag,
    created_at, updated_at
)
SELECT
    id, account_id, schedule_block_id, calendar_id, provider_event_id, etag,
    created_at, updated_at
FROM managed_calendar_event_links;

DROP TABLE managed_calendar_event_links;
ALTER TABLE managed_calendar_event_links_v2 RENAME TO managed_calendar_event_links;

-- SQLite range predicates compare TEXT lexicographically. Normalize every
-- persisted instant to the same millisecond-width UTC representation. CASE
-- retains an unexpected legacy value rather than replacing it with NULL.
UPDATE projects
SET archived_at = CASE
    WHEN archived_at IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', archived_at) IS NULL THEN archived_at
    ELSE strftime('%Y-%m-%dT%H:%M:%f', archived_at) || '000000Z'
END;

UPDATE tasks
SET created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END,
    deleted_at = CASE WHEN deleted_at IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', deleted_at) IS NULL THEN deleted_at ELSE strftime('%Y-%m-%dT%H:%M:%f', deleted_at) || '000000Z' END;

UPDATE milestones
SET created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END;

UPDATE schedule_blocks
SET start_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', start_at) IS NULL THEN start_at ELSE strftime('%Y-%m-%dT%H:%M:%f', start_at) || '000000Z' END,
    end_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', end_at) IS NULL THEN end_at ELSE strftime('%Y-%m-%dT%H:%M:%f', end_at) || '000000Z' END,
    created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END;

UPDATE automation_logs
SET created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END;

UPDATE calendar_accounts
SET created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END;

UPDATE calendar_sync_cursors
SET last_full_sync_at = CASE WHEN last_full_sync_at IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', last_full_sync_at) IS NULL THEN last_full_sync_at ELSE strftime('%Y-%m-%dT%H:%M:%f', last_full_sync_at) || '000000Z' END,
    last_incremental_sync_at = CASE WHEN last_incremental_sync_at IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', last_incremental_sync_at) IS NULL THEN last_incremental_sync_at ELSE strftime('%Y-%m-%dT%H:%M:%f', last_incremental_sync_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END;

UPDATE external_events
SET original_start_at = CASE WHEN original_start_at IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', original_start_at) IS NULL THEN original_start_at ELSE strftime('%Y-%m-%dT%H:%M:%f', original_start_at) || '000000Z' END,
    start_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', start_at) IS NULL THEN start_at ELSE strftime('%Y-%m-%dT%H:%M:%f', start_at) || '000000Z' END,
    end_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', end_at) IS NULL THEN end_at ELSE strftime('%Y-%m-%dT%H:%M:%f', end_at) || '000000Z' END,
    provider_updated_at = CASE WHEN provider_updated_at IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', provider_updated_at) IS NULL THEN provider_updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', provider_updated_at) || '000000Z' END,
    created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END;

UPDATE managed_calendar_event_links
SET created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END;

UPDATE habits
SET disabled_at = CASE WHEN disabled_at IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', disabled_at) IS NULL THEN disabled_at ELSE strftime('%Y-%m-%dT%H:%M:%f', disabled_at) || '000000Z' END,
    created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END;

UPDATE habit_occurrences
SET scheduled_start_at = CASE WHEN scheduled_start_at IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', scheduled_start_at) IS NULL THEN scheduled_start_at ELSE strftime('%Y-%m-%dT%H:%M:%f', scheduled_start_at) || '000000Z' END,
    scheduled_end_at = CASE WHEN scheduled_end_at IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', scheduled_end_at) IS NULL THEN scheduled_end_at ELSE strftime('%Y-%m-%dT%H:%M:%f', scheduled_end_at) || '000000Z' END,
    snoozed_until = CASE WHEN snoozed_until IS NULL OR strftime('%Y-%m-%dT%H:%M:%fZ', snoozed_until) IS NULL THEN snoozed_until ELSE strftime('%Y-%m-%dT%H:%M:%f', snoozed_until) || '000000Z' END,
    created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END;

UPDATE scheduling_preferences
SET created_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', created_at) IS NULL THEN created_at ELSE strftime('%Y-%m-%dT%H:%M:%f', created_at) || '000000Z' END,
    updated_at = CASE WHEN strftime('%Y-%m-%dT%H:%M:%fZ', updated_at) IS NULL THEN updated_at ELSE strftime('%Y-%m-%dT%H:%M:%f', updated_at) || '000000Z' END;
