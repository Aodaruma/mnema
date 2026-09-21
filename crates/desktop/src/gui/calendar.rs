use super::*;
use mnema_app::TimeWindow;

pub(super) struct CalendarUi {
    pub schedule_mode: ScheduleViewMode,
    pub future_blocks: Vec<ScheduleBlock>,
    pub external_events: Vec<ExternalEvent>,
    pub edit_start_date: String,
    pub edit_end_date: String,
    selected: Option<EventKey>,
    show_completed: bool,
    pub pan: [f32; 2],
}

impl Default for CalendarUi {
    fn default() -> Self {
        Self {
            schedule_mode: ScheduleViewMode::Week,
            edit_start_date: String::new(),
            edit_end_date: String::new(),
            future_blocks: Vec::new(),
            external_events: Vec::new(),
            selected: None,
            show_completed: true,
            pan: [0.0; 2],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum EventKey {
    Block(ScheduleBlockId),
    External(ExternalEventId),
    Preview(usize),
}

#[derive(Clone)]
struct CalendarEvent {
    key: EventKey,
    title: String,
    start: OffsetDateTime,
    end: OffsetDateTime,
    all_day: bool,
    completed: bool,
    fixed: bool,
    habit: bool,
    preview: bool,
}

impl MnemaGuiApp {
    fn calendar_month(
        &mut self,
        ui: &mut egui::Ui,
        date: Date,
        events: &[CalendarEvent],
        compact: bool,
        palette: Palette,
    ) {
        let Ok(first) = first_day_of_month(date) else {
            return;
        };
        let start = calendar_grid_start(first);
        let today = self.now_in_timezone().date();
        let height = ui.available_height().max(320.0);
        ScrollArea::vertical()
            .id_salt(("calendar_month", compact))
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (grid, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), height),
                    egui::Sense::hover(),
                );
                let p = ui.painter_at(grid);
                let width = grid.width() / 7.0;
                let row_height = (height - 24.0) / 6.0;
                for (i, label) in ["月", "火", "水", "木", "金", "土", "日"]
                    .iter()
                    .enumerate()
                {
                    p.text(
                        egui::pos2(grid.left() + (i as f32 + 0.5) * width, grid.top() + 9.0),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(11.0),
                        palette.muted,
                    );
                }
                for index in 0..42 {
                    let Some(day) = add_days(start, index) else {
                        continue;
                    };
                    let Ok(window) = mnema_app::iana_date_range(
                        day,
                        day.next_day().unwrap_or(day),
                        &self.timezone_offset,
                    ) else {
                        continue;
                    };
                    let rect = egui::Rect::from_min_size(
                        grid.min
                            + egui::vec2(
                                (index % 7) as f32 * width,
                                24.0 + (index / 7) as f32 * row_height,
                            ),
                        egui::vec2(width, row_height),
                    );
                    if day == date {
                        p.rect_filled(rect, 0.0, palette.faint);
                    }
                    p.hline(
                        rect.x_range(),
                        rect.top(),
                        Stroke::new(1.0_f32, palette.border),
                    );
                    p.vline(
                        rect.left(),
                        rect.y_range(),
                        Stroke::new(1.0_f32, palette.border),
                    );
                    let day_label = egui::Rect::from_center_size(
                        egui::pos2(rect.center().x, rect.top() + 15.0),
                        egui::vec2(24.0, 24.0),
                    );
                    if day == today {
                        p.rect_filled(day_label, 5.0, palette.error);
                    }
                    p.text(
                        day_label.center(),
                        egui::Align2::CENTER_CENTER,
                        day.day(),
                        egui::FontId::proportional(12.0),
                        if day == today {
                            Color32::WHITE
                        } else if day.month() == date.month() {
                            palette.text
                        } else {
                            palette.muted
                        },
                    );
                    if ui
                        .interact(rect, ui.id().with(("month_day", day)), egui::Sense::click())
                        .clicked()
                    {
                        self.select_home_date(day);
                        if compact {
                            self.home_calendar_mode = ScheduleViewMode::Day;
                        } else {
                            self.workspace_ui.calendar.schedule_mode = ScheduleViewMode::Day;
                        }
                    }
                    let matches: Vec<_> = events
                        .iter()
                        .filter(|e| {
                            e.start < window.end
                                && e.end > window.start
                                && (self.workspace_ui.calendar.show_completed || !e.completed)
                        })
                        .collect();
                    let max_rows = ((row_height - 32.0) / 23.0).max(0.0) as usize;
                    let shown = if matches.len() > max_rows {
                        max_rows.saturating_sub(1)
                    } else {
                        max_rows
                    };
                    for (row, event) in matches.iter().take(shown).enumerate() {
                        let chip = egui::Rect::from_min_size(
                            rect.min + egui::vec2(3.0, 32.0 + row as f32 * 23.0),
                            egui::vec2(width - 6.0, 20.0),
                        );
                        let color = if event.completed {
                            palette.muted
                        } else if event.habit {
                            palette.success
                        } else if matches!(event.key, EventKey::External(_)) {
                            palette.warning
                        } else {
                            palette.accent
                        };
                        p.rect_filled(chip, 3.0, color.gamma_multiply(0.12));
                        p.rect_filled(
                            egui::Rect::from_min_size(chip.min, egui::vec2(2.0, chip.height())),
                            1.0,
                            color,
                        );
                        let time = if event.all_day {
                            String::new()
                        } else {
                            format!("{} ", format_hm_in(event.start, self.app_timezone()))
                        };
                        p.with_clip_rect(p.clip_rect().intersect(chip)).text(
                            chip.left_center() + egui::vec2(5.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!(
                                "{}{time}{}",
                                if event.completed { "✓ " } else { "" },
                                event.title
                            ),
                            egui::FontId::proportional(10.0),
                            palette.text,
                        );
                        if ui
                            .interact(
                                chip,
                                ui.id().with(("month_event", index, &event.key)),
                                egui::Sense::click(),
                            )
                            .on_hover_text(&event.title)
                            .clicked()
                        {
                            self.workspace_ui.calendar.selected = Some(event.key.clone());
                        }
                    }
                    if matches.len() > shown {
                        let y = rect.top() + 32.0 + shown as f32 * 23.0;
                        p.text(
                            egui::pos2(rect.left() + 6.0, y),
                            egui::Align2::LEFT_TOP,
                            format!("＋{}件", matches.len() - shown),
                            egui::FontId::proportional(10.0),
                            palette.muted,
                        );
                    }
                }
            });
    }

