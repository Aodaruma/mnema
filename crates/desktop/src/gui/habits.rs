use super::*;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum HabitView {
    #[default]
    Gallery,
    Priority,
    Templates,
}

const TEMPLATES: [(&str, &str, char, u32, &str, &str); 6] = [
    (
        "ランチ",
        "毎日の休憩を確保",
        '\u{e56c}',
        60,
        "12:00",
        "14:00",
    ),
    (
        "集中する時間",
        "平日にまとまった時間を",
        '\u{e3b4}',
        90,
        "09:00",
        "12:00",
    ),
    ("運動", "身体を動かす習慣", '\u{e566}', 30, "17:00", "20:00"),
    ("読書", "毎日少しずつ読む", '\u{e865}', 30, "20:00", "22:00"),
    (
        "1日の振り返り",
        "終業前に整理する",
        '\u{e8f4}',
        15,
        "16:00",
        "18:00",
    ),
    ("散歩", "外に出て気分転換", '\u{e536}', 20, "12:00", "15:00"),
];

impl MnemaGuiApp {
    pub(super) fn show_habits(&mut self, ui: &mut egui::Ui, palette: Palette) {
        if self.workspace_ui.habit_create_open {
            self.habit_form(ui, palette);
            return;
        }
        pages::page_heading(ui, "習慣", palette);
        ui.horizontal_wrapped(|ui| {
            for (view, label) in [
                (HabitView::Gallery, "すべての習慣"),
                (HabitView::Priority, "優先度別"),
                (HabitView::Templates, "テンプレート"),
            ] {
                pages::tab(ui, &mut self.workspace_ui.habit_view, view, label, palette);
            }
            ui.add_sized(
                [170.0, 28.0],
                text_field(&mut self.workspace_ui.habit_search, "習慣を検索"),
            );
            if components::button(ui, "＋ 習慣を作成", true, palette).clicked() {
                self.open_habit_form(None);
            }
        });
        ui.add_space(24.0);
        let query = self.workspace_ui.habit_search.to_lowercase();
        let habits: Vec<_> = self
            .habits
            .iter()
            .filter(|habit| habit.title.to_lowercase().contains(&query))
            .cloned()
            .collect();
        if self.workspace_ui.habit_view != HabitView::Templates && self.habits.is_empty() {
            pages::empty_state(
                ui,
                ICON_HABIT,
                "習慣はまだ登録されていません",
                "新しく作成するか、テンプレートを選んで最初の習慣を登録しましょう。",
                palette,
            );
            ui.vertical_centered(|ui| {
                if components::button(ui, "習慣を作成する", true, palette).clicked() {
                    self.open_habit_form(None);
                }
                ui.add_space(8.0);
                if components::button(ui, "テンプレートを見る", false, palette).clicked() {
                    self.workspace_ui.habit_search.clear();
                    self.workspace_ui.habit_view = HabitView::Templates;
                }
            });
            return;
        }
        if self.workspace_ui.habit_view == HabitView::Templates {
            ui.label(bold_text("テンプレートから始める").size(18.0));
            ui.label(
                regular_text("選んだ後に、曜日・時間帯・所要時間を調整できます。")
                    .size(12.0)
                    .color(palette.muted),
            );
            ui.add_space(18.0);
            let columns = (ui.available_width() / 290.0).floor().clamp(1.0, 3.0) as usize;
            let matching: Vec<_> = TEMPLATES
                .iter()
                .filter(|template| template.0.to_lowercase().contains(&query))
                .collect();
            for row in matching.chunks(columns) {
                ui.columns(columns, |uis| {
                    for (column, template) in row.iter().enumerate() {
                        home::card(palette).show(&mut uis[column], |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_min_height(160.0);
                            ui.label(material_icon_text(template.2, 28.0, palette.accent));
                            ui.label(bold_text(template.0).size(17.0));
                            ui.label(regular_text(template.1).size(12.0).color(palette.muted));
                            ui.add_space(12.0);
                            if components::button(ui, "この習慣を設定 →", false, palette).clicked()
                            {
                                self.open_habit_form(None);
                                self.habit_draft.title = template.0.into();
                                self.habit_draft.duration_minutes = template.3.to_string();
                                self.habit_draft.preferred_start = template.4.into();
                                self.habit_draft.preferred_end = template.5.into();
                                if template.0 == "集中する時間" || template.0 == "1日の振り返り"
                                {
                                    self.habit_draft.schedule = HabitScheduleChoice::Weekdays;
                                }
                            }
                        });
                    }
                });
                ui.add_space(16.0);
            }
            return;
        }
        if self.workspace_ui.habit_view == HabitView::Priority {
            ui.label(
                regular_text("自動配置の条件別に表示。必須の習慣を先に確保します。")
                    .size(12.0)
                    .color(palette.muted),
            );
            ui.add_space(12.0);
            // The model has two scheduling tiers; a time window constrains flexible habits, not a stored priority.
            ScrollArea::horizontal()
                .id_salt("habit_board")
                .show(ui, |ui| {
                    let width = ui.available_width().max(850.0);
                    ui.set_min_width(width);
                    ui.columns(3, |uis| {
                        for (index, label) in
                            ["必ず確保", "時間帯あり", "柔軟に配置"].iter().enumerate()
                        {
                            let group: Vec<_> = habits
                                .iter()
                                .filter(|habit| habit_column(habit) == index)
                                .collect();
                            uis[index]
                                .label(bold_text(format!("{label}  {}", group.len())).size(14.0));
                            uis[index].add_space(12.0);
                            for habit in group {
                                self.habit_card(&mut uis[index], habit, palette);
                                uis[index].add_space(12.0);
                            }
                            if components::button(&mut uis[index], "＋ 習慣を追加", false, palette)
                                .clicked()
                            {
                                self.open_habit_form(None);
                                self.habit_draft.flexibility = if index == 0 {
                                    HabitFlexibility::Required
                                } else {
                                    HabitFlexibility::Flexible
                                };
                                self.habit_draft.has_preferred_window = index != 2;
                            }
                        }
                    });
                });
        } else {
            let columns = (ui.available_width() / 330.0).floor().clamp(1.0, 3.0) as usize;
            for row in habits.chunks(columns) {
                ui.columns(columns, |uis| {
                    for (column, habit) in row.iter().enumerate() {
                        self.habit_card(&mut uis[column], habit, palette);
                    }
                });
                ui.add_space(16.0);
            }
            if habits.is_empty() {
                pages::empty_state(
                    ui,
                    ICON_HABIT,
                    "該当する習慣はありません",
                    "検索条件を変更してください。",
                    palette,
                );
            }
        }
    }

    fn habit_card(&mut self, ui: &mut egui::Ui, habit: &Habit, palette: Palette) {
        home::card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(172.0);
            ui.horizontal(|ui| {
                ui.label(material_icon_text(ICON_HABIT, 23.0, palette.accent));
                ui.add(egui::Label::new(bold_text(&habit.title).size(17.0)).truncate());
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let more = components::icon_button(ui, '\u{e5d3}', "習慣の操作", palette);
                    components::popup(&more).show(|ui| {
                        if ui.button("設定を編集").clicked() {
                            self.open_habit_form(Some(habit.clone()));
                            ui.close();
                        }
                        if ui.button("この習慣を停止").clicked() {
                            self.disable_habit(habit.id.clone());
                            ui.close();
                        }
                    });
                });
            });
            ui.label(
                regular_text(format!(
                    "{} · {}分",
                    habit_schedule_label(habit),
                    habit.duration_minutes
                ))
                .size(12.0)
                .color(palette.muted),
            );
            ui.label(
                regular_text(["必ず確保", "時間帯あり", "柔軟に配置"][habit_column(habit)])
                    .size(11.0)
                    .color(if habit.flexibility == HabitFlexibility::Required {
                        palette.warning
                    } else {
                        palette.accent
                    }),
            );
            if let Some(window) = habit.preferred_window {
                ui.label(
                    regular_text(format!(
                        "{:02}:{:02}–{:02}:{:02}",
                        window.start.hour(),
                        window.start.minute(),
                        window.end.hour(),
                        window.end.minute()
                    ))
                    .size(12.0)
                    .color(palette.muted),
                );
            }
            let next = self
                .habit_occurrences
                .iter()
                .filter(|o| {
                    o.habit_id == habit.id
                        && !matches!(
                            o.state,
                            HabitOccurrenceState::Done | HabitOccurrenceState::Skipped
                        )
                })
                .min_by_key(|o| o.occurrence_date)
                .cloned();
            ui.add_space(14.0);
            ui.separator();
            if let Some(occurrence) = next {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        regular_text(format!(
                            "次回 {}",
                            home::japanese_date(occurrence.occurrence_date)
                        ))
                        .size(11.0)
                        .color(palette.muted),
                    );
                    let next = components::menu_button(ui, "次回の操作", false, palette);
                    components::popup(&next).show(|ui| {
                        if ui.button("予定を開く").clicked() {
                            self.select_home_date(occurrence.occurrence_date);
                            self.view = View::Schedule;
                            ui.close();
                        }
                        if ui.button("今回だけスキップ").clicked() {
                            self.skip_habit_occurrence(occurrence.id.clone());
                            ui.close();
                        }
                        if ui.button("明日に延期").clicked() {
                            self.snooze_habit_occurrence(occurrence.id.clone());
                            ui.close();
                        }
                    });
                });
            }
            if components::button(ui, "設定を開く →", false, palette).clicked() {
                self.open_habit_form(Some(habit.clone()));
            }
        });
    }

    fn open_habit_form(&mut self, habit: Option<Habit>) {
        self.habit_draft = habit
            .as_ref()
            .map_or_else(HabitDraft::default, HabitDraft::from_habit);
        self.workspace_ui.editing_habit = habit;
        self.workspace_ui.habit_create_open = true;
        self.error = None;
    }

    fn habit_form(&mut self, ui: &mut egui::Ui, palette: Palette) {
        if components::button(ui, "‹ 習慣に戻る", false, palette).clicked() {
            self.workspace_ui.habit_create_open = false;
            return;
        }
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(ui.available_width().min(760.0));
            ui.with_layout(egui::Layout::top_down(Align::Min), |ui| {
                pages::page_heading(ui, if self.workspace_ui.editing_habit.is_some() { "習慣を編集" } else { "新しい習慣" }, palette);
                home::card(palette).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(bold_text("習慣の詳細").size(18.0)); ui.add_space(16.0);
                    ui.label("名前");
                    ui.add_sized([ui.available_width(), 38.0], text_field(&mut self.habit_draft.title, "例：読書"));
                    ui.add_space(12.0); ui.label("自動配置の優先度");
                    ui.horizontal_wrapped(|ui| {
                        for (value, label) in [(HabitFlexibility::Required, "必ず確保"), (HabitFlexibility::Flexible, "空き時間に配置")] {
                            if components::button(ui, label, self.habit_draft.flexibility == value, palette).clicked() { self.habit_draft.flexibility = value; }
                        }
                    });
                    ui.label(regular_text("「必ず確保」は、タスクより先に時間を確保します。").size(11.0).color(palette.muted));
                });
                ui.add_space(16.0);
                home::card(palette).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(bold_text("スケジューリング").size(18.0)); ui.add_space(16.0);
                    ui.label("繰り返し");
                    ui.horizontal_wrapped(|ui| {
                        pages::tab(ui, &mut self.habit_draft.schedule, HabitScheduleChoice::Daily, "毎日", palette);
                        pages::tab(ui, &mut self.habit_draft.schedule, HabitScheduleChoice::Weekdays, "曜日を指定", palette);
                    });
                    if self.habit_draft.schedule == HabitScheduleChoice::Weekdays {
                        ui.horizontal(|ui| { for (index, label) in ["月", "火", "水", "木", "金", "土", "日"].iter().enumerate() {
                            if components::button(ui, label, self.habit_draft.weekdays[index], palette).clicked() { self.habit_draft.weekdays[index] = !self.habit_draft.weekdays[index]; }
                        } });
                    }
                    ui.add_space(16.0); ui.label("所要時間");
                    ui.horizontal_wrapped(|ui| {
                        minutes_editor(ui, &mut self.habit_draft.duration_minutes); ui.label("分");
                        for minutes in [15, 30, 60, 90] { if components::button(ui, &format!("{minutes}分"), self.habit_draft.duration_minutes == minutes.to_string(), palette).clicked() { self.habit_draft.duration_minutes = minutes.to_string(); } }
                    });
                    ui.add_space(16.0);
                    ui.checkbox(&mut self.habit_draft.has_preferred_window, "配置する時間帯を指定");
                    if self.habit_draft.has_preferred_window {
                        ui.horizontal(|ui| {
                            ui.add_sized([95.0, 34.0], text_field(&mut self.habit_draft.preferred_start, "07:00")); ui.label("〜");
                            ui.add_sized([95.0, 34.0], text_field(&mut self.habit_draft.preferred_end, "09:00"));
                        });
                    }
                    ui.label(regular_text(format!("タイムゾーン · {}", self.timezone_offset)).size(11.0).color(palette.muted));
                });
                ui.add_space(16.0);
                ui.label(regular_text("保存した設定は、次の「計画を提案」から反映されます。固定済みの予定は維持されます。").size(11.0).color(palette.muted));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if components::button(ui, "キャンセル", false, palette).clicked() { self.workspace_ui.habit_create_open = false; }
                    if components::button(ui, "保存する", true, palette).clicked() { self.save_habit_form(); }
                });
            });
        });
    }

    pub(super) fn save_habit_form(&mut self) {
        if let Some(original) = self.workspace_ui.editing_habit.clone() {
            let result = (|| -> Result<()> {
                let request = self.habit_draft.request(local_user_id())?;
                let vault = self.vault_clone()?;
                self.runtime.block_on(async move {
                    let habits = vault.habit_repo();
                    let occurrences = vault.habit_occurrence_repo();
                    HabitService::new(habits.as_ref(), occurrences.as_ref())
                        .update(&original, request)
                        .await?;
                    Result::<()>::Ok(())
                })
            })();
            match result {
                Ok(()) => {
                    self.error = None;
                    self.auto_preview = None;
                    self.plan = None;
                    self.message = "習慣を保存しました".into();
                    self.refresh_habits();
                }
                Err(error) => self.set_error(error),
            }
        } else {
            self.add_habit();
        }
        if self.error.is_none() {
            self.workspace_ui.habit_create_open = false;
            self.workspace_ui.editing_habit = None;
        }
    }
}

fn habit_column(habit: &Habit) -> usize {
    if habit.flexibility == HabitFlexibility::Required {
        0
    } else if habit.preferred_window.is_some() {
        1
    } else {
        2
    }
}
