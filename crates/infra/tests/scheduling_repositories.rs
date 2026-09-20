use mnema_core::prelude::*;
use mnema_infra::db::Vault;
use sqlx::Row;
use tempfile::{TempDir, tempdir};
use time::UtcOffset;
use time::macros::{date, datetime, time};

async fn test_vault() -> anyhow::Result<(TempDir, Vault)> {
    let dir = tempdir()?;
    let sqlite_path = dir.path().join("scheduling.sqlite");
    let vault = Vault::connect_or_init_with_sqlite_path(dir.path(), sqlite_path).await?;
    Ok((dir, vault))
}

#[tokio::test]
async fn sqlite_migration_normalizes_legacy_offset_schedule_blocks() -> anyhow::Result<()> {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await?;
    for migration in [
        include_str!("../migrations/sqlite/202511240001_init.sql"),
        include_str!("../migrations/sqlite/202511240002_schedule_blocks.sql"),
        include_str!("../migrations/sqlite/202511240003_automation_logs.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&pool).await?;
    }
    sqlx::query(
        r#"
        INSERT INTO schedule_blocks (
            id, task_id, title_snapshot, start_at, end_at, block_type, state,
            locked, source, required_minutes, created_at, updated_at
        ) VALUES (?, NULL, 'legacy', ?, ?, 'TASK', 'PROPOSED', 0, 'SCHEDULER', 30, ?, ?)
        "#,
    )
    .bind(ScheduleBlockId::new().0.to_string())
    .bind("2026-08-17T09:00:00+09:00")
    .bind("2026-08-17T09:30:00+09:00")
    .bind("2026-08-15T09:00:00+09:00")
    .bind("2026-08-15T09:00:00+09:00")
    .execute(&pool)
    .await?;

    sqlx::raw_sql(include_str!(
        "../migrations/sqlite/202608150004_calendar_habits_scheduling.sql"
    ))
    .execute(&pool)
    .await?;
    sqlx::raw_sql(include_str!(
        "../migrations/sqlite/202608150005_scheduler_integrity.sql"
    ))
    .execute(&pool)
    .await?;
    let row = sqlx::query(
        r#"
        SELECT start_at, end_at FROM schedule_blocks
        WHERE start_at < ? AND end_at > ?
        "#,
    )
    .bind("2026-08-17T00:20:00Z")
    .bind("2026-08-17T00:10:00Z")
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        row.try_get::<String, _>("start_at")?,
        "2026-08-17T00:00:00.000000000Z"
    );
    assert_eq!(
        row.try_get::<String, _>("end_at")?,
        "2026-08-17T00:30:00.000000000Z"
    );
    let repo = mnema_infra::db::SqliteScheduleBlockRepository::new(pool);
    assert!(
        repo.list_overlapping(
            datetime!(2026-08-16 23:30 UTC),
            datetime!(2026-08-17 00:00 UTC),
        )
        .await?
        .is_empty(),
        "a block starting exactly at the range end must not compare as overlapping"
    );
    Ok(())
}

fn account(user_id: UserId, now: time::OffsetDateTime) -> CalendarAccount {
    CalendarAccount {
        id: CalendarAccountId::new(),
        user_id,
        provider: CalendarProvider::Google,
        provider_account_id: "google-user-1".into(),
        display_name: "Calendar account".into(),
        email: Some("person@example.com".into()),
        access_mode: CalendarAccessMode::ReadWrite,
        enabled: true,
        selected_calendar_ids: vec!["primary".into(), "team".into()],
        managed_calendar_id: Some("mnema-managed".into()),
        timezone: Some("Asia/Tokyo".into()),
        created_at: now,
        updated_at: now,
    }
}