    pub(super) fn calendar_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if !matches!(self.view, View::Home | View::Schedule)
            || egui::Popup::is_any_open(ctx)
            || self.workspace_ui.task_editor.is_some()
            || self.workspace_ui.calendar.selected.is_some()
            || self.confirming_delete_task_id.is_some()
            || ctx.input(|i| i.modifiers != egui::Modifiers::NONE)
        {
            return;
        }
        let compact = self.view == View::Home;
        let mut mode = if compact {
            self.home_calendar_mode
        } else {
            self.workspace_ui.calendar.schedule_mode
        };
        let previous = mode;
        for (key, value) in [
            (egui::Key::D, ScheduleViewMode::Day),
            (egui::Key::Num0, ScheduleViewMode::Week),
            (egui::Key::W, ScheduleViewMode::Week),
            (egui::Key::M, ScheduleViewMode::Calendar),
            (egui::Key::Num1, ScheduleViewMode::Day),
            (egui::Key::Num2, ScheduleViewMode::Days(2)),
            (egui::Key::Num3, ScheduleViewMode::Days(3)),
            (egui::Key::Num4, ScheduleViewMode::Days(4)),
            (egui::Key::Num5, ScheduleViewMode::Days(5)),
            (egui::Key::Num6, ScheduleViewMode::Days(6)),
            (egui::Key::Num7, ScheduleViewMode::Days(7)),
            (egui::Key::Num8, ScheduleViewMode::Days(8)),
            (egui::Key::Num9, ScheduleViewMode::Days(9)),
        ] {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                mode = value;
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::T)) {
            self.select_home_date(self.now_in_timezone().date());
        }
        if mode != previous {
            self.workspace_ui.calendar.pan = [0.0; 2];
            if mode == ScheduleViewMode::Week {
                let date =
                    parse_required_date(&self.target_date).unwrap_or(self.now_in_timezone().date());
                self.select_home_date(calendar_grid_start(date));
            }
            self.scroll_home_agenda_to_now = true;
        }
        if compact {
            self.home_calendar_mode = mode;
        } else {
            self.workspace_ui.calendar.schedule_mode = mode;
        }
    }

    pub(super) fn calendar_panel(&mut self, ui: &mut egui::Ui, compact: bool, palette: Palette) {
        let height = ui.available_height().max(340.0);
        home::card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height((height - 42.0).max(280.0));
            let today = self.now_in_timezone().date();
            let mut date = parse_required_date(&self.target_date).unwrap_or(today);
            let mut mode = if compact {
                self.home_calendar_mode
            } else {
                self.workspace_ui.calendar.schedule_mode
            };
            let original_mode = mode;
            // egui already smooths wheel input and maps Shift + vertical wheel to X.
            let area = ui.available_rect_before_wrap();
            if ui.rect_contains_pointer(area) && !egui::Popup::is_any_open(ui.ctx()) {
                let delta = ui.input_mut(|i| {
                    let x = i.smooth_scroll_delta.x;
                    i.smooth_scroll_delta.x = 0.0;
                    x
                });
                if mode != ScheduleViewMode::Calendar && delta != 0.0 {
                    let column = ((area.width() - 62.0) / mode_days(mode) as f32).max(1.0);
                    let pan = &mut self.workspace_ui.calendar.pan[usize::from(compact)];
                    *pan -= delta / column;
                    let whole = pan.floor() as i32;
                    if whole != 0 {
                        *pan -= whole as f32;
                        let previous = date;
                        date = add_days(date, whole).unwrap_or(date);
                        self.target_date = date.to_string();
                        self.refresh_schedule();
                        if date.month() != previous.month() || date.year() != previous.year() {
                            self.refresh_schedule_month();
                        }
                    }
                }
            }
            let width = ui.available_width();
            let narrow = width < 530.0;
            ui.allocate_ui_with_layout(
                egui::vec2(width, 28.0),
                egui::Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = if narrow { 3.0 } else { 6.0 };
                    ui.spacing_mut().interact_size.y = 28.0;
                    let title = format!("{}年{}月", date.year(), date.month() as u8);
                    let font = egui::FontId::proportional(if narrow { 15.0 } else { 18.0 });
                    let galley = ui.painter().layout_no_wrap(title, font, palette.text);
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(galley.size().x, 28.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().galley(
                        rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
                        galley,
                        palette.text,
                    );
                    response.on_hover_text(format!(
                        "{}週目 · {}",
                        date.iso_week(),
                        self.timezone_offset
                    ));
                    if width > 650.0 {
                        ui.label(
                            regular_text(format!("{}週目", date.iso_week()))
                                .size(11.0)
                                .color(palette.muted),
                        );
                    }
                    if !narrow {
                        if components::icon_button(ui, '\u{e5cb}', "前へ", palette).clicked() {
                            date = shift_date(date, mode, -1);
                        }
                        if components::icon_button(ui, '\u{e5cc}', "次へ", palette).clicked() {
                            date = shift_date(date, mode, 1);
                        }
                    }
                    if ui
                        .add_sized(
                            [40.0, 28.0],
                            egui::Button::new(regular_text("今日").size(12.0))
                                .fill(palette.control_hover)
                                .stroke(Stroke::new(1.0_f32, palette.border_strong))
                                .corner_radius(5.0),
                        )
                        .on_hover_text("今日へ移動 · T")
                        .clicked()
                    {
                        date = today;
                        self.workspace_ui.calendar.pan = [0.0; 2];
                        self.scroll_home_agenda_to_now = true;
                    }
                    let view = components::menu_button(ui, &mode_label(mode), false, palette);
                    components::popup(&view).show(|ui| {
                        ui.label(regular_text("カレンダーの表示期間").color(palette.muted));
                        for value in [
                            ScheduleViewMode::Day,
                            ScheduleViewMode::Days(2),
                            ScheduleViewMode::Days(3),
                            ScheduleViewMode::Days(4),
                            ScheduleViewMode::Week,
                            ScheduleViewMode::Calendar,
                        ] {
                            if ui
                                .selectable_value(&mut mode, value, mode_label(value))
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                        if narrow {
                            ui.separator();
                            if ui.button("前の期間へ").clicked() {
                                date = shift_date(date, mode, -1);
                                ui.close();
                            }
                            if ui.button("次の期間へ").clicked() {
                                date = shift_date(date, mode, 1);
                                ui.close();
                            }
                        }
                        ui.separator();
                        for hint in [
                            "T 今日 · 1–9 表示日数",
                            "D / 1 日 · W / 0 週 · M 月",
                            "Shift + スクロールで日付を移動",
                        ] {
                            ui.label(regular_text(hint).size(11.0).color(palette.muted));
                        }
                    });
                    let settings =
                        components::icon_button(ui, ICON_SETTINGS, "カレンダーの表示設定", palette);
                    components::popup(&settings).show(|ui| {
                        ui.label(bold_text("カレンダーの表示"));
                        ui.label(format!(
                            "{}週目 · {}",
                            date.iso_week(),
                            self.timezone_offset
                        ));
                        ui.checkbox(
                            &mut self.workspace_ui.calendar.show_completed,
                            "完了した予定を表示",
                        );
                        ui.separator();
                        if ui.button("作業時間・タイムゾーン").clicked() {
                            self.view = View::Settings;
                            self.workspace_ui.settings_section = pages::SettingsSection::Planning;
                            ui.close();
                        }
                        if ui.button("カレンダー連携").clicked() {
                            self.view = View::Settings;
                            self.workspace_ui.settings_section =
                                pages::SettingsSection::Connections;
                            ui.close();
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if components::icon_button(
                            ui,
                            '\u{e8b3}',
                            "現在時刻から再計画を提案",
                            palette,
                        )
                        .clicked()
                        {
                            self.preview_repair_at_now();
                        }
                        self.calendar_planning_menu(ui, narrow, palette);
                    });
                },
            );
            if mode != original_mode {
                self.workspace_ui.calendar.pan = [0.0; 2];
                if mode == ScheduleViewMode::Week {
                    date = calendar_grid_start(date);
                }
                self.scroll_home_agenda_to_now = true;
            }
            if date.to_string() != self.target_date {
                self.select_home_date(date);
            }
            if compact {
                self.home_calendar_mode = mode;
            } else {
                self.workspace_ui.calendar.schedule_mode = mode;
            }
            let end = if mode == ScheduleViewMode::Calendar {
                shift_date(
                    first_day_of_month(date).unwrap_or(date),
                    ScheduleViewMode::Calendar,
                    1,
                )
            } else {
                add_days(date, mode_days(mode)).unwrap_or(date)
            };
            let events = self.calendar_events();
            let summary_start = if mode == ScheduleViewMode::Calendar {
                first_day_of_month(date).unwrap_or(date)
            } else {
                date
            };
            self.calendar_summary(ui, summary_start, end, &events, palette);
            ui.add_space(8.0);
            if mode == ScheduleViewMode::Calendar {
                self.calendar_month(ui, date, &events, compact, palette);
            } else {
                self.calendar_grid(
                    ui,
                    date,
                    mode_days(mode) as usize,
                    &events,
                    compact,
                    palette,
                );
            }
        });
    }

    fn calendar_planning_menu(&mut self, ui: &mut egui::Ui, narrow: bool, palette: Palette) {
        let pending = self.auto_preview.is_some() || self.plan.is_some();
        let response = if narrow {
            components::icon_button(
                ui,
                ICON_ASSISTANT,
                if pending {
                    "未保存の計画を確認"
                } else {
                    "計画を提案"
                },
                palette,
            )
        } else {
            components::menu_button(
                ui,
                if pending {
                    "未保存の提案"
                } else {
                    "計画を提案"
                },
                true,
                palette,
            )
        };
        if narrow && pending {
            ui.painter().circle_filled(
                response.rect.right_top() + egui::vec2(-3.0, 3.0),
                3.0,
                palette.accent,
            );
        }
        components::popup(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(270.0);
                ui.label(bold_text("自動スケジューリング"));
                ui.label(
                    regular_text("計画する期間（カレンダーの表示日数とは別）")
                        .size(11.0)
                        .color(palette.muted),
                );
                ui.horizontal(|ui| {
                    for days in [1, 3, 7] {
                        ui.selectable_value(&mut self.planning_days, days, format!("{days}日間"));
                    }
                });
                let start =
                    parse_required_date(&self.target_date).unwrap_or(self.now_in_timezone().date());
                let end = add_days(start, self.planning_days as i32 - 1).unwrap_or(start);
                ui.label(format!("{} ～ {}", start, end));
                if components::button(ui, "この期間の計画を提案", true, palette).clicked()
                {
                    self.preview_auto_schedule();
                }
                if let Some(preview) = &self.auto_preview {
                    ui.separator();
                    ui.label(format!(
                        "未保存 · {}件の変更 · 未配置{}件",
                        preview.diff.changed_count(),
                        preview.output.unscheduled.len()
                    ));
                    for issue in &preview.output.issues {
                        ui.label(regular_text(item_issue_label(issue)).color(palette.warning));
                    }
                    if ui.button("提案を適用").clicked() {
                        self.apply_auto_schedule();
                        ui.close();
                    }
                } else if let Some(plan) = &self.plan {
                    ui.separator();
                    ui.label(format!("未保存の再計画 · {}件", plan.output.blocks.len()));
                    for issue in &plan.output.issues {
                        ui.label(regular_text(schedule_issue_label(issue)).color(palette.warning));
                    }
                    if ui.button("再計画を保存").clicked() {
                        self.save_schedule();
                        ui.close();
                    }
                }
                if pending
                    && ui
                        .button("未保存の提案を破棄")
                        .on_hover_text("プレビューのみ取り消します。保存済みの予定は削除しません。")
                        .clicked()
                {
                    self.auto_preview = None;
                    self.plan = None;
                    self.message.clear();
                    ui.close();
                }
            });
    }

    fn preview_repair_at_now(&mut self) {
        let now = self.now_in_timezone();
        let displayed = self.target_date.clone();
        self.target_date = now.date().to_string();
        self.auto_preview = None;
        self.preview_auto_schedule();
        self.target_date = displayed;
    }

    fn save_schedule(&mut self) {
        let Some(plan) = self.plan.clone() else {
            return;
        };
        let result = (|| -> Result<()> {
            let vault = self.vault_clone()?;
            self.runtime.block_on(async {
                let repo = vault.schedule_block_repo();
                SchedulePlanStoreService::new(repo.as_ref())
                    .save_repaired_plan(&plan)
                    .await?;
                Ok(())
            })
        })();
        match result {
            Ok(()) => {
                self.plan = None;
                self.refresh_schedule();
                self.refresh_schedule_month();
                self.message = "再計画を保存しました".into();
            }
            Err(error) => self.set_error(error),
        }
    }

    fn calendar_events(&self) -> Vec<CalendarEvent> {
        let preview_window = self
            .auto_preview
            .as_ref()
            .map(|preview| preview.planning_window)
            .or_else(|| {
                self.plan.as_ref().and_then(|plan| {
                    mnema_app::iana_date_range(
                        plan.target_date,
                        plan.target_date.next_day()?,
                        &self.timezone_offset,
                    )
                    .ok()
                })
            });
        let mut events: Vec<_> = self
            .schedule_month
            .iter()
            .filter(|block| {
                block.state != ScheduleBlockState::Cancelled
                    && (!preview_window.is_some_and(|window| {
                        block.start_at < window.end && block.end_at > window.start
                    }) || block.locked
                        || block.state != ScheduleBlockState::Proposed
                        || matches!(
                            block.source,
                            ScheduleBlockSource::Manual | ScheduleBlockSource::ExternalCalendar
                        ))
            })
            .map(|block| CalendarEvent {
                key: EventKey::Block(block.id.clone()),
                title: block
                    .title_snapshot
                    .clone()
                    .unwrap_or_else(|| "予定".into()),
                start: block.start_at,
                end: block.end_at,
                all_day: false,
                completed: block.state == ScheduleBlockState::Done,
                fixed: block.locked
                    || matches!(
                        block.source,
                        ScheduleBlockSource::Manual | ScheduleBlockSource::ExternalCalendar
                    )
                    || block.state != ScheduleBlockState::Proposed,
                habit: block.block_type == ScheduleBlockType::Habit,
                preview: false,
            })
            .collect();
        events.extend(
            self.workspace_ui
                .calendar
                .external_events
                .iter()
                .filter(|event| event.status != ExternalEventStatus::Cancelled)
                .map(|event| CalendarEvent {
                    key: EventKey::External(event.id.clone()),
                    title: event.title.clone(),
                    start: event.start_at,
                    end: event.end_at,
                    all_day: event.all_day,
                    completed: false,
                    fixed: true,
                    habit: false,
                    preview: false,
                }),
        );
        if let Some(preview) = &self.auto_preview {
            events.extend(
                preview
                    .output
                    .blocks
                    .iter()
                    .enumerate()
                    .map(|(index, block)| CalendarEvent {
                        key: EventKey::Preview(index),
                        title: block.title.clone(),
                        start: block.window.start,
                        end: block.window.end,
                        all_day: false,
                        completed: false,
                        fixed: false,
                        habit: matches!(
                            block.item_ref,
                            mnema_app::ScheduleItemRef::HabitOccurrence(_)
                        ),
                        preview: true,
                    }),
            );
        } else if let Some(plan) = &self.plan {
            events.extend(plan.output.blocks.iter().enumerate().map(|(index, block)| {
                CalendarEvent {
                    key: EventKey::Preview(index),
                    title: block.title.clone(),
                    start: block.window.start,
                    end: block.window.end,
                    all_day: false,
                    completed: false,
                    fixed: false,
                    habit: false,
                    preview: true,
                }
            }));
        }
        events.sort_by_key(|event| (event.start, event.end));
        events
    }

    fn calendar_summary(
        &self,
        ui: &mut egui::Ui,
        start: Date,
        end: Date,
        events: &[CalendarEvent],
        palette: Palette,
    ) {
        let Ok(window) = mnema_app::iana_date_range(start, end, &self.timezone_offset) else {
            return;
        };
        let active: Vec<_> = events
            .iter()
            .filter(|e| e.start < window.end && e.end > window.start)
            .collect();
        let tasks =
            active
                .iter()
                .filter_map(|event| match &event.key {
                    EventKey::Block(id) => self
                        .schedule_month
                        .iter()
                        .find(|block| &block.id == id)
                        .and_then(|block| block.task_id.clone()),
                    EventKey::Preview(index) => {
                        if let Some(preview) = &self.auto_preview {
                            preview.output.blocks.get(*index).and_then(|block| {
                                match &block.item_ref {
                                    mnema_app::ScheduleItemRef::Task(id) => Some(id.clone()),
                                    _ => None,
                                }
                            })
                        } else {
                            self.plan
                                .as_ref()
                                .and_then(|plan| plan.output.blocks.get(*index))
                                .map(|block| block.task_id.clone())
                        }
                    }
                    EventKey::External(_) => None,
                })
                .collect::<HashSet<_>>()
                .len();
        let planned = union_minutes(
            active
                .iter()
                .filter_map(|e| clip(e.start, e.end, window))
                .collect(),
        );
        let free = self
            .scheduling_preferences
            .as_ref()
            .and_then(|preferences| {
                let work = preferences
                    .named_hours
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case("work"))?;
                let available = mnema_app::expand_weekly_policy(
                    work,
                    start,
                    end,
                    &preferences.timezone,
                    window,
                )
                .ok()?;
                let mut busy: Vec<_> = active
                    .iter()
                    .filter(|e| !matches!(e.key, EventKey::External(_)))
                    .map(|e| TimeWindow::new(e.start, e.end))
                    .collect();
                busy.extend(
                    self.workspace_ui
                        .calendar
                        .external_events
                        .iter()
                        .filter(|e| e.blocks_time())
                        .map(|e| TimeWindow::new(e.start_at, e.end_at)),
                );
                busy.extend(
                    mnema_app::travel_busy_blocks(
                        &self.workspace_ui.calendar.external_events,
                        window,
                        mnema_app::TravelBufferPolicy::symmetric(
                            preferences.default_travel_buffer_minutes,
                        ),
                    )
                    .into_iter()
                    .map(|b| b.window),
                );
                if let Some(sleep) = &preferences.sleep {
                    busy.extend(
                        mnema_app::expand_weekly_policy(
                            sleep,
                            start,
                            end,
                            &preferences.timezone,
                            window,
                        )
                        .ok()?,
                    );
                }
                let total = union_minutes(available.clone());
                let occupied = union_minutes(
                    available
                        .iter()
                        .flat_map(|work| busy.iter().filter_map(|b| clip(b.start, b.end, *work)))
                        .collect(),
                );
                Some((total, (total - occupied).max(0)))
            });
        ui.horizontal_wrapped(|ui| {
            ui.label(
                regular_text(format!("タスク {tasks}件"))
                    .size(11.0)
                    .color(palette.accent),
            );
            ui.label(
                regular_text(format!("予定 {:.1}h", planned as f32 / 60.0))
                    .size(11.0)
                    .color(palette.muted),
            );
            if let Some((_, free)) = free {
                ui.label(
                    regular_text(format!("空き {:.1}h", free as f32 / 60.0))
                        .size(11.0)
                        .color(palette.success),
                )
                .on_hover_text("表示期間の作業時間内の空き。予定・睡眠・移動時間を除外します。");
            }
        });
        if let Some((total, free)) = free {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, palette.faint);
            let fraction = if total > 0 {
                (total - free) as f32 / total as f32
            } else {
                0.0
            };
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, 4.0)),
                2.0,
                palette.accent.gamma_multiply(0.65),
            );
        }
    }

    fn calendar_grid(
        &mut self,
        ui: &mut egui::Ui,
        start: Date,
        days: usize,
        events: &[CalendarEvent],
        compact: bool,
        palette: Palette,
    ) {
        let now = self.now_in_timezone();
        let timezone = self.app_timezone();
        let gutter = 50.0;
        let width = ui.available_width();
        let column = (width - gutter - 12.0) / days as f32;
        let offset = self.workspace_ui.calendar.pan[usize::from(compact)];
        let dates: Vec<_> = (-1..=days as i32)
            .filter_map(|i| add_days(start, i))
            .collect();
        let windows: Vec<_> = dates
            .iter()
            .filter_map(|date| {
                mnema_app::iana_date_range(*date, date.next_day()?, &self.timezone_offset).ok()
            })
            .collect();
        if windows.len() != days + 2 {
            return;
        }
        let (header, _) = ui.allocate_exact_size(egui::vec2(width, 50.0), egui::Sense::hover());
        let gp = ui.painter_at(header);
        let mut columns_clip = header;
        columns_clip.min.x += gutter;
        let p = ui.painter_at(columns_clip);
        use time_tz::{Offset, TimeZone};
        let timezone_label = timezones::get_by_name(&self.timezone_offset)
            .map(|tz| {
                tz.get_offset_utc(&OffsetDateTime::now_utc())
                    .name()
                    .to_owned()
            })
            .unwrap_or_else(|| "UTC".into());
        gp.text(
            egui::pos2(header.left(), header.bottom() - 10.0),
            egui::Align2::LEFT_CENTER,
            timezone_label,
            egui::FontId::proportional(10.0),
            palette.muted,
        );
        for (index, date) in dates.iter().enumerate() {
            let x = header.left() + gutter + column * (index as f32 - 1.0 - offset);
            let cell = egui::Rect::from_min_size(
                egui::pos2(x, header.top()),
                egui::vec2(column, header.height()),
            );
            let today = *date == now.date();
            let selected = date.to_string() == self.target_date;
            if selected {
                p.rect_filled(cell, 4.0, palette.faint);
            }
            let day_rect = egui::Rect::from_center_size(
                cell.center() + egui::vec2(0.0, 8.0),
                egui::vec2(27.0, 27.0),
            );
            if today {
                p.rect_filled(day_rect, 6.0, palette.error);
            }
            p.text(
                egui::pos2(cell.center().x, cell.top() + 7.0),
                egui::Align2::CENTER_TOP,
                ["月", "火", "水", "木", "金", "土", "日"]
                    [date.weekday().number_days_from_monday() as usize],
                egui::FontId::proportional(10.0),
                palette.muted,
            );
            p.text(
                day_rect.center(),
                egui::Align2::CENTER_CENTER,
                date.day(),
                egui::FontId::proportional(16.0),
                if today { Color32::WHITE } else { palette.text },
            );
            if ui
                .interact(
                    cell.intersect(columns_clip),
                    ui.id().with(("day_focus", date)),
                    egui::Sense::click(),
                )
                .clicked()
            {
                self.select_home_date(*date);
            }
        }
        let all_day_count = windows
            .iter()
            .map(|w| {
                events
                    .iter()
                    .filter(|e| e.all_day && e.start < w.end && e.end > w.start)
                    .count()
            })
            .max()
            .unwrap_or(0);
        let (all_day, _) = ui.allocate_exact_size(
            egui::vec2(
                width,
                (all_day_count as f32 * 24.0 + 4.0).clamp(25.0, 100.0),
            ),
            egui::Sense::hover(),
        );
        let gp = ui.painter_at(all_day);
        let mut columns_clip = all_day;
        columns_clip.min.x += gutter;
        let p = ui.painter_at(columns_clip);
        gp.text(
            egui::pos2(all_day.left(), all_day.top() + 10.0),
            egui::Align2::LEFT_CENTER,
            "終日",
            egui::FontId::proportional(10.0),
            palette.muted,
        );
        for (index, window) in windows.iter().enumerate() {
            for (row, event) in events
                .iter()
                .filter(|e| e.all_day && e.start < window.end && e.end > window.start)
                .enumerate()
            {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(
                        all_day.left() + gutter + column * (index as f32 - 1.0 - offset) + 3.0,
                        all_day.top() + row as f32 * 24.0,
                    ),
                    egui::vec2(column - 6.0, 21.0),
                );
                if rect.bottom() > all_day.bottom() {
                    break;
                }
                p.rect_filled(rect, 4.0, palette.selected_fill);
                p.with_clip_rect(p.clip_rect().intersect(rect)).text(
                    rect.left_center() + egui::vec2(5.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &event.title,
                    egui::FontId::proportional(11.0),
                    palette.text,
                );
                if ui
                    .interact(
                        rect.intersect(columns_clip),
                        ui.id().with((&event.key, dates[index])),
                        egui::Sense::click(),
                    )
                    .clicked()
                {
                    self.workspace_ui.calendar.selected = Some(event.key.clone());
                }
            }
        }
        let scroll_now = self.scroll_home_agenda_to_now;
        self.scroll_home_agenda_to_now = false;
        let mut output = HomeAgendaOutput::default();
        ScrollArea::vertical()
            .id_salt(("calendar_timeline", compact))
            .max_height(ui.available_height().max(100.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (grid, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 24.0 * 66.0),
                    egui::Sense::hover(),
                );
                let column = (grid.width() - gutter - 12.0) / days as f32;
                let gp = ui.painter_at(grid);
                let mut columns_clip = grid;
                columns_clip.min.x += gutter;
                columns_clip = columns_clip.intersect(ui.clip_rect());
                let p = ui.painter_at(columns_clip);
                let timezone_ref = timezones::get_by_name(&self.timezone_offset);
                let now_label_y = windows.iter().enumerate().find_map(|(index, window)| {
                    let left = grid.left() + gutter + column * (index as f32 - 1.0 - offset);
                    (now >= window.start
                        && now < window.end
                        && left + column > columns_clip.left()
                        && left < columns_clip.right())
                    .then(|| {
                        grid.top()
                            + (now - window.start).whole_seconds() as f32
                                / (window.end - window.start).whole_seconds() as f32
                                * grid.height()
                    })
                });
                let mut now_indicator = None;
                for (index, window) in windows.iter().enumerate() {
                    let day_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            grid.left() + gutter + column * (index as f32 - 1.0 - offset),
                            grid.top(),
                        ),
                        egui::vec2(column, grid.height()),
                    );
                    let total = (window.end - window.start).whole_minutes() as f32;
                    if dates[index] == now.date() {
                        p.rect_filled(day_rect, 0.0, palette.faint.gamma_multiply(0.28));
                    }
                    p.vline(
                        day_rect.left(),
                        day_rect.y_range(),
                        Stroke::new(1.0_f32, palette.border),
                    );
                    // Elapsed-time axis also handles 23/25-hour DST days without moving event instants.
                    for minute in (0..total as i64).step_by(60) {
                        let instant = window.start + time::Duration::minutes(minute);
                        let local = timezone_ref.map_or_else(
                            || instant.to_offset(timezone),
                            |tz| instant.to_timezone(tz),
                        );
                        let y = grid.top() + minute as f32 / total * grid.height();
                        p.hline(day_rect.x_range(), y, Stroke::new(0.7_f32, palette.border));
                        if index == 1
                            && now_label_y.is_none_or(|now_y| (y + 8.0 - now_y).abs() > 16.0)
                        {
                            gp.text(
                                egui::pos2(grid.left() + 39.0, y + 2.0),
                                egui::Align2::RIGHT_TOP,
                                format!("{:02}:00", local.hour()),
                                egui::FontId::proportional(10.0),
                                palette.muted,
                            );
                        }
                    }
                    let day_events: Vec<_> = events
                        .iter()
                        .filter(|e| {
                            !e.all_day
                                && e.start < window.end
                                && e.end > window.start
                                && (self.workspace_ui.calendar.show_completed || !e.completed)
                        })
                        .collect();
                    let lanes = event_lanes(&day_events);
                    for (i, event) in day_events.iter().enumerate() {
                        let (lane, lane_count) = lanes[i];
                        let y = grid.top()
                            + (event.start.max(window.start) - window.start).whole_minutes() as f32
                                / total
                                * grid.height();
                        let end_y = grid.top()
                            + (event.end.min(window.end) - window.start).whole_minutes() as f32
                                / total
                                * grid.height();
                        let lane_width = (column - 8.0) / lane_count as f32;
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(day_rect.left() + 4.0 + lane as f32 * lane_width, y + 1.0),
                            egui::vec2((lane_width - 3.0).max(4.0), (end_y - y - 2.0).max(20.0)),
                        );
                        if !ui.is_rect_visible(rect) {
                            continue;
                        }
                        let response = ui.interact(
                            rect.intersect(columns_clip),
                            ui.id().with(("event", &event.key, dates[index])),
                            if self.schedule_month.iter().any(|block| {
                                event.key == EventKey::Block(block.id.clone())
                                    && matches!(
                                        block.block_type,
                                        ScheduleBlockType::Task | ScheduleBlockType::Habit
                                    )
                                    && block.source != ScheduleBlockSource::ExternalCalendar
                            }) && !event.completed
                            {
                                egui::Sense::click_and_drag()
                            } else {
                                egui::Sense::click()
                            },
                        );
                        let hover = ui.ctx().animate_bool_with_time(
                            response.id.with("hover"),
                            response.hovered(),
                            components::HOVER_SECONDS,
                        );
                        let base = if matches!(event.key, EventKey::External(_)) {
                            palette.warning
                        } else if event.habit {
                            palette.success
                        } else {
                            palette.accent
                        };
                        let faded = event.completed || event.end < now;
                        let color = mix_color(base, palette.muted, if faded { 0.65 } else { 0.0 });
                        p.rect_filled(
                            rect,
                            4.0,
                            mix_color(
                                palette.surface,
                                color,
                                if faded { 0.07 } else { 0.15 + hover * 0.08 },
                            ),
                        );
                        if !event.fixed {
                            dashed_outline(&p, rect, color.gamma_multiply(0.7));
                        }
                        p.rect_filled(
                            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
                            2.0,
                            color,
                        );
                        let text_painter =
                            p.with_clip_rect(p.clip_rect().intersect(rect.shrink(4.0)));
                        let prefix = if event.completed {
                            "✓ "
                        } else if event.preview {
                            "提案 · "
                        } else {
                            ""
                        };
                        text_painter.text(
                            rect.left_top() + egui::vec2(8.0, 4.0),
                            egui::Align2::LEFT_TOP,
                            format!("{prefix}{}", event.title),
                            egui::FontId::proportional(12.0),
                            if faded { palette.muted } else { palette.text },
                        );
                        if rect.height() > 36.0 {
                            text_painter.text(
                                rect.left_top() + egui::vec2(8.0, 22.0),
                                egui::Align2::LEFT_TOP,
                                format!(
                                    "{}–{}{}",
                                    format_hm_in(
                                        event.start,
                                        timezone_ref.map_or(timezone, |tz| event
                                            .start
                                            .to_timezone(tz)
                                            .offset())
                                    ),
                                    format_hm_in(
                                        event.end,
                                        timezone_ref.map_or(timezone, |tz| event
                                            .end
                                            .to_timezone(tz)
                                            .offset())
                                    ),
                                    if event.fixed {
                                        " · 固定"
                                    } else {
                                        " · 自動"
                                    }
                                ),
                                egui::FontId::proportional(10.0),
                                color,
                            );
                        }
                        if response.clicked() {
                            self.workspace_ui.calendar.selected = Some(event.key.clone());
                            self.clear_schedule_block_editor();
                        }
                        if let EventKey::Block(id) = &event.key {
                            let grab = ui
                                .input(|input| input.pointer.press_origin())
                                .map_or(0, |pos| {
                                    ((pos.y - y) / grid.height() * total).max(0.0) as i64
                                });
                            response
                                .clone()
                                .dnd_set_drag_payload(ScheduleBlockDragPayload {
                                    block_id: id.clone(),
                                    title: event.title.clone(),
                                    duration_minutes: (event.end - event.start).whole_minutes(),
                                    grab_offset_minutes: grab,
                                });
                        }
                        response
                            .on_hover_text(format!(
                                "{}\n{}–{}",
                                event.title,
                                format_hm_in(event.start, timezone),
                                format_hm_in(event.end, timezone)
                            ))
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                    }
                    if now >= window.start
                        && now < window.end
                        && day_rect.right() > columns_clip.left()
                        && day_rect.left() < columns_clip.right()
                    {
                        let y = grid.top()
                            + (now - window.start).whole_seconds() as f32 / 60.0 / total
                                * grid.height();
                        p.hline(
                            (grid.left() + gutter)..=grid.right(),
                            y,
                            Stroke::new(0.5_f32, palette.error.gamma_multiply(0.3)),
                        );
                        p.hline(day_rect.x_range(), y, Stroke::new(1.5_f32, palette.error));
                        p.circle_filled(egui::pos2(day_rect.left() + 1.0, y), 3.5, palette.error);
                        let clock = egui::Rect::from_center_size(
                            egui::pos2(grid.left() + 23.0, y),
                            egui::vec2(44.0, 18.0),
                        );
                        gp.rect_filled(clock, 4.0, palette.error);
                        gp.text(
                            clock.center(),
                            egui::Align2::CENTER_CENTER,
                            format_hm(now),
                            egui::FontId::proportional(10.0),
                            Color32::WHITE,
                        );
                        now_indicator = Some(egui::Rect::from_center_size(
                            egui::pos2(day_rect.right(), y),
                            egui::vec2(24.0, 24.0),
                        ));
                        if scroll_now {
                            ui.scroll_to_rect(
                                egui::Rect::from_min_size(
                                    egui::pos2(grid.left(), (y - 70.0).max(grid.top())),
                                    egui::vec2(1.0, 140.0),
                                ),
                                Some(Align::Min),
                            );
                        }
                    }
                    if let Some(pointer) = ui
                        .input(|input| input.pointer.hover_pos())
                        .filter(|pos| day_rect.contains(*pos) && columns_clip.contains(*pos))
                    {
                        let task_payload = egui::DragAndDrop::payload::<TaskDragPayload>(ui.ctx());
                        let block_payload =
                            egui::DragAndDrop::payload::<ScheduleBlockDragPayload>(ui.ctx());
                        if task_payload.is_some() || block_payload.is_some() {
                            let duration = block_payload.as_ref().map_or_else(
                                || {
                                    task_payload
                                        .as_ref()
                                        .and_then(|t| t.estimated_minutes)
                                        .unwrap_or(30) as i64
                                },
                                |b| b.duration_minutes,
                            );
                            let y = pointer.y
                                - block_payload.as_ref().map_or(0.0, |b| {
                                    agenda_pixels_for_minutes(
                                        b.grab_offset_minutes,
                                        day_rect,
                                        total,
                                    )
                                });
                            let at = agenda_time_from_y_for_duration(
                                y,
                                day_rect,
                                window.start,
                                total,
                                duration,
                            );
                            let y = grid.top()
                                + (at - window.start).whole_minutes() as f32 / total
                                    * grid.height();
                            p.hline(day_rect.x_range(), y, Stroke::new(2.0_f32, palette.accent));
                            p.text(
                                egui::pos2(day_rect.right() - 5.0, y - 3.0),
                                egui::Align2::RIGHT_BOTTOM,
                                format_hm_in(at, timezone),
                                egui::FontId::proportional(11.0),
                                palette.accent,
                            );
                            if ui.input(|input| input.pointer.any_released()) {
                                if task_payload.is_some()
                                    && let Some(payload) =
                                        egui::DragAndDrop::take_payload::<TaskDragPayload>(ui.ctx())
                                {
                                    output.manual_schedule = Some(ManualScheduleRequest {
                                        task_id: payload.task_id.clone(),
                                        title: payload.title.clone(),
                                        start_at: at,
                                        duration_minutes: duration as u32,
                                    });
                                } else if let Some(payload) =
                                    egui::DragAndDrop::take_payload::<ScheduleBlockDragPayload>(
                                        ui.ctx(),
                                    )
                                {
                                    output.move_schedule = Some(MoveScheduleBlockRequest {
                                        block_id: payload.block_id.clone(),
                                        title: payload.title.clone(),
                                        start_at: at,
                                        duration_minutes: duration,
                                    });
                                }
                            }
                        }
                    }
                }
                if let Some(indicator) = now_indicator
                    && columns_clip.contains(indicator.center())
                {
                    let response = ui.interact(
                        indicator.intersect(columns_clip),
                        ui.id().with("repair_at_now"),
                        egui::Sense::click(),
                    );
                    let turn = ui.ctx().animate_bool_with_time(
                        response.id.with("turn"),
                        response.hovered(),
                        if response.hovered() { 0.3 } else { 0.0 },
                    );
                    p.circle_filled(indicator.center(), 11.0, palette.surface);
                    let angle = turn * std::f32::consts::TAU;
                    let points: Vec<_> = (0..=20)
                        .map(|i| {
                            let a = angle + i as f32 / 20.0 * 5.0;
                            indicator.center() + egui::vec2(a.cos(), a.sin()) * 6.5
                        })
                        .collect();
                    let end = *points.last().unwrap();
                    p.add(egui::Shape::line(
                        points,
                        Stroke::new(1.5_f32, palette.error),
                    ));
                    let a = angle + 5.0;
                    let tangent = egui::vec2(-a.sin(), a.cos());
                    let normal = egui::vec2(a.cos(), a.sin());
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            end + tangent * 2.0,
                            end - tangent * 3.0 + normal * 3.0,
                            end - tangent * 3.0 - normal * 3.0,
                        ],
                        palette.error,
                        Stroke::NONE,
                    ));
                    if response
                        .on_hover_text("現在時刻から再計画を提案")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        output.repair_clicked = true;
                    }
                }
                if scroll_now && !dates.contains(&now.date()) {
                    ui.scroll_to_rect(
                        egui::Rect::from_min_size(
                            grid.min + egui::vec2(0.0, 8.0 * 66.0),
                            egui::vec2(1.0, 1.0),
                        ),
                        Some(Align::Min),
                    );
                }
            });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(30));
        if output.repair_clicked {
            self.preview_repair_at_now();
        }
        if let Some(request) = output.manual_schedule {
            self.create_manual_schedule_block(request);
        }
        if let Some(request) = output.move_schedule {
            self.move_schedule_block(request);
        }
    }

    pub(super) fn show_calendar_event_details(&mut self, ctx: &egui::Context, palette: Palette) {
        let Some(key) = self.workspace_ui.calendar.selected.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new("予定の詳細")
            .id(egui::Id::new("calendar_event_details"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(340.0)
            .anchor(egui::Align2::RIGHT_TOP, [-32.0, 100.0])
            .show(ctx, |ui| {
                if let EventKey::External(id) = &key {
                    if let Some(event) = self
                        .workspace_ui
                        .calendar
                        .external_events
                        .iter()
                        .find(|e| &e.id == id)
                    {
                        ui.label(bold_text(&event.title).size(20.0));
                        ui.label(format!(
                            "{}  {}–{}",
                            home::japanese_date(
                                event.start_at.to_offset(self.app_timezone()).date()
                            ),
                            format_hm_in(event.start_at, self.app_timezone()),
                            format_hm_in(event.end_at, self.app_timezone())
                        ));
                        ui.label(regular_text("連携カレンダーの予定").color(palette.muted));
                        if let Some(location) = &event.location {
                            ui.label(location);
                        }
                        if let Some(description) = &event.description {
                            ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                                ui.label(description);
                            });
                        }
                    }
                    return;
                }
                let EventKey::Block(id) = &key else {
                    ui.label("未保存の提案です。カレンダー上部の「適用」で保存できます。");
                    return;
                };
                let Some(block) = self.schedule_month.iter().find(|b| &b.id == id).cloned() else {
                    ui.label("この予定は更新されました。");
                    return;
                };
                ui.label(bold_text(block.title_snapshot.as_deref().unwrap_or("予定")).size(20.0));
                ui.add_space(8.0);
                ui.label(home::japanese_date(
                    block.start_at.to_offset(self.app_timezone()).date(),
                ));
                ui.label(format!(
                    "{}–{} · {}分",
                    format_hm_in(block.start_at, self.app_timezone()),
                    format_hm_in(block.end_at, self.app_timezone()),
                    (block.end_at - block.start_at).whole_minutes()
                ));
                ui.label(regular_text(schedule_state_label(&block.state)).color(palette.muted));
                ui.add_space(12.0);
                let editable = matches!(
                    block.block_type,
                    ScheduleBlockType::Task | ScheduleBlockType::Habit
                );
                if editable {
                    ui.horizontal_wrapped(|ui| {
                        if block.state != ScheduleBlockState::Done
                            && components::button(ui, "✓ 予定を完了", true, palette).clicked()
                        {
                            self.update_schedule_block_state(
                                block.id.clone(),
                                ScheduleBlockState::Done,
                            );
                        }
                        if block.state == ScheduleBlockState::Done
                            && components::button(ui, "予定に戻す", false, palette).clicked()
                        {
                            self.update_schedule_block_state(
                                block.id.clone(),
                                ScheduleBlockState::Scheduled,
                            );
                        }
                        if components::button(ui, "時刻を編集", false, palette).clicked() {
                            self.select_home_date(
                                block.start_at.to_offset(self.app_timezone()).date(),
                            );
                            self.start_edit_schedule_block(block.id.clone());
                        }
                    });
                    ui.add_space(10.0);
                    if matches!(
                        block.state,
                        ScheduleBlockState::Proposed | ScheduleBlockState::Scheduled
                    ) {
                        let mut fixed = block.locked
                            || block.source == ScheduleBlockSource::Manual
                            || block.state != ScheduleBlockState::Proposed;
                        if ui.checkbox(&mut fixed, "この時間に固定する").changed() {
                            let block_id = block.id.clone();
                            let result = self.vault_clone().and_then(|vault| {
                                self.runtime.block_on(async move {
                                    let repo = vault.schedule_block_repo();
                                    ScheduleBlockCommandService::new(repo.as_ref())
                                        .set_fixed(block_id, fixed)
                                        .await
                                        .map_err(Into::into)
                                })
                            });
                            match result {
                                Ok(_) => {
                                    self.auto_preview = None;
                                    self.plan = None;
                                    self.refresh_schedule();
                                    self.refresh_schedule_month();
                                }
                                Err(error) => self.set_error(error),
                            }
                        }
                        ui.label(
                            regular_text("固定を解除すると、次の自動計画で時間を調整できます。")
                                .size(11.0)
                                .color(palette.muted),
                        );
                    }
                    if let Some(task_id) = &block.task_id
                        && components::button(ui, "タスクの詳細を開く", false, palette).clicked()
                    {
                        self.workspace_ui.task_editor = self
                            .tasks
                            .iter()
                            .chain(self.done_tasks.iter())
                            .find(|t| &t.id == task_id)
                            .cloned();
                    }
                    self.show_schedule_block_editor(ui, palette);
                    ui.separator();
                    let more = components::menu_button(ui, "その他", false, palette);
                    components::popup(&more).show(|ui| {
                        if ui.button("予定を取り消す").clicked() {
                            self.update_schedule_block_state(
                                block.id.clone(),
                                ScheduleBlockState::Cancelled,
                            );
                            self.workspace_ui.calendar.selected = None;
                            ui.close();
                        }
                    });
                }
            });
        if !open {
            self.workspace_ui.calendar.selected = None;
            self.clear_schedule_block_editor();
        }
    }
}

