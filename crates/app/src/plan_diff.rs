use std::collections::{BTreeMap, BTreeSet};

use mnema_core::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::PlanTodayResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanChangeKind {
    Create,
    Move,
    Resize,
    MoveAndResize,
    Remove,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanBlockSnapshot {
    pub block_id: Option<ScheduleBlockId>,
    pub task_id: TaskId,
    pub title: String,
    #[serde(with = "time::serde::rfc3339")]
    pub start_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub end_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanChange {
    pub kind: PlanChangeKind,
    pub task_id: TaskId,
    pub title: String,
    pub before: Option<PlanBlockSnapshot>,
    pub after: Option<PlanBlockSnapshot>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulePlanDiff {
    pub fingerprint: String,
    pub changes: Vec<PlanChange>,
}

impl SchedulePlanDiff {
    #[must_use]
    pub fn has_changes(&self) -> bool {
        self.changes
            .iter()
            .any(|change| change.kind != PlanChangeKind::Unchanged)
    }

    #[must_use]
    pub fn changed_count(&self) -> usize {
        self.changes
            .iter()
            .filter(|change| change.kind != PlanChangeKind::Unchanged)
            .count()
    }
}

#[must_use]
pub fn diff_task_plan(existing: &[ScheduleBlock], proposed: &PlanTodayResult) -> SchedulePlanDiff {
    let existing = existing
        .iter()
        .filter(|block| {
            block.task_id.is_some()
                && block.state == ScheduleBlockState::Proposed
                && matches!(
                    block.source,
                    ScheduleBlockSource::Scheduler | ScheduleBlockSource::Repair
                )
        })
        .filter_map(|block| {
            let task_id = block.task_id.clone()?;
            Some((
                task_id.clone(),
                PlanBlockSnapshot {
                    block_id: Some(block.id.clone()),
                    task_id,
                    title: block
                        .title_snapshot
                        .clone()
                        .unwrap_or_else(|| "(untitled)".into()),
                    start_at: block.start_at,
                    end_at: block.end_at,
                },
            ))
        })
        .collect::<BTreeMap<_, _>>();

    let proposed = proposed
        .output
        .blocks
        .iter()
        .map(|block| {
            (
                block.task_id.clone(),
                PlanBlockSnapshot {
                    block_id: None,
                    task_id: block.task_id.clone(),
                    title: block.title.clone(),
                    start_at: block.window.start,
                    end_at: block.window.end,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let task_ids = existing
        .keys()
        .chain(proposed.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::with_capacity(task_ids.len());

    for task_id in task_ids {
        let before = existing.get(&task_id).cloned();
        let after = proposed.get(&task_id).cloned();
        let (kind, reason) = change_kind(before.as_ref(), after.as_ref());
        let title = after
            .as_ref()
            .or(before.as_ref())
            .map(|block| block.title.clone())
            .unwrap_or_default();
        changes.push(PlanChange {
            kind,
            task_id,
            title,
            before,
            after,
            reason,
        });
    }

    let fingerprint = fingerprint(&changes);
    SchedulePlanDiff {
        fingerprint,
        changes,
    }
}

fn change_kind(
    before: Option<&PlanBlockSnapshot>,
    after: Option<&PlanBlockSnapshot>,
) -> (PlanChangeKind, String) {
    match (before, after) {
        (None, Some(_)) => (PlanChangeKind::Create, "新しい空き時間へ配置します".into()),
        (Some(_), None) => (
            PlanChangeKind::Remove,
            "今回の計画対象から外れたため提案を取り除きます".into(),
        ),
        (Some(before), Some(after)) => {
            let moved = before.start_at != after.start_at;
            let resized = (before.end_at - before.start_at) != (after.end_at - after.start_at);
            match (moved, resized) {
                (false, false) => (PlanChangeKind::Unchanged, "変更はありません".into()),
                (true, false) => (PlanChangeKind::Move, "空き時間に合わせて移動します".into()),
                (false, true) => (
                    PlanChangeKind::Resize,
                    "最新の見積時間に合わせて長さを変更します".into(),
                ),
                (true, true) => (
                    PlanChangeKind::MoveAndResize,
                    "空き時間と最新の見積に合わせて移動・変更します".into(),
                ),
            }
        }
        (None, None) => unreachable!("a task id is collected from at least one side"),
    }
}

fn fingerprint(changes: &[PlanChange]) -> String {
    let bytes = serde_json::to_vec(changes).expect("plan changes are serializable");
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlanTodayResult, ProposedScheduleBlock, SchedulingOutput, TimeWindow};
    use time::macros::{date, datetime};

    fn saved(task_id: TaskId, start_hour: u8, minutes: i64) -> ScheduleBlock {
        let start = datetime!(2026-08-15 00:00 UTC) + time::Duration::hours(start_hour.into());
        ScheduleBlock {
            id: ScheduleBlockId::new(),
            task_id: Some(task_id),
            habit_occurrence_id: None,
            title_snapshot: Some("Focus".into()),
            start_at: start,
            end_at: start + time::Duration::minutes(minutes),
            block_type: ScheduleBlockType::Task,
            state: ScheduleBlockState::Proposed,
            locked: false,
            source: ScheduleBlockSource::Scheduler,
            required_minutes: Some(minutes as u32),
            created_at: datetime!(2026-08-15 00:00 UTC),
            updated_at: datetime!(2026-08-15 00:00 UTC),
        }
    }

    fn plan(task_id: TaskId, start_hour: u8, minutes: i64) -> PlanTodayResult {
        let start = datetime!(2026-08-15 00:00 UTC) + time::Duration::hours(start_hour.into());
        PlanTodayResult {
            target_date: date!(2026 - 08 - 15),
            output: SchedulingOutput {
                blocks: vec![ProposedScheduleBlock {
                    task_id,
                    title: "Focus".into(),
                    window: TimeWindow::new(start, start + time::Duration::minutes(minutes)),
                    required_minutes: minutes as u32,
                }],
                unscheduled: vec![],
                issues: vec![],
            },
        }
    }

    #[test]
    fn detects_move_without_mutating_saved_block() {
        let task_id = TaskId::new();
        let existing = saved(task_id.clone(), 9, 30);
        let diff = diff_task_plan(std::slice::from_ref(&existing), &plan(task_id, 10, 30));

        assert_eq!(diff.changes[0].kind, PlanChangeKind::Move);
        assert_eq!(existing.start_at, datetime!(2026-08-15 09:00 UTC));
        assert_eq!(diff.fingerprint.len(), 64);
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let task_id = TaskId::new();
        let existing = saved(task_id.clone(), 9, 30);
        let plan = plan(task_id, 10, 45);
        assert_eq!(
            diff_task_plan(std::slice::from_ref(&existing), &plan).fingerprint,
            diff_task_plan(&[existing], &plan).fingerprint
        );
    }
}
