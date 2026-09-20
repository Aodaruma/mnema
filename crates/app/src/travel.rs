use mnema_core::prelude::*;
use mnema_scheduler::{BusyBlock, BusyBlockSource, TimeWindow};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TravelBufferPolicy {
    pub default_before_minutes: u32,
    pub default_after_minutes: u32,
}

impl TravelBufferPolicy {
    #[must_use]
    pub fn symmetric(minutes: u32) -> Self {
        Self {
            default_before_minutes: minutes,
            default_after_minutes: minutes,
        }
    }
}

#[must_use]
pub fn event_needs_travel(event: &ExternalEvent) -> bool {
    event.needs_travel.unwrap_or_else(|| {
        event
            .location
            .as_deref()
            .is_some_and(|location| !location.trim().is_empty())
    })
}

/// Produces fixed travel buffers around external events.
///
/// The result is clipped to `planning_window`; cancelled/transparent events
/// never create travel constraints. Per-event values override defaults.
#[must_use]
pub fn travel_busy_blocks(
    events: &[ExternalEvent],
    planning_window: TimeWindow,
    policy: TravelBufferPolicy,
) -> Vec<BusyBlock> {
    let mut blocks = events
        .iter()
        .filter(|event| event.blocks_time() && event_needs_travel(event))
        .flat_map(|event| {
            let before = event
                .travel_before_minutes
                .unwrap_or(policy.default_before_minutes);
            let after = event
                .travel_after_minutes
                .unwrap_or(policy.default_after_minutes);
            [
                event
                    .start_at
                    .checked_sub(Duration::minutes(i64::from(before)))
                    .and_then(|start| {
                        clipped_buffer(
                            start,
                            event.start_at,
                            planning_window,
                            format!("Travel to {}", event.title),
                        )
                    }),
                event
                    .end_at
                    .checked_add(Duration::minutes(i64::from(after)))
                    .and_then(|end| {
                        clipped_buffer(
                            event.end_at,
                            end,
                            planning_window,
                            format!("Travel from {}", event.title),
                        )
                    }),
            ]
            .into_iter()
            .flatten()
        })
        .collect::<Vec<_>>();

    blocks.sort_by_key(|block| (block.window.start, block.window.end));
    blocks
}

fn clipped_buffer(
    start: OffsetDateTime,
    end: OffsetDateTime,
    planning_window: TimeWindow,
    label: String,
) -> Option<BusyBlock> {
    let start = start.max(planning_window.start);
    let end = end.min(planning_window.end);
    (start < end).then(|| BusyBlock {
        window: TimeWindow::new(start, end),
        source: BusyBlockSource::ExternalCalendar,
        label: Some(label),
    })
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    fn event(location: Option<&str>) -> ExternalEvent {
        ExternalEvent {
            id: ExternalEventId::new(),
            account_id: CalendarAccountId::new(),
            calendar_id: "primary".into(),
            provider_event_id: "event-1".into(),
            recurring_event_id: None,
            original_start_at: None,
            title: "Client visit".into(),
            description: None,
            location: location.map(str::to_owned),
            start_at: datetime!(2026-08-15 10:00 UTC),
            end_at: datetime!(2026-08-15 11:00 UTC),
            all_day: false,
            timezone: Some("UTC".into()),
            status: ExternalEventStatus::Confirmed,
            transparency: ExternalEventTransparency::Opaque,
            needs_travel: None,
            travel_before_minutes: None,
            travel_after_minutes: Some(20),
            etag: None,
            provider_updated_at: None,
            created_at: datetime!(2026-08-15 00:00 UTC),
            updated_at: datetime!(2026-08-15 00:00 UTC),
        }
    }

    #[test]
    fn location_event_gets_clipped_before_and_after_buffers() {
        let window = TimeWindow::new(
            datetime!(2026-08-15 09:45 UTC),
            datetime!(2026-08-15 11:15 UTC),
        );
        let blocks = travel_busy_blocks(
            &[event(Some("Tokyo"))],
            window,
            TravelBufferPolicy::symmetric(30),
        );

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].window.start, datetime!(2026-08-15 09:45 UTC));
        assert_eq!(blocks[0].window.end, datetime!(2026-08-15 10:00 UTC));
        assert_eq!(blocks[1].window.start, datetime!(2026-08-15 11:00 UTC));
        assert_eq!(blocks[1].window.end, datetime!(2026-08-15 11:15 UTC));
    }

    #[test]
    fn explicit_false_disables_location_fallback() {
        let mut external = event(Some("Tokyo"));
        external.needs_travel = Some(false);
        let blocks = travel_busy_blocks(
            &[external],
            TimeWindow::new(
                datetime!(2026-08-15 09:00 UTC),
                datetime!(2026-08-15 12:00 UTC),
            ),
            TravelBufferPolicy::symmetric(30),
        );

        assert!(blocks.is_empty());
    }
}
