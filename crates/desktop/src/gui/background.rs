use super::*;
use mnema_infra::calendar::{
    CalendarReadApi, CalendarWriteApi, GoogleCalendarAdapter, KeyringCredentialStore,
    RemoteCalendar, desktop_oauth, service,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{future::Future, sync::mpsc, time::Duration};

#[derive(Default)]
pub(super) struct BackgroundState {
    receiver: Option<mpsc::Receiver<Result<Output>>>,
    handle: Option<tokio::task::JoinHandle<()>>,
    pub status: String,
    pub applying: bool,
    pub error: Option<String>,
    pub suggestion: Option<AutoSchedulePreview>,
    pub calendars: HashMap<CalendarAccountId, Vec<RemoteCalendar>>,
    context: Option<egui::Context>,
    periodic_handle: Option<tokio::task::JoinHandle<()>>,
    periodic_result: Arc<Mutex<Option<Result<Output>>>>,
    periodic_enabled: Arc<AtomicBool>,
    periodic_running: Arc<AtomicBool>,
    gate: Arc<tokio::sync::Mutex<()>>,
    wake: Arc<tokio::sync::Notify>,
}

impl Drop for BackgroundState {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.periodic_handle.take() {
            handle.abort();
        }
    }
}

impl BackgroundState {
    pub fn wake_for_sync(&self) {
        self.wake.notify_one();
    }

    pub fn busy(&self) -> bool {
        self.receiver.is_some() || self.periodic_running.load(Ordering::Acquire)
    }
}

pub(super) enum Output {
    Suggested(
        AutoSchedulePreview,
        HashMap<CalendarAccountId, Vec<RemoteCalendar>>,
    ),
    Preview(AutoSchedulePreview),
    Applied(usize),
    Calendar(String),
}

fn google() -> Result<GoogleCalendarAdapter> {
    Ok(GoogleCalendarAdapter::new(
        desktop_oauth::desktop_google_config()?,
        Arc::new(KeyringCredentialStore::mnema()),
    ))
}

pub(super) async fn preview(
    vault: &Vault,
    request: AutoScheduleRequest,
) -> Result<AutoSchedulePreview> {
    let tasks = vault.task_repo();
    let statuses = vault.status_repo();
    let blocks = vault.schedule_block_repo();
    let events = vault.external_event_repo();
    let habits = vault.habit_repo();
    let occurrences = vault.habit_occurrence_repo();
    let preferences = vault.scheduling_preferences_repo();
    Ok(AutoScheduleService::new(
        tasks.as_ref(),
        statuses.as_ref(),
        blocks.as_ref(),
        events.as_ref(),
        habits.as_ref(),
        occurrences.as_ref(),
        preferences.as_ref(),
    )
    .preview(request)
    .await?)
}

pub(super) async fn apply(vault: &Vault, preview: &AutoSchedulePreview) -> Result<usize> {
    let tasks = vault.task_repo();
    let statuses = vault.status_repo();
    let blocks = vault.schedule_block_repo();
    let events = vault.external_event_repo();
    let habits = vault.habit_repo();
    let occurrences = vault.habit_occurrence_repo();
    let preferences = vault.scheduling_preferences_repo();
    let result = AutoScheduleService::new(
        tasks.as_ref(),
        statuses.as_ref(),
        blocks.as_ref(),
        events.as_ref(),
        habits.as_ref(),
        occurrences.as_ref(),
        preferences.as_ref(),
    )
    .apply(preview, &preview.diff.fingerprint)
    .await?;
    Ok(result.blocks.len())
}