fn mode_label(mode: ScheduleViewMode) -> String {
    match mode {
        ScheduleViewMode::Day => "日".into(),
        ScheduleViewMode::Days(days) => format!("{days}日"),
        ScheduleViewMode::Week => "週".into(),
        ScheduleViewMode::Calendar => "月".into(),
    }
}
fn mode_days(mode: ScheduleViewMode) -> i32 {
    match mode {
        ScheduleViewMode::Days(days) => days as i32,
        ScheduleViewMode::Week => 7,
        _ => 1,
    }
}
fn shift_date(date: Date, mode: ScheduleViewMode, direction: i32) -> Date {
    if mode == ScheduleViewMode::Calendar {
        let mut value = date.to_string();
        shift_month(&mut value, direction);
        parse_required_date(&value).unwrap_or(date)
    } else {
        add_days(
            date,
            direction
                * match mode {
                    ScheduleViewMode::Days(days) => days as i32,
                    ScheduleViewMode::Week => 7,
                    _ => 1,
                },
        )
        .unwrap_or(date)
    }
}
fn clip(start: OffsetDateTime, end: OffsetDateTime, window: TimeWindow) -> Option<TimeWindow> {
    let (start, end) = (start.max(window.start), end.min(window.end));
    (start < end).then(|| TimeWindow::new(start, end))
}
pub(super) fn union_minutes(mut windows: Vec<TimeWindow>) -> i64 {
    windows.sort_by_key(|w| w.start);
    let mut total = 0;
    let mut current: Option<TimeWindow> = None;
    for window in windows {
        match current.as_mut() {
            Some(current) if window.start <= current.end => {
                current.end = current.end.max(window.end)
            }
            _ => {
                if let Some(previous) = current {
                    total += (previous.end - previous.start).whole_minutes();
                }
                current = Some(window);
            }
        }
    }
    total + current.map_or(0, |w| (w.end - w.start).whole_minutes())
}

