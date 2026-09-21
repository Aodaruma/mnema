use anyhow::{Result, anyhow};
use mnema_app::{AddHabitRequest, ItemScheduleIssue, UnscheduledItemReason};
use mnema_core::prelude::*;
use time::{Date, OffsetDateTime, Time, macros::format_description};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HabitScheduleChoice {
    Daily,
    Weekdays,
}

#[derive(Debug, Clone)]
pub struct HabitDraft {
    pub title: String,
    pub schedule: HabitScheduleChoice,
    pub weekdays: [bool; 7],
    pub duration_minutes: String,
    pub has_preferred_window: bool,
    pub preferred_start: String,
    pub preferred_end: String,
    pub flexibility: HabitFlexibility,
}

impl Default for HabitDraft {
    fn default() -> Self {
        Self {
            title: String::new(),
            schedule: HabitScheduleChoice::Daily,
            weekdays: [true, true, true, true, true, false, false],
            duration_minutes: "30".into(),
            has_preferred_window: true,
            preferred_start: "07:00".into(),
            preferred_end: "09:00".into(),
            flexibility: HabitFlexibility::Flexible,
        }
    }
}

impl HabitDraft {
    pub fn from_habit(habit: &Habit) -> Self {
        let mut draft = Self {
            title: habit.title.clone(),
            duration_minutes: habit.duration_minutes.to_string(),
            flexibility: habit.flexibility,
            has_preferred_window: habit.preferred_window.is_some(),
            ..Self::default()
        };
        if let HabitSchedule::Weekdays { weekdays } = &habit.schedule {
            draft.schedule = HabitScheduleChoice::Weekdays;
            draft.weekdays = DAYS.map(|day| weekdays.contains(&day));
        }
        if let Some(window) = &habit.preferred_window {
            draft.preferred_start =
                format!("{:02}:{:02}", window.start.hour(), window.start.minute());
            draft.preferred_end = format!("{:02}:{:02}", window.end.hour(), window.end.minute());
        }
        draft
    }

    pub fn request(&self, user_id: UserId) -> Result<AddHabitRequest> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(anyhow!("Habit名を入力してください"));
        }
        let duration_minutes = self
            .duration_minutes
            .trim()
            .parse::<u32>()
            .map_err(|_| anyhow!("所要時間は分単位の数値で入力してください"))?;
        if duration_minutes == 0 {
            return Err(anyhow!("所要時間は1分以上にしてください"));
        }
        let schedule = match self.schedule {
            HabitScheduleChoice::Daily => HabitSchedule::Daily,
            HabitScheduleChoice::Weekdays => {
                let weekdays = DAYS
                    .into_iter()
                    .zip(self.weekdays)
                    .filter_map(|(day, selected)| selected.then_some(day))
                    .collect::<Vec<_>>();
                if weekdays.is_empty() {
                    return Err(anyhow!("曜日を1つ以上選択してください"));
                }
                HabitSchedule::Weekdays { weekdays }
            }
        };
        let preferred_window = self
            .has_preferred_window
            .then(|| {
                DailyTimeRange::new(
                    parse_hm(&self.preferred_start)?,
                    parse_hm(&self.preferred_end)?,
                )
                .map_err(|error| anyhow!(error.to_string()))
            })
            .transpose()?;
        Ok(AddHabitRequest {
            user_id,
            title: title.to_owned(),
            schedule,
            duration_minutes,
            preferred_window,
            flexibility: self.flexibility,
        })
    }

    pub fn clear_after_add(&mut self) {
        self.title.clear();
    }
}

#[derive(Debug, Clone)]
pub struct SchedulingForm {
    pub timezone: String,
    pub work_start: String,
    pub work_end: String,
    pub sleep_start: String,
    pub sleep_end: String,
    pub travel_buffer_minutes: String,
}