async fn synchronize_and_suggest(vault: Vault) -> Result<Output> {
    let preferences = vault
        .scheduling_preferences_repo()
        .get_for_user(local_user_id())
        .await?
        .ok_or_else(|| anyhow!("設定で作業時間を保存してください。"))?;
    let timezone = timezones::get_by_name(&preferences.timezone)
        .ok_or_else(|| anyhow!("タイムゾーンが不正です。"))?;
    let now = OffsetDateTime::now_utc();
    let start = now.to_timezone(timezone).date();
    let end = start
        .checked_add(time::Duration::days(7))
        .ok_or_else(|| anyhow!("日付の範囲外です。"))?;
    let window = mnema_app::iana_date_range(start, end, &preferences.timezone)?;
    let accounts = vault
        .calendar_account_repo()
        .list_enabled(local_user_id())
        .await?;
    let mut calendars = HashMap::new();
    if !accounts.is_empty() {
        let adapter = google()?;
        for account in accounts {
            calendars.insert(account.id.clone(), adapter.list_calendars(&account).await?);
            if !service::selected_sync_calendar_ids(&account).is_empty() {
                // A failed sync stops the proposal. Stale or partial busy data
                // must never be used for automatic planning.
                service::sync_selected_calendars(
                    &vault,
                    &adapter,
                    &account,
                    window.start,
                    window.end,
                    false,
                )
                .await?;
            }
        }
    }
    let request = AutoScheduleRequest {
        user_id: local_user_id(),
        start_date: start,
        end_date_exclusive: end,
        timezone: preferences.timezone,
        named_hours: vec![],
        not_before: Some(OffsetDateTime::now_utc()),
    };
    Ok(Output::Suggested(
        preview(&vault, request).await?,
        calendars,
    ))
}

impl MnemaGuiApp {
    pub(super) fn start_background_job(
        &mut self,
        future: impl Future<Output = Result<Output>> + Send + 'static,
    ) -> bool {
        if self.background.busy() {
            return false;
        }
        let Ok(guard) = self.background.gate.clone().try_lock_owned() else {
            return false;
        };
        self.background
            .periodic_result
            .lock()
            .expect("background result lock")
            .take();
        let (sender, receiver) = mpsc::channel();
        let context = self.background.context.clone();
        self.background.error = None;
        self.background.status = "処理中…".into();
        self.background.receiver = Some(receiver);
        self.background.handle = Some(self.runtime.spawn(async move {
            let _guard = guard;
            let result = future.await;
            let _ = sender.send(result);
            if let Some(context) = context {
                context.request_repaint();
            }
        }));
        true
    }