// Partition connected overlap groups; each event keeps its own lane throughout its duration.
fn event_lanes(events: &[&CalendarEvent]) -> Vec<(usize, usize)> {
    let mut result = vec![(0, 1); events.len()];
    let mut begin = 0;
    while begin < events.len() {
        let mut end = begin + 1;
        let mut until = events[begin].end;
        while end < events.len() && events[end].start < until {
            until = until.max(events[end].end);
            end += 1;
        }
        let mut lanes: Vec<OffsetDateTime> = Vec::new();
        for i in begin..end {
            let lane = lanes
                .iter()
                .position(|until| *until <= events[i].start)
                .unwrap_or(lanes.len());
            if lane == lanes.len() {
                lanes.push(events[i].end);
            } else {
                lanes[lane] = events[i].end;
            }
            result[i].0 = lane;
        }
        for position in &mut result[begin..end] {
            position.1 = lanes.len();
        }
        begin = end;
    }
    result
}

fn dashed_outline(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    for y in [rect.top(), rect.bottom()] {
        let mut x = rect.left();
        while x < rect.right() {
            painter.hline(
                x..=(x + 4.0).min(rect.right()),
                y,
                Stroke::new(1.0_f32, color),
            );
            x += 7.0;
        }
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.vline(
            rect.right(),
            y..=(y + 4.0).min(rect.bottom()),
            Stroke::new(1.0_f32, color),
        );
        y += 7.0;
    }
}