impl SchedulingForm {
    pub fn from_legacy(timezone: String, work_start: String, work_end: String) -> Self {
        Self {
            timezone,
            work_start,
            work_end,
            sleep_start: "23:00".into(),
            sleep_end: "07:00".into(),
            travel_buffer_minutes: "15".into(),
        }
    }

    pub fn apply_preferences(&mut self, preferences: &SchedulingPreferences) {
        self.timezone.clone_from(&preferences.timezone);
        if let Some(work) = preferences
            .named_hours
            .iter()
            .find(|policy| policy.name.eq_ignore_ascii_case("work"))
            && let Some(range) = first_range(work)
        {
            self.work_start = format_hm(range.start);
            self.work_end = format_hm(range.end);
        }
        if let Some(sleep) = preferences.sleep.as_ref()
            && let Some(range) = first_range(sleep)
        {
            self.sleep_start = format_hm(range.start);
            self.sleep_end = format_hm(range.end);
        }
        self.travel_buffer_minutes = preferences.default_travel_buffer_minutes.to_string();
    }

    pub fn build_preferences(
        &self,
        existing: Option<&SchedulingPreferences>,
        user_id: UserId,
        now: OffsetDateTime,
    ) -> Result<SchedulingPreferences> {
        let tomorrow = now
            .date()
            .next_day()
            .ok_or_else(|| anyhow!("日付範囲を作成できません"))?;
        mnema_app::iana_date_range(now.date(), tomorrow, self.timezone.trim())
            .map_err(|error| anyhow!(error.to_string()))?;
        let work_range =
            DailyTimeRange::new(parse_hm(&self.work_start)?, parse_hm(&self.work_end)?)
                .map_err(|error| anyhow!(error.to_string()))?;
        let sleep_range =
            DailyTimeRange::new(parse_hm(&self.sleep_start)?, parse_hm(&self.sleep_end)?)
                .map_err(|error| anyhow!(error.to_string()))?;
        let travel = self
            .travel_buffer_minutes
            .trim()
            .parse::<u32>()
            .map_err(|_| anyhow!("移動バッファは分単位の数値で入力してください"))?;

        let mut preferences = existing.cloned().unwrap_or_else(|| {
            mnema_app::default_scheduling_preferences(user_id.clone(), self.timezone.trim(), now)
        });
        preferences.user_id = user_id;
        preferences.timezone = self.timezone.trim().to_owned();
        preferences.default_travel_buffer_minutes = travel;
        preferences.updated_at = now;
        let work = weekly_policy("work", false, &WORK_DAYS, work_range);
        if let Some(current) = preferences
            .named_hours
            .iter_mut()
            .find(|policy| policy.name.eq_ignore_ascii_case("work"))
        {
            *current = work;
        } else {
            preferences.named_hours.push(work);
        }
        preferences.sleep = Some(weekly_policy("sleep", true, &DAYS, sleep_range));
        preferences
            .validate()
            .map_err(|error| anyhow!(error.to_string()))?;
        Ok(preferences)
    }
}

pub fn item_issue_label(issue: &ItemScheduleIssue) -> String {
    match issue {
        ItemScheduleIssue::NoAvailability => "利用可能な時間帯がありません".into(),
        ItemScheduleIssue::ItemUnscheduled {
            title,
            required_minutes,
            reason,
            ..
        } => format!(
            "{title} ({required_minutes}分): {}",
            unscheduled_reason_label(*reason)
        ),
    }
}

pub fn date_range_end(start: Date, days: u32) -> Result<Date> {
    let mut end = start;
    for _ in 0..days.max(1) {
        end = end
            .next_day()
            .ok_or_else(|| anyhow!("計画日付が範囲外です"))?;
    }
    Ok(end)
}

