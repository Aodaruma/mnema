use super::*;
use std::time::{Duration, Instant};

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub(super) enum SettingsSection {
    #[default]
    General,
    Planning,
    Connections,
    Assistant,
    Storage,
}

#[derive(Default)]
pub(super) struct WorkspaceUi {
    pub task_search: String,
    pub task_editor: Option<Task>,
    pub detail_task_id: Option<TaskId>,
    pub detail_title: String,
    pub detail_description: String,
    pub detail_title_editing: bool,
    pub detail_description_editing: bool,
    pub active_hovered_task: Option<TaskId>,
    pub capture_due: String,
    pub capture_minutes: String,
    pub capture_list: Option<ListId>,
    pub capture_expanded: bool,
    pub capture_status: Option<StatusId>,
    pub capture_commands: task_capture::CommandMenu,
    pub capture: task_capture::TaskCapture,
    pub task_grouping: tasks::TaskGrouping,
    pub applied_dark_mode: Option<bool>,
    pub calendar: calendar::CalendarUi,
    pub habit_view: habits::HabitView,
    pub habit_search: String,
    pub editing_habit: Option<Habit>,
    pub settings_section: SettingsSection,
    pub task_milestones: Vec<Milestone>,
    pub read_notifications: HashSet<AutomationLogId>,
    pub(super) selected_notification: Option<AutomationLogId>,
    pub(super) unread_only: bool,
    pub(super) project_overview: bool,
    pub(super) project_create_open: bool,
    pub(super) habit_create_open: bool,
    pub(super) last_refresh: Option<Instant>,
    pub(super) observed_view: Option<View>,
    pub(super) observed_settings_section: Option<SettingsSection>,
    pub(super) was_focused: bool,
}

impl MnemaGuiApp {
    pub(super) fn show_inbox(&mut self, ui: &mut egui::Ui, palette: Palette) {
        page_heading(ui, "マイタスク", palette);
        self.task_panel(ui, None, false, palette);
    }