pub(super) fn parse_event_time(
    date: Date,
    value: &str,
    timezone: &str,
    earlier: bool,
) -> Result<OffsetDateTime> {
    use time_tz::{OffsetResult, PrimitiveDateTimeExt};
    let tz = timezones::get_by_name(timezone).ok_or_else(|| anyhow!("タイムゾーンが無効です"))?;
    let time = Time::parse(value.trim(), format_description!("[hour]:[minute]"))?;
    match date.with_time(time).assume_timezone(tz) {
        OffsetResult::Some(value) => Ok(value),
        OffsetResult::Ambiguous(first, second) => Ok(if earlier {
            first.min(second)
        } else {
            first.max(second)
        }),
        OffsetResult::None => Err(anyhow!("夏時間への切り替えにより、この時刻は存在しません")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn busy_minutes_merge_overlaps_and_clip_cross_midnight() {
        let day = TimeWindow::new(
            datetime!(2026-09-21 00:00 UTC),
            datetime!(2026-09-22 00:00 UTC),
        );
        let windows = vec![
            clip(
                datetime!(2026-09-20 23:30 UTC),
                datetime!(2026-09-21 00:30 UTC),
                day,
            )
            .unwrap(),
            TimeWindow::new(
                datetime!(2026-09-21 00:15 UTC),
                datetime!(2026-09-21 01:00 UTC),
            ),
            TimeWindow::new(
                datetime!(2026-09-21 00:20 UTC),
                datetime!(2026-09-21 00:40 UTC),
            ),
            TimeWindow::new(
                datetime!(2026-09-21 02:00 UTC),
                datetime!(2026-09-21 02:30 UTC),
            ),
        ];
        assert_eq!(union_minutes(windows), 90);
        assert!(
            clip(
                datetime!(2026-09-20 22:00 UTC),
                datetime!(2026-09-21 00:00 UTC),
                day
            )
            .is_none()
        );
    }

    #[test]
    fn overlapping_events_share_lanes_but_adjacent_events_reuse_full_width() {
        let base = datetime!(2026-09-21 09:00 UTC);
        let events: Vec<_> = [(0, 60), (15, 30), (30, 90), (90, 120)]
            .into_iter()
            .enumerate()
            .map(|(i, (start, end))| CalendarEvent {
                key: EventKey::Preview(i),
                title: "test".into(),
                start: base + time::Duration::minutes(start),
                end: base + time::Duration::minutes(end),
                all_day: false,
                completed: false,
                fixed: false,
                habit: false,
                preview: false,
            })
            .collect();
        assert_eq!(
            event_lanes(&events.iter().collect::<Vec<_>>()),
            vec![(0, 2), (1, 2), (1, 2), (0, 1)]
        );
    }

    #[test]
    fn event_editor_uses_date_specific_timezone_and_rejects_nonexistent_time() {
        assert_eq!(
            parse_event_time(date!(2026 - 01 - 15), "09:00", "America/New_York", true).unwrap(),
            datetime!(2026-01-15 14:00 UTC)
        );
        assert_eq!(
            parse_event_time(date!(2026 - 07 - 15), "09:00", "America/New_York", true).unwrap(),
            datetime!(2026-07-15 13:00 UTC)
        );
        assert!(
            parse_event_time(date!(2026 - 03 - 08), "02:30", "America/New_York", true).is_err()
        );
    }

    #[test]
    fn calendar_drag_preserves_grab_offset_and_moves_to_another_day() {
        let (_directory, mut app, ctx) = super::super::interaction_tests::fixture();
        let day = app.now_in_timezone().date();
        app.quick_capture = "Calendar drag fixture /minutes 60".into();
        app.submit_composer(None);
        let task = app.tasks[0].clone();
        let start = parse_event_time(day, "09:00", &app.timezone_offset, true).unwrap();
        app.create_manual_schedule_block(ManualScheduleRequest {
            task_id: task.id,
            title: task.title,
            start_at: start,
            duration_minutes: 60,
        });
        let block_id = app.schedule[0].id.clone();
        app.scroll_home_agenda_to_now = false;
        configure_style(&ctx, 1.0, true);
        let frame = |app: &mut MnemaGuiApp, events: Vec<egui::Event>| {
            let calendar_events = app.calendar_events();
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(950.0, 950.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    app.calendar_grid(ui, day, 2, &calendar_events, false, Palette::at(1.0));
                },
            )
        };
        let first = frame(&mut app, vec![]);
        let title = first
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains("Calendar drag fixture") => {
                    Some(text.pos)
                }
                _ => None,
            })
            .expect("event title must be visible");
        let grab = title + egui::vec2(25.0, 12.0);
        let pointer = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut app, vec![egui::Event::PointerMoved(grab)]);
        frame(&mut app, vec![pointer(grab, true)]);
        let destination = grab + egui::vec2(450.0, 66.0);
        frame(&mut app, vec![egui::Event::PointerMoved(destination)]);
        let payload = egui::DragAndDrop::payload::<ScheduleBlockDragPayload>(&ctx)
            .expect("schedule block drag payload");
        assert!(payload.grab_offset_minutes > 0 && payload.grab_offset_minutes < 30);
        frame(&mut app, vec![pointer(destination, false)]);
        assert!(app.error.is_none(), "{:?}", app.error);
        let moved = app
            .schedule_month
            .iter()
            .find(|block| block.id == block_id)
            .unwrap();
        assert_eq!(
            moved.start_at,
            start + time::Duration::days(1) + time::Duration::hours(1)
        );
        assert_eq!((moved.end_at - moved.start_at).whole_minutes(), 60);
        assert!(
            app.workspace_ui.calendar.selected.is_none(),
            "drag must not open event details"
        );
    }
}