fn proposed_block(
    id: ScheduleBlockId,
    start_at: time::OffsetDateTime,
    locked: bool,
    now: time::OffsetDateTime,
) -> ScheduleBlock {
    ScheduleBlock {
        id,
        task_id: None,
        habit_occurrence_id: None,
        title_snapshot: Some("Proposal".into()),
        start_at,
        end_at: start_at + time::Duration::minutes(30),
        block_type: ScheduleBlockType::Task,
        state: ScheduleBlockState::Proposed,
        locked,
        source: ScheduleBlockSource::Scheduler,
        required_minutes: Some(30),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn locked_proposals_and_orphan_writeback_links_survive_stale_replacement()
-> anyhow::Result<()> {
    let (_dir, vault) = test_vault().await?;
    let now = datetime!(2026-08-15 00:00 UTC);
    let account = account(UserId::new(), now);
    let account_id = account.id.clone();
    vault.calendar_account_repo().upsert(account).await?;

    let blocks = vault.schedule_block_repo();
    let links = vault.managed_calendar_event_link_repo();
    let locked_id = ScheduleBlockId::new();
    let stale_day_id = ScheduleBlockId::new();
    for block in [
        proposed_block(
            locked_id.clone(),
            datetime!(2026-08-17 09:00 UTC),
            true,
            now,
        ),
        proposed_block(
            stale_day_id.clone(),
            datetime!(2026-08-17 10:00 UTC),
            false,
            now,
        ),
    ] {
        blocks.insert(block).await?;
    }
    for (block_id, provider_event_id) in [
        (locked_id.clone(), "remote-locked"),
        (stale_day_id.clone(), "remote-stale-day"),
    ] {
        links
            .upsert(ManagedCalendarEventLink {
                id: ManagedCalendarEventId::new(),
                account_id: account_id.clone(),
                schedule_block_id: block_id,
                calendar_id: "mnema-managed".into(),
                provider_event_id: provider_event_id.into(),
                etag: None,
                created_at: now,
                updated_at: now,
            })
            .await?;
    }

    blocks
        .replace_proposed_for_day(date!(2026 - 08 - 17), Vec::new())
        .await?;
    assert!(blocks.find(locked_id.clone()).await?.is_some());
    assert!(blocks.find(stale_day_id.clone()).await?.is_none());
    assert!(
        links
            .find_by_schedule_block(account_id.clone(), stale_day_id)
            .await?
            .is_some(),
        "the remote identity must remain after local stale deletion"
    );

    let stale_range_id = ScheduleBlockId::new();
    blocks
        .insert(proposed_block(
            stale_range_id.clone(),
            datetime!(2026-08-17 11:00 UTC),
            false,
            now,
        ))
        .await?;
    links
        .upsert(ManagedCalendarEventLink {
            id: ManagedCalendarEventId::new(),
            account_id: account_id.clone(),
            schedule_block_id: stale_range_id.clone(),
            calendar_id: "mnema-managed".into(),
            provider_event_id: "remote-stale-range".into(),
            etag: None,
            created_at: now,
            updated_at: now,
        })
        .await?;
    blocks
        .replace_proposed_in_range(
            datetime!(2026-08-17 00:00 UTC),
            datetime!(2026-08-18 00:00 UTC),
            Vec::new(),
        )
        .await?;
    assert!(blocks.find(locked_id).await?.is_some());
    assert!(blocks.find(stale_range_id.clone()).await?.is_none());
    assert!(
        links
            .find_by_schedule_block(account_id.clone(), stale_range_id)
            .await?
            .is_some()
    );
    assert_eq!(links.list_for_account(account_id).await?.len(), 3);
    Ok(())
}

#[tokio::test]
async fn calendar_sync_is_per_calendar_and_event_upsert_preserves_overrides() -> anyhow::Result<()>
{
    let (_dir, vault) = test_vault().await?;
    let now = datetime!(2026-08-15 00:00 UTC);
    let account = account(UserId::new(), now);
    let account_id = account.id.clone();
    vault
        .calendar_account_repo()
        .upsert(account.clone())
        .await?;
    assert_eq!(
        vault
            .calendar_account_repo()
            .find(account_id.clone())
            .await?,
        Some(account.clone())
    );

    let cursor_repo = vault.calendar_sync_cursor_repo();
    for (calendar_id, token) in [("primary", "primary-token"), ("team", "team-token")] {
        cursor_repo
            .upsert(CalendarSyncCursor {
                account_id: account_id.clone(),
                calendar_id: calendar_id.into(),
                sync_token: Some(token.into()),
                last_full_sync_at: Some(now),
                last_incremental_sync_at: None,
                updated_at: now,
            })
            .await?;
    }
    cursor_repo
        .clear(account_id.clone(), "primary".into())
        .await?;
    assert!(
        cursor_repo
            .get(account_id.clone(), "primary".into())
            .await?
            .is_none()
    );
    assert_eq!(
        cursor_repo
            .get(account_id.clone(), "team".into())
            .await?
            .and_then(|cursor| cursor.sync_token),
        Some("team-token".into())
    );

    let original_id = ExternalEventId::new();
    let event_repo = vault.external_event_repo();
    event_repo
        .upsert(ExternalEvent {
            id: original_id.clone(),
            account_id: account_id.clone(),
            calendar_id: "primary".into(),
            provider_event_id: "remote-1".into(),
            recurring_event_id: None,
            original_start_at: None,
            title: "Original".into(),
            description: None,
            location: Some("Office".into()),
            start_at: datetime!(2026-08-15 01:00 UTC),
            end_at: datetime!(2026-08-15 02:00 UTC),
            all_day: false,
            timezone: Some("Asia/Tokyo".into()),
            status: ExternalEventStatus::Confirmed,
            transparency: ExternalEventTransparency::Opaque,
            needs_travel: Some(true),
            travel_before_minutes: Some(20),
            travel_after_minutes: Some(10),
            etag: Some("etag-1".into()),
            provider_updated_at: Some(now),
            created_at: now,
            updated_at: now,
        })
        .await?;
    event_repo
        .upsert(ExternalEvent {
            id: ExternalEventId::new(),
            account_id: account_id.clone(),
            calendar_id: "primary".into(),
            provider_event_id: "remote-1".into(),
            recurring_event_id: None,
            original_start_at: None,
            title: "Synced title".into(),
            description: Some("new provider data".into()),
            location: Some("Office".into()),
            start_at: datetime!(2026-08-15 01:00 UTC),
            end_at: datetime!(2026-08-15 02:00 UTC),
            all_day: false,
            timezone: Some("Asia/Tokyo".into()),
            status: ExternalEventStatus::Confirmed,
            transparency: ExternalEventTransparency::Opaque,
            needs_travel: None,
            travel_before_minutes: None,
            travel_after_minutes: None,
            etag: Some("etag-2".into()),
            provider_updated_at: Some(now),
            created_at: now,
            updated_at: now,
        })
        .await?;

    let stored = event_repo
        .find_by_provider_event(account_id.clone(), "primary".into(), "remote-1".into())
        .await?
        .expect("event should exist");
    assert_eq!(stored.id, original_id);
    assert_eq!(stored.title, "Synced title");
    assert_eq!(stored.needs_travel, Some(true));
    assert_eq!(stored.travel_before_minutes, Some(20));
    assert_eq!(stored.travel_after_minutes, Some(10));
    assert_eq!(
        event_repo
            .list_overlapping(
                datetime!(2026-08-15 01:30 UTC),
                datetime!(2026-08-15 01:45 UTC),
            )
            .await?
            .len(),
        1
    );
    assert!(
        event_repo
            .list_overlapping(
                datetime!(2026-08-15 02:00 UTC),
                datetime!(2026-08-15 03:00 UTC),
            )
            .await?
            .is_empty()
    );

    let mut team_event = stored.clone();
    team_event.id = ExternalEventId::new();
    team_event.calendar_id = "team".into();
    team_event.provider_event_id = "remote-team".into();
    team_event.title = "Team event".into();
    event_repo.upsert(team_event).await?;
    assert_eq!(
        event_repo
            .list_overlapping(
                datetime!(2026-08-15 01:30 UTC),
                datetime!(2026-08-15 01:45 UTC),
            )
            .await?
            .len(),
        2
    );

    let account_repo = vault.calendar_account_repo();
    let mut selected_account = account.clone();
    selected_account.selected_calendar_ids = vec!["primary".into()];
    account_repo.upsert(selected_account.clone()).await?;
    let visible = event_repo
        .list_overlapping(
            datetime!(2026-08-15 01:30 UTC),
            datetime!(2026-08-15 01:45 UTC),
        )
        .await?;
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].provider_event_id, "remote-1");

    selected_account.selected_calendar_ids = vec!["team".into()];
    account_repo.upsert(selected_account.clone()).await?;
    let visible = event_repo
        .list_overlapping(
            datetime!(2026-08-15 01:30 UTC),
            datetime!(2026-08-15 01:45 UTC),
        )
        .await?;
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].provider_event_id, "remote-team");

    selected_account.enabled = false;
    account_repo.upsert(selected_account.clone()).await?;
    assert!(
        event_repo
            .list_overlapping(
                datetime!(2026-08-15 01:30 UTC),
                datetime!(2026-08-15 01:45 UTC),
            )
            .await?
            .is_empty()
    );

    // Calendar rows and cursors remain available while selection changes; a
    // later re-selection immediately makes the retained event busy again.
    selected_account.enabled = true;
    selected_account.selected_calendar_ids = vec!["primary".into()];
    account_repo.upsert(selected_account).await?;
    assert_eq!(
        event_repo
            .list_overlapping(
                datetime!(2026-08-15 01:30 UTC),
                datetime!(2026-08-15 01:45 UTC),
            )
            .await?
            .len(),
        1
    );
    assert!(
        event_repo
            .find_by_provider_event(account_id.clone(), "team".into(), "remote-team".into(),)
            .await?
            .is_some()
    );

    event_repo
        .upsert(ExternalEvent {
            id: ExternalEventId::new(),
            account_id: stored.account_id.clone(),
            calendar_id: "primary".into(),
            provider_event_id: "stale-event".into(),
            recurring_event_id: None,
            original_start_at: None,
            title: "Stale".into(),
            description: None,
            location: None,
            start_at: datetime!(2026-08-15 03:00 UTC),
            end_at: datetime!(2026-08-15 04:00 UTC),
            all_day: false,
            timezone: None,
            status: ExternalEventStatus::Confirmed,
            transparency: ExternalEventTransparency::Opaque,
            needs_travel: None,
            travel_before_minutes: None,
            travel_after_minutes: None,
            etag: None,
            provider_updated_at: None,
            created_at: now,
            updated_at: now,
        })
        .await?;
    assert_eq!(
        event_repo
            .delete_unseen_for_calendar(
                stored.account_id.clone(),
                "primary".into(),
                vec!["remote-1".into()],
            )
            .await?,
        1
    );
    assert_eq!(
        event_repo
            .find_by_provider_event(stored.account_id, "primary".into(), "remote-1".into(),)
            .await?
            .and_then(|event| event.travel_before_minutes),
        Some(20)
    );
    Ok(())
}