    pub(super) fn show_projects(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let timezone = self.app_timezone();
        page_heading(ui, "プロジェクト", palette);
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("project_selection")
                .popup_style(components::menu_style.into())
                .selected_text(
                    self.selected_project_title()
                        .unwrap_or_else(|| "プロジェクトを選択".into()),
                )
                .width(240.0)
                .show_ui(ui, |ui| {
                    let projects = self.projects.clone();
                    for project in projects {
                        if ui
                            .selectable_label(
                                self.selected_project_id.as_ref() == Some(&project.id),
                                &project.title,
                            )
                            .clicked()
                        {
                            self.selected_project_id = Some(project.id);
                            self.refresh_project_children();
                        }
                    }
                });
            if ui.button("＋ プロジェクト").clicked() {
                self.workspace_ui.project_create_open = !self.workspace_ui.project_create_open;
            }
        });
        if self.workspace_ui.project_create_open {
            ui.add_space(12.0);
            home::card(palette).show(ui, |ui| {
                ui.label(bold_text("新しいプロジェクト"));
                ui.add_sized(
                    [ui.available_width().min(540.0), INPUT_HEIGHT],
                    text_field(&mut self.project_title, "プロジェクト名"),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label("開始");
                    date_editor(ui, &mut self.project_start_date, timezone, palette);
                    ui.label("終了");
                    date_editor(ui, &mut self.project_end_date, timezone, palette);
                    if ui.button("作成").clicked() {
                        self.add_project();
                        if self.error.is_none() {
                            self.workspace_ui.project_create_open = false;
                        }
                    }
                });
            });
        }
        ui.add_space(18.0);
        if let Some(project_id) = self.selected_project_id.clone() {
            ui.horizontal(|ui| {
                let tasks_selected = !self.workspace_ui.project_overview
                    && self.project_view_mode == ProjectViewMode::Details;
                if tab_button(ui, tasks_selected, "リスト", palette).clicked() {
                    self.workspace_ui.project_overview = false;
                    self.project_view_mode = ProjectViewMode::Details;
                }
                if tab_button(ui, self.workspace_ui.project_overview, "概要", palette).clicked() {
                    self.workspace_ui.project_overview = true;
                    self.project_view_mode = ProjectViewMode::Details;
                }
                if tab_button(
                    ui,
                    self.project_view_mode == ProjectViewMode::Gantt,
                    "タイムライン",
                    palette,
                )
                .clicked()
                {
                    self.project_view_mode = ProjectViewMode::Gantt;
                    self.workspace_ui.project_overview = false;
                }
            });
            ui.separator();
            ui.add_space(12.0);
            if self.project_view_mode == ProjectViewMode::Gantt {
                if let Some(id) = project_gantt_view(
                    ui,
                    &self.projects,
                    self.selected_project_id.as_ref(),
                    &self.milestones,
                    palette,
                ) {
                    self.selected_project_id = Some(id);
                    self.refresh_project_children();
                }
            } else if self.workspace_ui.project_overview {
                self.show_project_overview(ui, palette);
            } else {
                self.task_panel(ui, Some(project_id), false, palette);
            }
        } else {
            let projects = self.projects.clone();
            if projects.is_empty() {
                empty_state(
                    ui,
                    ICON_FOLDER,
                    "プロジェクトはまだありません",
                    "上の「＋ プロジェクト」から作成できます。",
                    palette,
                );
            }
            for project in projects {
                let task_count = self
                    .tasks
                    .iter()
                    .filter(|task| task.project_id.as_ref() == Some(&project.id))
                    .count();
                let response = home::card(palette)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(material_icon_text(ICON_FOLDER, 24.0, palette.accent));
                            ui.label(bold_text(&project.title).size(17.0));
                            ui.label(
                                regular_text(format!("{task_count}件のタスク"))
                                    .color(palette.muted),
                            );
                        });
                        ui.label(
                            regular_text(
                                project
                                    .end_date
                                    .map(|d| format!("期限 {d}"))
                                    .unwrap_or_else(|| "期限なし".into()),
                            )
                            .color(palette.muted),
                        );
                    })
                    .response
                    .interact(egui::Sense::click());
                if response.clicked() {
                    self.selected_project_id = Some(project.id);
                    self.refresh_project_children();
                }
                ui.add_space(12.0);
            }
        }
    }

    fn show_project_overview(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let timezone = self.app_timezone();
        let id = self.selected_project_id.as_ref();
        let total = self
            .tasks
            .iter()
            .chain(&self.done_tasks)
            .filter(|t| t.project_id.as_ref() == id)
            .count();
        let done = self
            .done_tasks
            .iter()
            .filter(|t| t.project_id.as_ref() == id)
            .count();
        home::card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(bold_text("進捗").size(17.0));
            ui.add(
                egui::ProgressBar::new(if total == 0 {
                    0.0
                } else {
                    done as f32 / total as f32
                })
                .text(format!("{done} / {total} 件完了"))
                .fill(palette.accent),
            );
        });
        ui.add_space(16.0);
        home::card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(bold_text("リスト").size(17.0));
            for list in &self.project_lists {
                ui.horizontal(|ui| {
                    ui.label(material_icon_text(ICON_INBOX, 17.0, palette.muted));
                    ui.label(&list.name);
                });
            }
            ui.horizontal(|ui| {
                ui.add_sized(
                    [ui.available_width().min(320.0) - 90.0, INPUT_HEIGHT],
                    text_field(&mut self.project_list_name, "リストを追加"),
                );
                if ui.button("追加").clicked() {
                    self.add_project_list();
                }
            });
        });
        ui.add_space(16.0);
        home::card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(bold_text("マイルストーン").size(17.0));
            for milestone in &self.milestones {
                ui.horizontal_wrapped(|ui| {
                    ui.label(material_icon_text('\u{e153}', 17.0, palette.accent));
                    ui.label(&milestone.title);
                    ui.label(regular_text(milestone.target_date.to_string()).color(palette.muted));
                    ui.label(milestone_status_label(&milestone.status));
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.add_sized(
                    [220.0, INPUT_HEIGHT],
                    text_field(&mut self.milestone_title, "マイルストーンを追加"),
                );
                date_editor(ui, &mut self.milestone_target_date, timezone, palette);
                if ui.button("追加").clicked() {
                    self.add_milestone();
                }
            });
        });
    }

    pub(super) fn show_schedule(&mut self, ui: &mut egui::Ui, palette: Palette) {
        self.calendar_panel(ui, false, palette);
    }

    pub(super) fn show_activity(&mut self, ui: &mut egui::Ui, palette: Palette) {
        page_heading(ui, "受信トレイ", palette);
        ui.horizontal_wrapped(|ui| {
            tab(
                ui,
                &mut self.workspace_ui.unread_only,
                false,
                "すべて",
                palette,
            );
            tab(
                ui,
                &mut self.workspace_ui.unread_only,
                true,
                "未読",
                palette,
            );
            if ui.button("すべて既読にする").clicked() {
                self.workspace_ui
                    .read_notifications
                    .extend(self.automation_logs.iter().map(|l| l.id.clone()));
                self.persist_ui_preferences();
            }
        });
        ui.separator();
        ui.add_space(14.0);
        let logs: Vec<_> = self
            .automation_logs
            .iter()
            .filter(|l| {
                !self.workspace_ui.unread_only
                    || !self.workspace_ui.read_notifications.contains(&l.id)
            })
            .cloned()
            .collect();
        if logs.is_empty() {
            empty_state(
                ui,
                ICON_INBOX,
                "新しい通知はありません",
                "タスク作成や自動処理の結果がここに表示されます。",
                palette,
            );
            return;
        }
        let width = ui.available_width();
        if width >= 900.0 {
            ui.columns(2, |columns| {
                self.notification_list(&mut columns[0], &logs, palette);
                self.notification_detail(&mut columns[1], palette);
            });
        } else {
            self.notification_list(ui, &logs, palette);
            ui.add_space(16.0);
            self.notification_detail(ui, palette);
        }
    }

    fn notification_list(&mut self, ui: &mut egui::Ui, logs: &[AutomationLog], palette: Palette) {
        ScrollArea::vertical()
            .id_salt("notification_list")
            .max_height(620.0)
            .show(ui, |ui| {
                for log in logs {
                    let selected =
                        self.workspace_ui.selected_notification.as_ref() == Some(&log.id);
                    let unread = !self.workspace_ui.read_notifications.contains(&log.id);
                    let title = automation_log_title(log).unwrap_or_else(|| "自動処理".into());
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 78.0),
                        egui::Sense::click(),
                    );
                    let painter = ui.painter_at(rect);
                    let hover =
                        ui.ctx()
                            .animate_bool_with_time(response.id, response.hovered(), 0.12);
                    painter.rect_filled(
                        rect,
                        6.0,
                        if selected {
                            palette.selected_fill
                        } else {
                            palette.faint.gamma_multiply(hover)
                        },
                    );
                    let avatar = rect.left_center() + egui::vec2(28.0, 0.0);
                    painter.circle_filled(avatar, 18.0, palette.selected_fill);
                    painter.text(
                        avatar,
                        egui::Align2::CENTER_CENTER,
                        ICON_CHECK,
                        material_icon_font(18.0),
                        palette.accent,
                    );
                    if unread {
                        painter.circle_filled(
                            rect.left_center() + egui::vec2(4.0, 0.0),
                            3.0,
                            palette.accent,
                        );
                    }
                    let content = egui::Rect::from_min_max(
                        rect.min + egui::vec2(58.0, 0.0),
                        rect.max - egui::vec2(66.0, 0.0),
                    );
                    let text_painter = painter.with_clip_rect(content);
                    text_painter.text(
                        content.left_top() + egui::vec2(0.0, 23.0),
                        egui::Align2::LEFT_CENTER,
                        &title,
                        egui::FontId::proportional(14.0),
                        palette.text,
                    );
                    text_painter.text(
                        content.left_top() + egui::vec2(0.0, 49.0),
                        egui::Align2::LEFT_CENTER,
                        automation_action_label(&log.action_type),
                        egui::FontId::proportional(12.0),
                        palette.muted,
                    );
                    painter.text(
                        rect.right_top() + egui::vec2(-10.0, 23.0),
                        egui::Align2::RIGHT_CENTER,
                        format_hm_in(log.created_at, self.app_timezone()),
                        egui::FontId::proportional(11.0),
                        palette.muted,
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title)
                    });
                    if response.on_hover_text(&title).clicked() {
                        self.workspace_ui.selected_notification = Some(log.id.clone());
                        self.workspace_ui.read_notifications.insert(log.id.clone());
                        self.persist_ui_preferences();
                    }
                    ui.separator();
                }
            });
    }

    fn notification_detail(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let selected = self
            .workspace_ui
            .selected_notification
            .as_ref()
            .and_then(|id| self.automation_logs.iter().find(|log| &log.id == id))
            .cloned();
        if let Some(log) = selected {
            home::card(palette).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(bold_text(automation_action_label(&log.action_type)).size(19.0));
                ui.label(
                    regular_text(format!(
                        "{} {}",
                        home::japanese_date(log.created_at.to_offset(self.app_timezone()).date()),
                        format_hm_in(log.created_at, self.app_timezone())
                    ))
                    .color(palette.muted),
                );
                ui.add_space(18.0);
                if let Some(title) = automation_log_title(&log) {
                    ui.label(bold_text(title));
                }
                if let Some(explanation) = &log.explanation {
                    ui.label(explanation);
                }
                if let Some(task) = self
                    .tasks
                    .iter()
                    .chain(&self.done_tasks)
                    .find(|t| Some(&t.id) == log.task_id.as_ref())
                    .cloned()
                {
                    ui.add_space(18.0);
                    if ui.button("タスクを開く").clicked() {
                        self.workspace_ui.task_editor = Some(task);
                    }
                }
                if ui.small_button("未読に戻す").clicked() {
                    self.workspace_ui.read_notifications.remove(&log.id);
                    self.persist_ui_preferences();
                }
            });
        } else {
            empty_state(
                ui,
                ICON_INBOX,
                "通知を選択",
                "左の一覧から内容を確認できます。",
                palette,
            );
        }
    }

    pub(super) fn show_assistant(&mut self, ui: &mut egui::Ui, palette: Palette) {
        page_heading(ui, "アシスタント", palette);
        ui.horizontal(|ui| {
            ui.label(
                regular_text(assistant_provider_label(self.llm_provider))
                    .size(12.0)
                    .color(palette.muted),
            );
            if tasks::icon_button(ui, ICON_SETTINGS, "アシスタント設定", palette).clicked()
            {
                self.view = View::Settings;
                self.workspace_ui.settings_section = SettingsSection::Assistant;
            }
        });
        ui.add_space(12.0);
        let width = ui.available_width().min(900.0);
        ui.vertical_centered(|ui| {
            ui.set_width(width);
            ScrollArea::vertical()
                .id_salt("assistant_messages")
                .max_height(500.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for message in &self.assistant_messages {
                        let mine = message.role == "You";
                        ui.with_layout(
                            if mine {
                                egui::Layout::top_down(Align::Max)
                            } else {
                                egui::Layout::top_down(Align::Min)
                            },
                            |ui| {
                                egui::Frame::new()
                                    .fill(if mine {
                                        palette.selected_fill
                                    } else {
                                        palette.surface
                                    })
                                    .corner_radius(12.0)
                                    .inner_margin(16.0)
                                    .show(ui, |ui| {
                                        ui.set_max_width(width * 0.84);
                                        ui.label(
                                            regular_text(if mine { "あなた" } else { "Mnema" })
                                                .size(11.0)
                                                .color(palette.muted),
                                        );
                                        ui.add_space(6.0);
                                        ui.label(&message.content);
                                    });
                                ui.add_space(14.0);
                            },
                        );
                    }
                });
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                for prompt in ["次に何をすればいい？", "今日のタスクを確認したい"]
                {
                    if ui.small_button(prompt).clicked() {
                        self.assistant_input = prompt.into();
                    }
                }
            });
            let mut send = false;
            home::card(palette).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let input = ui.add_sized(
                    [ui.available_width(), 65.0],
                    TextEdit::multiline(&mut self.assistant_input)
                        .frame(egui::Frame::NONE)
                        .hint_text("メッセージを入力…"),
                );
                send = input.has_focus()
                    && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    ui.label(
                        regular_text("Ctrl + Enter で送信")
                            .size(11.0)
                            .color(palette.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !self.assistant_input.trim().is_empty(),
                                egui::Button::new("送信").fill(palette.selected_fill),
                            )
                            .clicked()
                        {
                            send = true;
                        }
                    });
                });
            });
            if send {
                self.send_assistant_message();
            }
        });
    }

    pub(super) fn show_settings(&mut self, ui: &mut egui::Ui, palette: Palette) {
        page_heading(ui, "設定", palette);
        ui.horizontal_wrapped(|ui| {
            for (section, label) in [
                (SettingsSection::General, "外観"),
                (SettingsSection::Planning, "時間・計画"),
                (SettingsSection::Connections, "カレンダー連携"),
                (SettingsSection::Assistant, "アシスタント"),
                (SettingsSection::Storage, "データ・接続"),
            ] {
                tab(
                    ui,
                    &mut self.workspace_ui.settings_section,
                    section,
                    label,
                    palette,
                );
            }
        });
        ui.separator();
        ui.add_space(20.0);
        let section = self.workspace_ui.settings_section;
        if section == SettingsSection::Connections {
            self.show_calendar(ui, palette);
            return;
        }
        let max_width = ui
            .available_width()
            .min(if section == SettingsSection::Planning {
                1200.0
            } else {
                840.0
            });
        ui.vertical(|ui| {
            ui.set_width(max_width);
            match section {
                SettingsSection::General => {
                    settings_card(ui, "表示", palette, |ui| {
                        setting_label(
                            ui,
                            "カラーテーマ",
                            "ライト・ダークを切り替えます。",
                            palette,
                        );
                        theme_toggle(ui, &mut self.dark_mode, palette);
                        ui.add_space(18.0);
                        setting_label(ui, "タスクの行間", "一覧の情報量を調整します。", palette);
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.home_task_density,
                                TaskListDensity::Normal,
                                "標準",
                            );
                            ui.selectable_value(
                                &mut self.home_task_density,
                                TaskListDensity::Compact,
                                "コンパクト",
                            );
                        });
                        ui.add_space(18.0);
                        setting_label(ui, "並び順", "タスク一覧の既定の並び順です。", palette);
                        egui::ComboBox::from_id_salt("settings_sort")
                            .popup_style(components::menu_style.into())
                            .selected_text(self.home_task_sort.label())
                            .show_ui(ui, |ui| {
                                for mode in [
                                    TaskSortMode::DueDate,
                                    TaskSortMode::Estimate,
                                    TaskSortMode::Importance,
                                    TaskSortMode::Created,
                                ] {
                                    ui.selectable_value(
                                        &mut self.home_task_sort,
                                        mode,
                                        mode.label(),
                                    );
                                }
                            });
                    });
                }
                SettingsSection::Planning => {
                    settings_card(ui, "自動同期・提案", palette, |ui| { self.show_background_controls(ui, palette); });
                    settings_card(ui, "タイムゾーン", palette, |ui| {
                        let mut mode = self.timezone_mode;
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut mode, TimezoneMode::Automatic, "OSに合わせる");
                            ui.selectable_value(&mut mode, TimezoneMode::Manual, "手動で指定");
                        });
                        if mode != self.timezone_mode { self.select_timezone_mode(mode); }
                        ui.add_space(12.0);
                        if mode == TimezoneMode::Automatic {
                            ui.label(bold_text(self.detected_timezone.as_deref().unwrap_or(&self.timezone_offset)).size(16.0));
                            ui.label(regular_text("このPCのタイムゾーンを使用します。OS側の変更も自動で反映します。").size(12.0).color(palette.muted));
                            if let Some(error) = &self.timezone_error {
                                ui.colored_label(palette.warning, format!("{error} 現在は {} を使用しています。", self.timezone_offset));
                            }
                        } else {
                            ui.add_sized([ui.available_width().min(460.0), INPUT_HEIGHT], text_field(&mut self.timezone_offset, "Asia/Tokyo"));
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.small_button("日本標準時").clicked() { self.timezone_offset = "Asia/Tokyo".into(); }
                                if ui.small_button("UTC").clicked() { self.timezone_offset = "UTC".into(); }
                            });
                            ui.label(regular_text("地域名で指定します。例: Asia/Tokyo、America/New_York").size(12.0).color(palette.muted));
                        }
                    });
                    ui.add_space(16.0);
                    settings_card(ui, "計画できる時間", palette, |ui| {
                        let columns = if ui.available_width() >= 980.0 { 3 } else if ui.available_width() >= 580.0 { 2 } else { 1 };
                        for first in (0..3).step_by(columns) {
                            ui.columns(columns, |uis| {
                                for (column, ui) in uis.iter_mut().enumerate() {
                                    let index = first + column;
                                    if index >= 3 { break; }
                                    ui.push_id(("planning_time", index), |ui| {
                                        egui::Frame::new().fill(palette.faint).corner_radius(8).inner_margin(16).show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.set_min_height(116.0);
                                            match index {
                                                0 => {
                                                    setting_label(ui, "作業時間", "この時間帯にタスクを配置します。", palette);
                                                    time_pair(ui, &mut self.availability_start, &mut self.availability_end);
                                                }
                                                1 => {
                                                    setting_label(ui, "睡眠時間", "この時間にはタスクを配置しません。", palette);
                                                    time_pair(ui, &mut self.sleep_start, &mut self.sleep_end);
                                                }
                                                _ => {
                                                    setting_label(ui, "移動の余裕", "移動を伴う予定の前後に確保する時間です。", palette);
                                                    ui.horizontal(|ui| minutes_editor(ui, &mut self.travel_buffer_minutes));
                                                }
                                            }
                                        });
                                    });
                                }
                            });
                            if first + columns < 3 { ui.add_space(12.0); }
                        }
                    });
                }
                SettingsSection::Assistant => {
                    settings_card(ui, "AIの接続先", palette, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(
                                &mut self.llm_provider,
                                DesktopLlmProvider::Disabled,
                                "オフ",
                            );
                            ui.selectable_value(
                                &mut self.llm_provider,
                                DesktopLlmProvider::Ollama,
                                "Ollama",
                            );
                            ui.selectable_value(
                                &mut self.llm_provider,
                                DesktopLlmProvider::OpenAiCompatible,
                                "OpenAI互換",
                            );
                        });
                        ui.add_space(16.0);
                        if self.llm_provider != DesktopLlmProvider::Disabled {
                            ui.label("接続URL");
                            let url = if self.llm_provider == DesktopLlmProvider::Ollama {
                                &mut self.ollama_url
                            } else {
                                &mut self.openai_url
                            };
                            ui.add_sized(
                                [ui.available_width(), INPUT_HEIGHT],
                                text_field(url, "接続URL"),
                            );
                            ui.add_space(14.0);
                            ui.label("計画用モデル");
                            ui.add_sized(
                                [ui.available_width(), INPUT_HEIGHT],
                                text_field(&mut self.planning_model, "モデル名"),
                            );
                            ui.label("通常の会話用モデル");
                            ui.add_sized(
                                [ui.available_width(), INPUT_HEIGHT],
                                text_field(&mut self.routine_model, "モデル名"),
                            );
                        }
                    });
                }
                SettingsSection::Storage => {
                    settings_card(ui, "保存先", palette, |ui| {
                        ui.label("Vault");
                        ui.add_sized(
                            [ui.available_width(), INPUT_HEIGHT],
                            text_field(&mut self.vault_path, "Vaultのパス"),
                        );
                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.selected_backend,
                                DesktopStorageBackend::Sqlite,
                                "ローカル（SQLite）",
                            );
                            ui.selectable_value(
                                &mut self.selected_backend,
                                DesktopStorageBackend::Postgres,
                                "PostgreSQL",
                            );
                        });
                        ui.add_space(12.0);
                        if self.selected_backend == DesktopStorageBackend::Sqlite {
                            ui.label("データベース");
                            ui.add_sized(
                                [ui.available_width(), INPUT_HEIGHT],
                                text_field(&mut self.sqlite_path, "データベースのパス"),
                            );
                            if ui.small_button("既定の場所に戻す").clicked() {
                                self.sqlite_path = default_sqlite_path().display().to_string();
                            }
                        } else {
                            ui.label("接続URL");
                            ui.add_sized(
                                [ui.available_width(), INPUT_HEIGHT],
                                text_field(&mut self.database_url, DEFAULT_POSTGRES_URL)
                                    .password(true),
                            );
                        }
                        ui.add_space(14.0);
                        ui.label(
                            regular_text(format!(
                                "現在: {}",
                                self.backend
                                    .map(|b| match b {
                                        StorageBackend::Sqlite => "ローカルに接続済み",
                                        StorageBackend::Postgres => "サーバーに接続済み",
                                    })
                                    .unwrap_or("未接続")
                            ))
                            .color(palette.muted),
                        );
                    });
                }
                SettingsSection::Connections => {}
            }
            ui.add_space(20.0);
            if ui
                .add(
                    egui::Button::new(if section == SettingsSection::Storage {
                        "保存して接続"
                    } else {
                        "変更を保存"
                    })
                    .fill(palette.selected_fill),
                )
                .clicked()
            {
                self.save_settings();
                if self.error.is_none() && section == SettingsSection::Storage {
                    self.connect_and_refresh();
                }
            }
            if !self.settings_message.is_empty() {
                ui.label(regular_text(&self.settings_message).color(palette.success));
            }
        });
    }

    pub(super) fn persist_ui_preferences(&mut self) {
        let mut config = DesktopConfig::load(self.normalized_vault_path(), self.dark_mode);
        config.dark_mode = self.dark_mode;
        config.background_enabled = self.background_enabled;
        config.home_task_density = self.home_task_density;
        config.home_task_sort = self.home_task_sort;
        config.task_grouping = self.workspace_ui.task_grouping;
        config.read_notifications = self
            .workspace_ui
            .read_notifications
            .iter()
            .cloned()
            .collect();
        if let Err(error) = save_config(&config) {
            self.set_error(error);
        }
    }

    pub(super) fn automatic_refresh(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.focused);
        let changed = self.workspace_ui.observed_view != Some(self.view);
        let section_changed = self.view == View::Settings
            && self.workspace_ui.observed_settings_section
                != Some(self.workspace_ui.settings_section);
        let regained = focused && !self.workspace_ui.was_focused;
        self.workspace_ui.was_focused = focused;
        ctx.request_repaint_after(Duration::from_secs(15));
        if self.vault.is_none()
            || !focused
            || ctx.egui_wants_keyboard_input()
            || egui::Popup::is_any_open(ctx)
            || egui::DragAndDrop::has_any_payload(ctx)
            || self.workspace_ui.task_editor.is_some()
            || self.workspace_ui.capture.open
            || self.workspace_ui.habit_create_open
            || self.editing_schedule_block_id.is_some()
        {
            return;
        }
        let elapsed = self
            .workspace_ui
            .last_refresh
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(15));
        if !(changed || section_changed || regained || elapsed) {
            return;
        }
        self.workspace_ui.observed_view = Some(self.view);
        self.workspace_ui.observed_settings_section = Some(self.workspace_ui.settings_section);
        self.workspace_ui.last_refresh = Some(Instant::now());
        // Check the OS outside active edits. Do not apply an unsaved mode selection.
        if self.view == View::Settings
            && self.workspace_ui.settings_section == SettingsSection::Planning
        {
            if changed || section_changed || regained {
                self.refresh_system_timezone(false);
            }
        } else if self.timezone_mode == TimezoneMode::Automatic
            && self.applied_timezone_mode == TimezoneMode::Automatic
            && self.auto_preview.is_none()
            && self.plan.is_none()
        {
            self.refresh_system_timezone(true);
        }
        // Do not replace a proposal or a form that the user is editing.
        if self.auto_preview.is_none() && self.plan.is_none() && self.error.is_none() {
            self.refresh_tasks();
            self.refresh_projects();
            self.refresh_task_context();
            self.refresh_schedule();
            self.refresh_schedule_month();
            self.refresh_automation_logs();
        }
        if self.view == View::Habits {
            self.refresh_habits();
        }
        if self.view == View::Settings
            && self.workspace_ui.settings_section == SettingsSection::Connections
            && (changed || section_changed || regained || elapsed)
        {
            self.refresh_calendar_accounts();
        }
    }
}

