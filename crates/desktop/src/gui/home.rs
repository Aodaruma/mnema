use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskFilter {
    All,
    Today,
    Overdue,
    Done,
}

impl MnemaGuiApp {
    pub(super) fn show_shell(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let sidebar_width = if ui.available_width() < 1050.0 {
            184.0
        } else {
            216.0
        };
        egui::Panel::top("top_bar")
            .frame(
                egui::Frame::new()
                    .fill(palette.surface)
                    .inner_margin(egui::Margin::symmetric(20, 8)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        regular_text("マイワークスペース")
                            .size(12.0)
                            .color(palette.muted),
                    );
                    ui.label(regular_text("/").color(palette.border_strong));
                    ui.label(
                        regular_text(view_label(self.view))
                            .size(12.0)
                            .color(palette.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        theme_toggle(ui, &mut self.dark_mode, palette);
                        ui.push_id("background_indicator", |ui| {
                            if self.background.busy() {
                                ui.spinner();
                            }
                            if self.background.suggestion.is_some()
                                && components::button(ui, "新しい計画案", false, palette).clicked()
                            {
                                self.view = View::Settings;
                                self.workspace_ui.settings_section =
                                    pages::SettingsSection::Planning;
                            }
                            if let Some(error) = &self.background.error {
                                ui.label(
                                    regular_text("同期・計画を確認")
                                        .size(12.0)
                                        .color(palette.warning),
                                )
                                .on_hover_text(error);
                            }
                        });
                    });
                });
            });
        self.sync_native_menu_theme();

        egui::Panel::left("navigation")
            .resizable(false)
            .exact_size(sidebar_width)
            .frame(
                egui::Frame::new()
                    .fill(palette.surface)
                    .inner_margin(egui::Margin::symmetric(12, 18)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 9.0, palette.selected_fill);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "m",
                        egui::FontId::proportional(23.0),
                        palette.accent,
                    );
                    ui.label(logo_text("mnema").size(21.0).color(palette.brand));
                });
                ui.add_space(24.0);
                ScrollArea::vertical()
                    .id_salt("sidebar_scroll")
                    .max_height((ui.available_height() - 30.0).max(80.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        sidebar_heading(ui, "ワークスペース", palette);
                        self.sidebar_item(ui, View::Home, ICON_HOME, "ホーム", None, palette);
                        self.sidebar_item(
                            ui,
                            View::Activity,
                            ICON_HISTORY,
                            "受信トレイ",
                            None,
                            palette,
                        );

                        self.sidebar_item(
                            ui,
                            View::Inbox,
                            ICON_INBOX,
                            "タスク",
                            Some(self.tasks.len()),
                            palette,
                        );
                        self.sidebar_item(
                            ui,
                            View::Projects,
                            ICON_FOLDER,
                            "プロジェクト",
                            None,
                            palette,
                        );
                        ui.add_space(18.0);
                        sidebar_heading(ui, "プランニング", palette);
                        self.sidebar_item(
                            ui,
                            View::Schedule,
                            ICON_EVENT,
                            "スケジュール",
                            None,
                            palette,
                        );
                        self.sidebar_item(
                            ui,
                            View::Habits,
                            ICON_HABIT,
                            "習慣",
                            Some(self.habits.len()),
                            palette,
                        );
                        ui.add_space(18.0);
                        sidebar_heading(ui, "サポート", palette);
                        self.sidebar_item(
                            ui,
                            View::Assistant,
                            ICON_ASSISTANT,
                            "アシスタント",
                            None,
                            palette,
                        );
                        ui.add_space(18.0);
                        ui.separator();
                        self.sidebar_item(ui, View::Settings, ICON_SETTINGS, "設定", None, palette);
                    });
                ui.with_layout(egui::Layout::bottom_up(Align::Min), |ui| {
                    ui.label(
                        regular_text(match self.backend {
                            Some(StorageBackend::Sqlite) => "●  ローカルに接続済み",
                            Some(StorageBackend::Postgres) => "●  サーバーに接続済み",
                            None => "○  未接続",
                        })
                        .size(11.0)
                        .color(palette.muted),
                    );
                });
            });
        if self.view == View::Home && self.last_view != View::Home {
            self.scroll_home_agenda_to_now = true;
        }
        self.last_view = self.view;

        if self.error.is_some() || (!self.message.is_empty() && self.message != "Vault connected") {
            egui::Panel::bottom("status_bar")
                .frame(
                    egui::Frame::new()
                        .fill(palette.panel)
                        .inner_margin(egui::Margin::symmetric(24, 5)),
                )
                .show_inside(ui, |ui| {
                    ui.label(
                        regular_text(self.error.as_deref().unwrap_or(&self.message))
                            .size(11.0)
                            .color(if self.error.is_some() {
                                palette.error
                            } else {
                                palette.muted
                            }),
                    );
                });
        }
        // A status message may add/remove a sibling panel above this allocation
        // in the widget tree. Keep content IDs independent of that sibling index.
        ui.scope_builder(
            egui::UiBuilder::new().id(egui::Id::new("workspace_content")),
            |ui| {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::new()
                            .fill(palette.panel)
                            .inner_margin(egui::Margin::symmetric(24, 24)),
                    )
                    .show_inside(ui, |ui| {
                        if self.view == View::Home {
                            self.show_home(ui, palette);
                        } else if self.view == View::Schedule {
                            self.show_schedule(ui, palette);
                        } else {
                            ScrollArea::vertical()
                                .id_salt("page_scroll")
                                .show(ui, |ui| match self.view {
                                    View::Home => {}
                                    View::Inbox => self.show_inbox(ui, palette),
                                    View::Projects => self.show_projects(ui, palette),
                                    View::Schedule => self.show_schedule(ui, palette),
                                    View::Habits => self.show_habits(ui, palette),
                                    View::Assistant => self.show_assistant(ui, palette),
                                    View::Activity => self.show_activity(ui, palette),
                                    View::Settings => self.show_settings(ui, palette),
                                });
                        }
                    });
            },
        );
    }

    pub(super) fn sidebar_item(
        &mut self,
        ui: &mut egui::Ui,
        target: View,
        icon: char,
        label: &str,
        count: Option<usize>,
        palette: Palette,
    ) {
        let selected = self.view == target;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
        });
        let painter = ui.painter();
        if selected || response.hovered() {
            painter.rect_filled(
                rect,
                6.0,
                if selected {
                    palette.selected_fill
                } else {
                    palette.faint
                },
            );
        }
        let color = if selected {
            palette.selected_text
        } else {
            palette.muted
        };
        painter.text(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            icon,
            material_icon_font(18.0),
            color,
        );
        painter.text(
            egui::pos2(rect.left() + 37.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            if selected { palette.text } else { color },
        );
        if let Some(count) = count.filter(|count| *count > 0) {
            painter.text(
                egui::pos2(rect.right() - 10.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                count,
                egui::FontId::proportional(11.0),
                palette.muted,
            );
        }
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.view = target;
        }
    }

    pub(super) fn show_home(&mut self, ui: &mut egui::Ui, palette: Palette) {
        ui.label(
            regular_text(japanese_date(self.now_in_timezone().date()))
                .size(13.0)
                .color(palette.muted),
        );
        ui.label(
            regular_text(match self.now_in_timezone().hour() {
                5..=10 => "おはようございます",
                11..=17 => "こんにちは",
                _ => "おつかれさまです",
            })
            .size(30.0),
        );
        ui.add_space(22.0);
        let width = ui.available_width();
        if width >= 960.0 {
            let calendar_width = (width - 20.0) / 3.0;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(calendar_width, ui.available_height()),
                    egui::Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(calendar_width);
                        self.home_calendar(ui, palette);
                    },
                );
                ui.add_space(10.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(width - calendar_width - 20.0, ui.available_height()),
                    egui::Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(width - calendar_width - 20.0);
                        self.task_panel(ui, None, true, palette);
                    },
                );
            });
        } else {
            ScrollArea::vertical().id_salt("home_page").show(ui, |ui| {
                ui.allocate_ui(egui::vec2(width, 560.0), |ui| {
                    self.home_calendar(ui, palette)
                });
                ui.add_space(18.0);
                ui.allocate_ui(egui::vec2(width, 600.0), |ui| {
                    self.task_panel(ui, None, true, palette)
                });
            });
        }
    }

    pub(super) fn select_home_date(&mut self, date: Date) {
        self.workspace_ui.calendar.pan = [0.0; 2];
        self.target_date = date.to_string();
        self.auto_preview = None;
        self.plan = None;
        self.refresh_schedule();
        self.refresh_schedule_month();
        self.scroll_home_agenda_to_now = true;
    }

    pub(super) fn home_calendar(&mut self, ui: &mut egui::Ui, palette: Palette) {
        self.calendar_panel(ui, true, palette);
    }
}

pub(super) fn card(palette: Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0_f32, palette.border))
        .corner_radius(10.0)
        .inner_margin(20.0)
}

fn sidebar_heading(ui: &mut egui::Ui, label: &str, palette: Palette) {
    ui.label(regular_text(label).size(10.0).color(palette.muted));
    ui.add_space(2.0);
}

fn view_label(view: View) -> &'static str {
    match view {
        View::Home => "ホーム",
        View::Inbox => "タスク",
        View::Projects => "プロジェクト",
        View::Schedule => "スケジュール",
        View::Habits => "習慣",
        View::Assistant => "アシスタント",
        View::Activity => "受信トレイ",
        View::Settings => "設定",
    }
}

pub(super) fn japanese_date(date: Date) -> String {
    let weekday = ["月", "火", "水", "木", "金", "土", "日"]
        [date.weekday().number_days_from_monday() as usize];
    format!("{}月{}日  {}曜日", date.month() as u8, date.day(), weekday)
}