#[tokio::test]
async fn habit_preferences_and_reapply_keep_schedule_and_link_ids() -> anyhow::Result<()> {
    let (_dir, vault) = test_vault().await?;
    let now = datetime!(2026-08-15 00:00 UTC);
    let user_id = UserId::new();
    let account = account(user_id.clone(), now);
    let account_id = account.id.clone();
    vault.calendar_account_repo().upsert(account).await?;

    let habit = Habit {
        id: HabitId::new(),
        user_id: user_id.clone(),
        title: "Morning walk".into(),
        schedule: HabitSchedule::Weekdays {
            weekdays: vec![DayOfWeek::Monday, DayOfWeek::Wednesday],
        },
        duration_minutes: 30,
        preferred_window: Some(DailyTimeRange::new(time!(06:00), time!(09:00))?),
        flexibility: HabitFlexibility::Flexible,
        enabled: true,
        disabled_at: None,
        created_at: now,
        updated_at: now,
    };
    let habit_id = habit.id.clone();
    vault.habit_repo().upsert(habit.clone()).await?;
    assert_eq!(
        vault.habit_repo().find(habit_id.clone()).await?,
        Some(habit)
    );

    let occurrence = HabitOccurrence::pending(habit_id.clone(), date!(2026 - 08 - 17), now);
    let occurrence_id = occurrence.id.clone();
    vault
        .habit_occurrence_repo()
        .upsert(occurrence.clone())
        .await?;
    let mut repeated = occurrence;
    repeated.id = HabitOccurrenceId::new();
    repeated.skip(Some("holiday".into()), now);
    vault.habit_occurrence_repo().upsert(repeated).await?;
    vault
        .habit_occurrence_repo()
        .upsert(HabitOccurrence::pending(
            habit_id.clone(),
            date!(2026 - 08 - 17),
            now + time::Duration::minutes(1),
        ))
        .await?;
    let stored_occurrence = vault
        .habit_occurrence_repo()
        .find_for_date(habit_id.clone(), date!(2026 - 08 - 17))
        .await?
        .expect("occurrence should exist");
    assert_eq!(stored_occurrence.id, occurrence_id.clone());
    assert_eq!(stored_occurrence.state, HabitOccurrenceState::Skipped);
    assert_eq!(stored_occurrence.skip_reason.as_deref(), Some("holiday"));

    let mut snoozed = HabitOccurrence::pending(habit_id.clone(), date!(2026 - 08 - 18), now);
    let snoozed_id = snoozed.id.clone();
    let snoozed_until = now + time::Duration::hours(2);
    snoozed.snooze_until(snoozed_until, now)?;
    vault.habit_occurrence_repo().upsert(snoozed).await?;
    vault
        .habit_occurrence_repo()
        .upsert(HabitOccurrence::pending(
            habit_id.clone(),
            date!(2026 - 08 - 18),
            now + time::Duration::minutes(1),
        ))
        .await?;
    let stored_snoozed = vault
        .habit_occurrence_repo()
        .find_for_date(habit_id.clone(), date!(2026 - 08 - 18))
        .await?
        .expect("snoozed occurrence should exist");
    assert_eq!(stored_snoozed.id, snoozed_id);
    assert_eq!(stored_snoozed.state, HabitOccurrenceState::Snoozed);
    assert_eq!(stored_snoozed.snoozed_until, Some(snoozed_until));

    let mut scheduled = HabitOccurrence::pending(habit_id.clone(), date!(2026 - 08 - 19), now);
    let scheduled_id = scheduled.id.clone();
    let scheduled_start = datetime!(2026-08-19 01:00 UTC);
    let scheduled_end = datetime!(2026-08-19 01:30 UTC);
    scheduled.state = HabitOccurrenceState::Scheduled;
    scheduled.scheduled_start_at = Some(scheduled_start);
    scheduled.scheduled_end_at = Some(scheduled_end);
    vault.habit_occurrence_repo().upsert(scheduled).await?;
    vault
        .habit_occurrence_repo()
        .upsert(HabitOccurrence::pending(
            habit_id.clone(),
            date!(2026 - 08 - 19),
            now + time::Duration::minutes(1),
        ))
        .await?;
    let stored_scheduled = vault
        .habit_occurrence_repo()
        .find_for_date(habit_id, date!(2026 - 08 - 19))
        .await?
        .expect("scheduled occurrence should exist");
    assert_eq!(stored_scheduled.id, scheduled_id);
    assert_eq!(stored_scheduled.state, HabitOccurrenceState::Scheduled);
    assert_eq!(stored_scheduled.scheduled_start_at, Some(scheduled_start));
    assert_eq!(stored_scheduled.scheduled_end_at, Some(scheduled_end));

    let preferences = SchedulingPreferences {
        id: SchedulingPolicyId::new(),
        user_id: user_id.clone(),
        timezone: "Asia/Tokyo".into(),
        named_hours: vec![WeeklyTimePolicy {
            name: "work".into(),
            hard: false,
            days: vec![WeekdayTimeRanges {
                weekday: DayOfWeek::Monday,
                ranges: vec![DailyTimeRange::new(time!(09:00), time!(18:00))?],
            }],
        }],
        sleep: Some(WeeklyTimePolicy {
            name: "sleep".into(),
            hard: true,
            days: vec![WeekdayTimeRanges {
                weekday: DayOfWeek::Monday,
                ranges: vec![DailyTimeRange::new(time!(23:00), time!(07:00))?],
            }],
        }),
        default_travel_buffer_minutes: 15,
        created_at: now,
        updated_at: now,
    };
    vault
        .scheduling_preferences_repo()
        .upsert(preferences.clone())
        .await?;
    assert_eq!(
        vault
            .scheduling_preferences_repo()
            .get_for_user(user_id)
            .await?,
        Some(preferences)
    );

    let block_id = ScheduleBlockId::new();
    let mut block = ScheduleBlock {
        id: block_id.clone(),
        task_id: None,
        habit_occurrence_id: Some(occurrence_id.clone()),
        title_snapshot: Some("Morning walk".into()),
        start_at: datetime!(2026-08-17 09:00 +09:00),
        end_at: datetime!(2026-08-17 09:30 +09:00),
        block_type: ScheduleBlockType::Habit,
        state: ScheduleBlockState::Proposed,
        locked: false,
        source: ScheduleBlockSource::Scheduler,
        required_minutes: Some(30),
        created_at: now,
        updated_at: now,
    };
    let range_start = datetime!(2026-08-16 00:00 UTC);
    let range_end = datetime!(2026-08-18 00:00 UTC);
    vault
        .schedule_block_repo()
        .replace_proposed_in_range(range_start, range_end, vec![block.clone()])
        .await?;

    let link = ManagedCalendarEventLink {
        id: ManagedCalendarEventId::new(),
        account_id: account_id.clone(),
        schedule_block_id: block_id.clone(),
        calendar_id: "mnema-managed".into(),
        provider_event_id: "managed-event-1".into(),
        etag: Some("etag-1".into()),
        created_at: now,
        updated_at: now,
    };
    let link_id = link.id.clone();
    vault
        .managed_calendar_event_link_repo()
        .upsert(link)
        .await?;

    block.title_snapshot = Some("Morning walk (moved)".into());
    block.start_at = datetime!(2026-08-17 09:30 +09:00);
    block.end_at = datetime!(2026-08-17 10:00 +09:00);
    block.updated_at = datetime!(2026-08-15 00:01 UTC);
    vault
        .schedule_block_repo()
        .replace_proposed_in_range(range_start, range_end, vec![block.clone()])
        .await?;
    vault
        .schedule_block_repo()
        .replace_proposed_in_range(range_start, range_end, vec![block])
        .await?;

    let blocks = vault
        .schedule_block_repo()
        .list_overlapping(
            datetime!(2026-08-17 00:45 UTC),
            datetime!(2026-08-17 00:50 UTC),
        )
        .await?;
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].id, block_id.clone());
    assert_eq!(blocks[0].start_at.offset(), UtcOffset::UTC);
    assert_eq!(blocks[0].habit_occurrence_id.as_ref(), Some(&occurrence_id));
    assert_eq!(
        vault
            .managed_calendar_event_link_repo()
            .find_by_schedule_block(account_id.clone(), block_id)
            .await?
            .map(|value| value.id),
        Some(link_id)
    );
    assert_eq!(
        vault
            .managed_calendar_event_link_repo()
            .list_for_account(account_id)
            .await?
            .len(),
        1
    );
    Ok(())
}