    pub(super) fn poll_background(&mut self, ctx: &egui::Context) {
        self.background.context = Some(ctx.clone());
        self.background
            .periodic_enabled
            .store(self.background_enabled, Ordering::Release);
        if self.background.periodic_handle.is_none()
            && let Some(vault) = self.vault.clone()
        {
            let pending = self.background.periodic_result.clone();
            let enabled = self.background.periodic_enabled.clone();
            let running = self.background.periodic_running.clone();
            let gate = self.background.gate.clone();
            let wake = self.background.wake.clone();
            let ctx = ctx.clone();
            self.background.periodic_handle = Some(self.runtime.spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(180));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! { _ = interval.tick() => {}, _ = wake.notified() => {} }
                    if !enabled.load(Ordering::Acquire) {
                        continue;
                    }
                    let Ok(_guard) = gate.clone().try_lock_owned() else {
                        continue;
                    };
                    running.store(true, Ordering::Release);
                    ctx.request_repaint();
                    let result = synchronize_and_suggest(vault.clone()).await;
                    if let Err(error) = &result {
                        tracing::warn!(%error, "desktop calendar sync / proposal failed");
                    } else {
                        tracing::info!(
                            "desktop calendar sync / proposal completed without applying changes"
                        );
                    }
                    running.store(false, Ordering::Release);
                    *pending.lock().expect("background result lock") = Some(result);
                    ctx.request_repaint();
                }
            }));
        }
        ctx.request_repaint_after(Duration::from_secs(1));
        let manual_result = self
            .background
            .receiver
            .as_ref()
            .map(|receiver| receiver.try_recv());
        let manual_finished = manual_result
            .as_ref()
            .is_some_and(|result| !matches!(result, Err(mpsc::TryRecvError::Empty)));
        let result = if manual_finished {
            manual_result
        } else {
            self.background
                .periodic_result
                .lock()
                .expect("background result lock")
                .take()
                .map(Ok)
        };
        if let Some(result) = result {
            let result = match result {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(anyhow!("バックグラウンド処理が中断されました。")))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                if result.is_ok() {
                    self.background.error = None;
                }
                if manual_finished {
                    self.background.applying = false;
                    self.background.receiver = None;
                    self.background.handle = None;
                }
                match result {
                    Ok(Output::Suggested(preview, calendars)) => {
                        self.background.status = format!(
                            "{}計画の変更候補 {}件",
                            if calendars.is_empty() {
                                ""
                            } else {
                                "同期済み · "
                            },
                            preview.diff.changed_count()
                        );
                        self.background.calendars = calendars;
                        self.background.suggestion = (self.background_enabled
                            && preview.diff.has_changes())
                        .then_some(preview);
                        self.refresh_calendar_accounts();
                        self.refresh_schedule();
                        self.refresh_schedule_month();
                    }
                    Ok(Output::Preview(preview)) => {
                        self.message = format!(
                            "計画案: {}件配置 / {}件未配置",
                            preview.output.blocks.len(),
                            preview.output.unscheduled.len()
                        );
                        self.plan = None;
                        self.auto_preview = Some(preview);
                        self.background.status = "計画案を作成しました".into();
                    }
                    Ok(Output::Applied(count)) => {
                        self.auto_preview = None;
                        self.background.suggestion = None;
                        self.message = format!("{count}件の予定を保存しました");
                        self.background.status = "計画を適用しました".into();
                        self.refresh_schedule();
                        self.refresh_schedule_month();
                        self.refresh_habits();
                    }
                    Ok(Output::Calendar(message)) => {
                        self.background.status = message;
                        self.background.wake.notify_one();
                        self.refresh_calendar_accounts();
                    }
                    Err(error) => {
                        self.background.status = "処理を完了できませんでした".into();
                        self.background.error = Some(error.to_string());
                        self.background.suggestion = None;
                    }
                }
            }
        }
    }

    pub(super) fn show_background_controls(&mut self, ui: &mut egui::Ui, palette: Palette) {
        ui.push_id("background_controls", |ui| {
            if ui
                .checkbox(
                    &mut self.background_enabled,
                    "バックグラウンドで同期・計画を提案（3分ごと）",
                )
                .changed()
            {
                self.background
                    .periodic_enabled
                    .store(self.background_enabled, Ordering::Release);
                if self.background_enabled {
                    self.background.wake.notify_one();
                }
                if !self.background_enabled {
                    self.background.suggestion = None;
                }
                self.persist_ui_preferences();
            }
            ui.label(
                regular_text("予定の変更は、提案を確認して適用したときだけ行います。")
                    .color(palette.muted),
            );
            self.show_background_status(ui, palette);
        });
    }

    pub(super) fn show_background_status(&mut self, ui: &mut egui::Ui, palette: Palette) {
        ui.horizontal_wrapped(|ui| {
            if self.background.busy() {
                ui.spinner();
            }
            ui.label(
                regular_text(&self.background.status)
                    .size(12.0)
                    .color(palette.muted),
            );
            if self.background.suggestion.is_some()
                && !self.background.busy()
                && components::button(ui, "計画の提案を確認", false, palette).clicked()
                && let Some(preview) = self.background.suggestion.take()
            {
                self.target_date = preview.request.start_date.to_string();
                self.planning_days = 7;
                self.plan = None;
                self.auto_preview = Some(preview);
                self.view = View::Schedule;
            }
        });
        if let Some(error) = &self.background.error {
            ui.label(regular_text(error).size(12.0).color(palette.warning));
        }
    }

    pub(super) fn connect_google(&mut self, ctx: &egui::Context, mode: CalendarAccessMode) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let ctx = ctx.clone();
        self.start_background_job(async move {
            let account = desktop_oauth::connect(
                &vault,
                local_user_id(),
                mode,
                Arc::new(KeyringCredentialStore::mnema()),
                move |url| {
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                    ctx.request_repaint();
                },
            )
            .await?;
            Ok(Output::Calendar(format!(
                "{} を接続しました",
                account.display_name
            )))
        });
    }

    pub(super) fn create_managed_calendar(&mut self, mut account: CalendarAccount) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        self.start_background_job(async move {
            let calendar = google()?
                .ensure_managed_calendar(&account, "Mnema Schedule")
                .await?;
            account
                .selected_calendar_ids
                .retain(|id| id != &calendar.id);
            account.managed_calendar_id = Some(calendar.id);
            account.updated_at = OffsetDateTime::now_utc();
            vault.calendar_account_repo().upsert(account).await?;
            Ok(Output::Calendar("Mnema専用カレンダーを準備しました".into()))
        });
    }

    pub(super) fn writeback_calendar(&mut self, account: CalendarAccount) {
        let Ok(vault) = self.vault_clone() else {
            return;
        };
        let start = self.now_in_timezone().date();
        let timezone = self.timezone_offset.clone();
        self.start_background_job(async move {
            let window =
                mnema_app::iana_date_range(start, start + time::Duration::days(7), &timezone)?;
            let report = service::writeback_schedule(
                &vault,
                &google()?,
                &account,
                window.start,
                window.end,
                &timezone,
            )
            .await?;
            Ok(Output::Calendar(format!(
                "Googleに反映: 作成{}件・更新{}件・削除{}件",
                report.created, report.updated, report.deleted
            )))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime, time};

    #[test]
    fn split_dependency_start_date_and_reapply_work_with_sqlite() {
        let (_directory, mut app, _) = super::super::interaction_tests::fixture();
        app.quick_capture = "First /minutes 90".into();
        app.submit_composer(None);
        let first_id = app.tasks[0].id.clone();
        app.quick_capture = "Next /minutes 30".into();
        app.submit_composer(None);
        let next_id = app
            .tasks
            .iter()
            .find(|task| task.title == "Next")
            .unwrap()
            .id
            .clone();
        let vault = app.vault.clone().unwrap();
        app.runtime.block_on(async {
            let tasks = vault.task_repo();
            let mut first = tasks.find(first_id.clone()).await.unwrap().unwrap();
            first.start_date = Some(date!(2026 - 09 - 22));
            tasks.update(first).await.unwrap();
            let mut next = tasks.find(next_id.clone()).await.unwrap().unwrap();
            next.dependencies = vec![first_id.clone()];
            next.due_date = Some(date!(2026 - 09 - 21));
            tasks.update(next).await.unwrap();
            let preferences_repo = vault.scheduling_preferences_repo();
            let mut preferences = preferences_repo
                .get_for_user(local_user_id())
                .await
                .unwrap()
                .unwrap();
            for day in &mut preferences.named_hours[0].days {
                day.ranges = vec![
                    DailyTimeRange {
                        start: time!(09:00),
                        end: time!(10:00),
                    },
                    DailyTimeRange {
                        start: time!(11:00),
                        end: time!(12:00),
                    },
                ];
            }
            preferences_repo.upsert(preferences).await.unwrap();
            let request = AutoScheduleRequest {
                user_id: local_user_id(),
                start_date: date!(2026 - 09 - 21),
                end_date_exclusive: date!(2026 - 09 - 23),
                timezone: "Asia/Tokyo".into(),
                named_hours: vec![],
                not_before: None,
            };
            let planned = preview(&vault, request.clone()).await.unwrap();
            assert_eq!(planned.output.blocks.len(), 3);
            assert!(planned.output.unscheduled.is_empty());
            assert!(
                planned
                    .output
                    .blocks
                    .iter()
                    .all(|block| block.window.start >= datetime!(2026-09-22 09:00 +09:00))
            );
            let child = planned
                .output
                .blocks
                .iter()
                .find(|block| block.title == "Next")
                .unwrap();
            assert_eq!(child.window.start, datetime!(2026-09-22 11:30 +09:00));
            assert_eq!(apply(&vault, &planned).await.unwrap(), 3);
            let blocks_repo = vault.schedule_block_repo();
            let window = planned.planning_window;
            let before = blocks_repo
                .list_overlapping(window.start, window.end)
                .await
                .unwrap();
            let ids = before
                .iter()
                .map(|block| block.id.clone())
                .collect::<HashSet<_>>();
            assert_eq!(ids.len(), 3);
            let replanned = preview(&vault, request.clone()).await.unwrap();
            assert!(!replanned.diff.has_changes());
            apply(&vault, &replanned).await.unwrap();
            let after = blocks_repo
                .list_overlapping(window.start, window.end)
                .await
                .unwrap();
            assert_eq!(ids, after.iter().map(|block| block.id.clone()).collect());

            // A started chunk remains unchanged, while the remainder still
            // precedes its dependent task. It is not recreated or deleted.
            let started = after
                .iter()
                .min_by_key(|block| block.start_at)
                .unwrap()
                .clone();
            let mut cutoff = request.clone();
            cutoff.not_before = Some(datetime!(2026-09-22 09:30 +09:00));
            let replanned = preview(&vault, cutoff).await.unwrap();
            apply(&vault, &replanned).await.unwrap();
            assert_eq!(
                blocks_repo.find(started.id.clone()).await.unwrap().unwrap(),
                started
            );
            assert_eq!(
                blocks_repo
                    .list_overlapping(window.start, window.end)
                    .await
                    .unwrap()
                    .len(),
                3
            );

            // Locking one chunk must not drop the rest of that task.
            let mut locked = started;
            locked.locked = true;
            blocks_repo.update(locked).await.unwrap();
            let replanned = preview(&vault, request).await.unwrap();
            assert_eq!(replanned.output.blocks.len(), 2);
            assert_eq!(
                replanned
                    .output
                    .blocks
                    .iter()
                    .filter(|block| block.title == "First")
                    .map(|block| block.required_minutes)
                    .sum::<u32>(),
                30
            );
            apply(&vault, &replanned).await.unwrap();
            assert_eq!(
                blocks_repo
                    .list_overlapping(window.start, window.end)
                    .await
                    .unwrap()
                    .len(),
                3
            );
        });
    }

    #[test]
    fn background_suggestion_never_applies_and_only_one_job_runs() {
        let (_directory, mut app, ctx) = super::super::interaction_tests::fixture();
        app.quick_capture = "Background task /minutes 30".into();
        app.submit_composer(None);
        let vault = app.vault.clone().unwrap();
        let before = app
            .runtime
            .block_on(vault.schedule_block_repo().list_overlapping(
                datetime!(2026-01-01 00:00 UTC),
                datetime!(2027-01-01 00:00 UTC),
            ))
            .unwrap();
        let result = app
            .runtime
            .block_on(synchronize_and_suggest(vault.clone()))
            .unwrap();
        assert!(matches!(result, Output::Suggested(..)));
        let after = app
            .runtime
            .block_on(vault.schedule_block_repo().list_overlapping(
                datetime!(2026-01-01 00:00 UTC),
                datetime!(2027-01-01 00:00 UTC),
            ))
            .unwrap();
        assert_eq!(before, after);
        app.start_background_job(async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok(Output::Calendar("first".into()))
        });
        app.start_background_job(async { panic!("a second job must not start") });
        for _ in 0..100 {
            app.poll_background(&ctx);
            if !app.background.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.background.status, "first");
        assert!(!app.background.busy());
    }

    #[test]
    fn periodic_worker_runs_without_additional_ui_frames() {
        let (_directory, mut app, ctx) = super::super::interaction_tests::fixture();
        app.background_enabled = true;
        app.poll_background(&ctx);
        for _ in 0..200 {
            if app.background.periodic_result.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let result = app.background.periodic_result.lock().unwrap().take();
        assert!(matches!(result, Some(Ok(Output::Suggested(..)))));
        assert!(app.auto_preview.is_none());
        assert!(app.schedule.is_empty());
    }
}