pub fn normalize_selected_calendar_ids(
    input: &str,
    managed_calendar_id: Option<&str>,
) -> Vec<String> {
    let managed_calendar_id = managed_calendar_id
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let mut normalized = Vec::new();
    for calendar_id in input.split(',').map(str::trim) {
        if calendar_id.is_empty()
            || Some(calendar_id) == managed_calendar_id
            || normalized.iter().any(|existing| existing == calendar_id)
        {
            continue;
        }
        normalized.push(calendar_id.to_owned());
    }
    normalized
}

const DAYS: [DayOfWeek; 7] = [
    DayOfWeek::Monday,
    DayOfWeek::Tuesday,
    DayOfWeek::Wednesday,
    DayOfWeek::Thursday,
    DayOfWeek::Friday,
    DayOfWeek::Saturday,
    DayOfWeek::Sunday,
];

const WORK_DAYS: [DayOfWeek; 5] = [
    DayOfWeek::Monday,
    DayOfWeek::Tuesday,
    DayOfWeek::Wednesday,
    DayOfWeek::Thursday,
    DayOfWeek::Friday,
];

fn weekly_policy(
    name: &str,
    hard: bool,
    weekdays: &[DayOfWeek],
    range: DailyTimeRange,
) -> WeeklyTimePolicy {
    WeeklyTimePolicy {
        name: name.into(),
        hard,
        days: weekdays
            .iter()
            .copied()
            .map(|weekday| WeekdayTimeRanges {
                weekday,
                ranges: vec![range],
            })
            .collect(),
    }
}

fn first_range(policy: &WeeklyTimePolicy) -> Option<DailyTimeRange> {
    policy
        .days
        .iter()
        .find_map(|day| day.ranges.first().copied())
}

fn parse_hm(value: &str) -> Result<Time> {
    Time::parse(value.trim(), format_description!("[hour]:[minute]"))
        .map_err(|_| anyhow!("時刻は HH:MM 形式で入力してください: {value}"))
}

fn format_hm(value: Time) -> String {
    value
        .format(format_description!("[hour]:[minute]"))
        .unwrap_or_else(|_| value.to_string())
}

fn unscheduled_reason_label(reason: UnscheduledItemReason) -> &'static str {
    match reason {
        UnscheduledItemReason::InvalidDuration => "所要時間が不正です",
        UnscheduledItemReason::NoAvailability => "利用可能時間がありません",
        UnscheduledItemReason::NoAllowedWindow => "希望時間帯に空きがありません",
        UnscheduledItemReason::InsufficientContiguousTime => "必要な空き時間が不足しています",
        UnscheduledItemReason::DependencyBlocked => {
            "先行タスクを配置できません（依存先・循環を確認してください）"
        }
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    #[test]
    fn weekday_habit_requires_a_selection() {
        let draft = HabitDraft {
            title: "Walk".into(),
            schedule: HabitScheduleChoice::Weekdays,
            weekdays: [false; 7],
            ..Default::default()
        };
        assert!(draft.request(UserId::new()).is_err());
    }

    #[test]
    fn scheduling_form_preserves_identity() {
        let user_id = UserId::new();
        let original = mnema_app::default_scheduling_preferences(
            user_id.clone(),
            "Asia/Tokyo",
            datetime!(2026-08-15 00:00 UTC),
        );
        let id = original.id.clone();
        let form = SchedulingForm::from_legacy("Asia/Tokyo".into(), "10:00".into(), "18:00".into());
        let updated = form
            .build_preferences(Some(&original), user_id, datetime!(2026-08-16 00:00 UTC))
            .unwrap();
        assert_eq!(updated.id, id);
        assert_eq!(
            updated.named_hours[0].days[0].ranges[0].start,
            time::macros::time!(10:00)
        );
    }

    #[test]
    fn calendar_selection_is_trimmed_deduplicated_and_excludes_managed() {
        assert_eq!(
            normalize_selected_calendar_ids(
                " primary, team@example.com, primary, , mnema-managed ",
                Some("mnema-managed"),
            ),
            vec!["primary", "team@example.com"]
        );
    }
}
