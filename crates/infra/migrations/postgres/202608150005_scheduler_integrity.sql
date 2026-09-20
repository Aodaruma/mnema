-- Keep the remote event identity after a stale local proposal is removed so
-- write-back can still delete the orphaned provider event.
ALTER TABLE managed_calendar_event_links
    DROP CONSTRAINT IF EXISTS managed_calendar_event_links_schedule_block_id_fkey;