pub(super) fn page_heading(ui: &mut egui::Ui, title: &str, palette: Palette) {
    ui.label(bold_text(title).size(27.0).color(palette.text));
    ui.add_space(18.0);
}
pub(super) fn tab<T: PartialEq>(
    ui: &mut egui::Ui,
    value: &mut T,
    choice: T,
    label: &str,
    palette: Palette,
) {
    if tab_button(ui, *value == choice, label, palette).clicked() {
        *value = choice;
    }
}
fn tab_button(ui: &mut egui::Ui, selected: bool, label: &str, palette: Palette) -> egui::Response {
    let response = components::button(ui, label, false, palette);
    if selected {
        ui.painter().hline(
            response.rect.x_range(),
            response.rect.bottom() + 3.0,
            Stroke::new(2.0_f32, palette.accent),
        );
    }
    response
}
pub(super) fn empty_state(
    ui: &mut egui::Ui,
    icon: char,
    title: &str,
    description: &str,
    palette: Palette,
) {
    ui.add_space(32.0);
    ui.vertical_centered(|ui| {
        ui.label(material_icon_text(icon, 34.0, palette.muted));
        ui.add_space(12.0);
        ui.label(bold_text(title).size(16.0));
        ui.add_space(5.0);
        ui.label(regular_text(description).size(12.0).color(palette.muted));
    });
    ui.add_space(32.0);
}
fn settings_card(
    ui: &mut egui::Ui,
    title: &str,
    palette: Palette,
    body: impl FnOnce(&mut egui::Ui),
) {
    home::card(palette).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(bold_text(title).size(18.0));
        ui.add_space(18.0);
        body(ui);
    });
}
fn setting_label(ui: &mut egui::Ui, title: &str, description: &str, palette: Palette) {
    ui.label(bold_text(title).size(14.0));
    ui.label(regular_text(description).size(12.0).color(palette.muted));
    ui.add_space(6.0);
}
fn time_pair(ui: &mut egui::Ui, start: &mut String, end: &mut String) {
    ui.horizontal(|ui| {
        let separator_width = ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(
                    "〜".into(),
                    egui::TextStyle::Body.resolve(ui.style()),
                    ui.visuals().text_color(),
                )
                .size()
                .x
        });
        let width = ((ui.available_width() - separator_width - 2.0 * ui.spacing().item_spacing.x)
            / 2.0)
            .clamp(64.0, 135.0);
        ui.add_sized([width, INPUT_HEIGHT], text_field(start, "09:00"));
        ui.label("〜");
        ui.add_sized([width, INPUT_HEIGHT], text_field(end, "17:00"));
    });
}
