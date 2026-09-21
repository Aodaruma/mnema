use super::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TaskGrouping {
    #[default]
    Schedule,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DateGroup {
    Today,
    Overdue,
    Next,
    Unscheduled,
}

pub(super) fn date_group(
    task: &Task,
    today: Date,
    blocks: &[ScheduleBlock],
    timezone: &str,
) -> DateGroup {
    if task.due_date.is_some_and(|date| date < today) {
        return DateGroup::Overdue;
    }
    let today_window =
        mnema_app::iana_date_range(today, today.next_day().unwrap_or(today), timezone).ok();
    let scheduled_today = today_window.is_some_and(|window| {
        blocks.iter().any(|block| {
            block.task_id.as_ref() == Some(&task.id)
                && !matches!(
                    block.state,
                    ScheduleBlockState::Cancelled
                        | ScheduleBlockState::Done
                        | ScheduleBlockState::Missed
                )
                && block.start_at < window.end
                && block.end_at > window.start
        })
    });
    if task.due_date == Some(today) || scheduled_today {
        return DateGroup::Today;
    }
    if task.due_date.is_some_and(|date| date > today)
        || blocks.iter().any(|block| {
            block.task_id.as_ref() == Some(&task.id)
                && !matches!(
                    block.state,
                    ScheduleBlockState::Cancelled
                        | ScheduleBlockState::Done
                        | ScheduleBlockState::Missed
                )
                && today_window.is_some_and(|window| block.start_at >= window.end)
        })
    {
        DateGroup::Next
    } else {
        DateGroup::Unscheduled
    }
}

impl MnemaGuiApp {
    pub(super) fn task_panel(
        &mut self,
        ui: &mut egui::Ui,
        project: Option<ProjectId>,
        home: bool,
        palette: Palette,
    ) {
        let frame = if home {
            home::card(palette)
        } else {
            egui::Frame::NONE
        };
        let panel_height = ui.available_height();
        frame.show(ui, |ui| {
            if home {
                ui.set_min_height((panel_height - 42.0).max(240.0));
            }
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(bold_text(if home { "マイタスク" } else { "タスク" }).size(18.0));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let settings =
                        components::icon_button(ui, ICON_SETTINGS, "表示・並び順", palette);
                    components::popup(&settings).show(|ui| {
                        ui.set_min_width(210.0);
                        ui.label(bold_text("グループ化"));
                        ui.selectable_value(
                            &mut self.workspace_ui.task_grouping,
                            TaskGrouping::Schedule,
                            "日付・予定別",
                        );
                        ui.selectable_value(
                            &mut self.workspace_ui.task_grouping,
                            TaskGrouping::Status,
                            "ステータス別",
                        );
                        ui.separator();
                        ui.label("行間・並び順");
                        ui.selectable_value(
                            &mut self.home_task_density,
                            TaskListDensity::Normal,
                            "標準の行間",
                        );
                        ui.selectable_value(
                            &mut self.home_task_density,
                            TaskListDensity::Compact,
                            "コンパクト",
                        );
                        ui.separator();
                        for mode in [
                            TaskSortMode::DueDate,
                            TaskSortMode::Estimate,
                            TaskSortMode::Importance,
                            TaskSortMode::Created,
                        ] {
                            ui.selectable_value(&mut self.home_task_sort, mode, mode.label());
                        }
                    });
                    ui.add_sized(
                        [ui.available_width().min(190.0), 26.0],
                        text_field(&mut self.workspace_ui.task_search, "タスクを検索"),
                    );
                });
            });
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                for (filter, label) in [
                    (home::TaskFilter::All, "すべて"),
                    (home::TaskFilter::Today, "今日"),
                    (home::TaskFilter::Overdue, "期限超過"),
                    (home::TaskFilter::Done, "完了"),
                ] {
                    pages::tab(ui, &mut self.home_filter, filter, label, palette);
                }
            });
            ui.separator();
            self.task_composer(ui, project.clone(), palette);
            ui.add_space(12.0);
            let today = self.now_in_timezone().date();
            let source = if self.home_filter == home::TaskFilter::Done {
                &self.done_tasks
            } else {
                &self.tasks
            };
            let query = self.workspace_ui.task_search.to_lowercase();
            let tasks: Vec<_> = sorted_tasks(source, self.home_task_sort)
                .into_iter()
                .filter(|task| {
                    project
                        .as_ref()
                        .is_none_or(|id| task.project_id.as_ref() == Some(id))
                        && (query.is_empty()
                            || task.title.to_lowercase().contains(&query)
                            || task_context_line(task, &self.lists, &self.projects)
                                .to_lowercase()
                                .contains(&query))
                        && match self.home_filter {
                            home::TaskFilter::Today => {
                                date_group(
                                    task,
                                    today,
                                    &self.workspace_ui.calendar.future_blocks,
                                    &self.timezone_offset,
                                ) == DateGroup::Today
                            }
                            home::TaskFilter::Overdue => {
                                task.due_date.is_some_and(|date| date < today)
                            }
                            _ => true,
                        }
                })
                .collect();
            let mut action = None;
            ScrollArea::vertical()
                .id_salt(("task_panel", home, project.clone()))
                .max_height(ui.available_height().max(120.0))
                .auto_shrink([false, !home])
                .show(ui, |ui| {
                    if tasks.is_empty() {
                        pages::empty_state(
                            ui,
                            ICON_CHECK,
                            "表示するタスクはありません",
                            "上の入力欄からタスクを追加できます。",
                            palette,
                        );
                    }
                    let schedule_grouping = self.workspace_ui.task_grouping
                        == TaskGrouping::Schedule
                        && self.home_filter != home::TaskFilter::Done;
                    let groups = if schedule_grouping {
                        vec![
                            ("今日", palette.accent),
                            ("期日超過", palette.error),
                            ("次", palette.success),
                            ("スケジュール未設定", palette.muted),
                        ]
                    } else {
                        vec![
                            ("進行中", palette.accent),
                            ("未着手", palette.muted),
                            ("保留", palette.warning),
                            ("完了", palette.success),
                        ]
                    };
                    for (index, (label, color)) in groups.into_iter().enumerate() {
                        let group: Vec<_> = tasks
                            .iter()
                            .filter(|task| {
                                if schedule_grouping {
                                    date_group(
                                        task,
                                        today,
                                        &self.workspace_ui.calendar.future_blocks,
                                        &self.timezone_offset,
                                    ) == [
                                        DateGroup::Today,
                                        DateGroup::Overdue,
                                        DateGroup::Next,
                                        DateGroup::Unscheduled,
                                    ][index]
                                } else {
                                    task_kind(task, &self.statuses, &self.status_groups)
                                        == [
                                            StatusGroupKind::InProgress,
                                            StatusGroupKind::NotStarted,
                                            StatusGroupKind::Pending,
                                            StatusGroupKind::Done,
                                        ][index]
                                }
                            })
                            .collect();
                        if group.is_empty() && !schedule_grouping {
                            continue;
                        }
                        egui::CollapsingHeader::new(
                            regular_text(format!("{label}  {}", group.len()))
                                .size(12.0)
                                .color(color),
                        )
                        .id_salt(("task_group", home, project.clone(), label))
                        .default_open(true)
                        .show_unindented(ui, |ui| {
                            for task in &group {
                                self.workspace_task_row(ui, task, palette, &mut action);
                            }
                            if group.is_empty() {
                                ui.label(
                                    regular_text("タスクはありません")
                                        .size(11.0)
                                        .color(palette.muted),
                                );
                            }
                        });
                        ui.add_space(12.0);
                    }
                });
            if let Some(action) = action {
                self.handle_task_action(action);
            }
        });
    }

    pub(super) fn workspace_task_row(
        &mut self,
        ui: &mut egui::Ui,
        task: &Task,
        palette: Palette,
        action: &mut Option<TaskAction>,
    ) {
        // Keep the outer layout slot stable while rows are removed. The global
        // task scope keeps child IDs/focus stable when a different task moves up.
        ui.scope(|ui| {
            ui.scope_builder(
                egui::UiBuilder::new().id(egui::Id::new(("workspace_task_row", &task.id))),
                |ui| {
                    let height = if self.home_task_density == TaskListDensity::Compact {
                        38.0
                    } else {
                        46.0
                    };
                    let width = ui.available_width();
                    let rect = egui::Rect::from_min_size(
                        ui.next_widget_position(),
                        egui::vec2(width, height),
                    );
                    let id = ui.make_persistent_id(("workspace_task", task.id.clone()));
                    let pointer_hovered = ui.rect_contains_pointer(rect);
                    if pointer_hovered {
                        self.workspace_ui.active_hovered_task = Some(task.id.clone());
                    }
                    let hovered = pointer_hovered
                        || (egui::Popup::is_any_open(ui.ctx())
                            && self.workspace_ui.active_hovered_task.as_ref() == Some(&task.id));
                    let hover = ui
                        .ctx()
                        .animate_bool_with_time(id.with("hover"), hovered, 0.12);
                    ui.painter()
                        .rect_filled(rect, 5.0, palette.faint.gamma_multiply(hover));
                    let milestone = self
                        .workspace_ui
                        .task_milestones
                        .iter()
                        .find(|m| task.milestone_id.as_ref() == Some(&m.id))
                        .filter(|_| width > 850.0)
                        .map(|m| m.title.clone());
                    let right_width = 241.0 + if milestone.is_some() { 82.0 } else { 0.0 };
                    let mut edited = task.clone();
                    let mut changed = false;
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, height),
                        egui::Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let (handle, drag) =
                                ui.allocate_exact_size(egui::vec2(14.0, 30.0), egui::Sense::drag());
                            for x in [-2.0, 2.0] {
                                for y in [-4.0, 0.0, 4.0] {
                                    ui.painter().circle_filled(
                                        handle.center() + egui::vec2(x, y),
                                        1.0,
                                        palette.muted.gamma_multiply(hover),
                                    );
                                }
                            }
                            drag.on_hover_cursor(egui::CursorIcon::Grab)
                                .dnd_set_drag_payload(TaskDragPayload {
                                    task_id: task.id.clone(),
                                    title: task.title.clone(),
                                    estimated_minutes: task.estimated_minutes,
                                });
                            task_status_button(
                                ui,
                                "workspace_tasks",
                                task,
                                &self.statuses,
                                &self.status_groups,
                                palette,
                                action,
                            );
                            let text_width = (width - right_width - 64.0).max(80.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(text_width, height),
                                egui::Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_min_width(text_width);
                                    let context_width = if text_width > 330.0 {
                                        (text_width * 0.38).min(240.0)
                                    } else {
                                        0.0
                                    };
                                    let title = row_text(
                                        ui,
                                        &task.title,
                                        text_width - context_width,
                                        14.0,
                                        palette.text,
                                        egui::Sense::click_and_drag(),
                                    );
                                    title.dnd_set_drag_payload(TaskDragPayload {
                                        task_id: task.id.clone(),
                                        title: task.title.clone(),
                                        estimated_minutes: task.estimated_minutes,
                                    });
                                    if title.clicked() {
                                        self.workspace_ui.task_editor = Some(task.clone());
                                    }
                                    title
                                        .on_hover_text(&task.title)
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if context_width > 0.0 {
                                        let context =
                                            task_context_line(task, &self.lists, &self.projects);
                                        let response = row_text(
                                            ui,
                                            &format!("· {context}"),
                                            context_width - 4.0,
                                            11.0,
                                            palette.muted,
                                            egui::Sense::click(),
                                        )
                                        .on_hover_text("所属を変更");
                                        components::popup(&response).show(|ui| {
                                            changed |= self.task_location_menu(ui, &mut edited);
                                        });
                                    }
                                },
                            );
                            ui.allocate_ui_with_layout(
                                egui::vec2(right_width, height),
                                egui::Layout::right_to_left(Align::Center),
                                |ui| {
                                    ui.set_min_width(right_width);
                                    let done =
                                        status_candidates(&self.statuses, task.project_id.as_ref())
                                            .into_iter()
                                            .find(|status| {
                                                status_group_kind(status, &self.status_groups)
                                                    == Some(StatusGroupKind::Done)
                                            })
                                            .filter(|_| {
                                                task_kind(task, &self.statuses, &self.status_groups)
                                                    != StatusGroupKind::Done
                                            })
                                            .map(|status| status.id.clone());
                                    // Keep the widget tree and allocated space identical on hover-in/out.
                                    ui.scope(|ui| {
                                        if !hovered || done.is_none() {
                                            ui.set_invisible();
                                        }
                                        if components::icon_button(
                                            ui,
                                            ICON_CHECK,
                                            "完了にする",
                                            palette,
                                        )
                                        .clicked()
                                            && let Some(done) = done
                                        {
                                            *action =
                                                Some(TaskAction::SetStatus(task.id.clone(), done));
                                        }
                                    });
                                    let more = components::icon_button(
                                        ui,
                                        '\u{e5d3}',
                                        "タスクの操作",
                                        palette,
                                    );
                                    components::popup(&more).show(|ui| {
                                        components::menu(ui, "所属を変更", |ui| {
                                            changed |= self.task_location_menu(ui, &mut edited);
                                        });
                                        if ui.button("タスク名をコピー").clicked() {
                                            ui.ctx().copy_text(task.title.clone());
                                            ui.close();
                                        }
                                        if ui.button("削除…").clicked() {
                                            *action =
                                                Some(TaskAction::RequestDelete(task.id.clone()));
                                            ui.close();
                                        }
                                    });
                                    let (flag, color, reason) =
                                        task_priority(task, self.now_in_timezone().date(), palette);
                                    ui.label(material_icon_text('\u{e153}', 16.0, color))
                                        .on_hover_text(format!("{flag} · {reason}"));
                                    if let Some(due) = task_fields::date_field(
                                        ui,
                                        id.with("due"),
                                        task.due_date,
                                        self.now_in_timezone().date(),
                                        palette,
                                    ) {
                                        edited.due_date = due;
                                        changed = true;
                                    }
                                    if let Some(minutes) = task_fields::estimate_field(
                                        ui,
                                        id.with("estimate"),
                                        task.estimated_minutes,
                                        palette,
                                    ) {
                                        edited.estimated_minutes = minutes;
                                        changed = true;
                                    }
                                    if let Some(title) = &milestone {
                                        ui.add_sized(
                                            [78.0, 24.0],
                                            egui::Label::new(
                                                regular_text(title)
                                                    .size(10.0)
                                                    .color(palette.accent),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(format!("マイルストーン: {title}"));
                                    }
                                },
                            );
                        },
                    );
                    if changed {
                        self.save_task_inline(edited);
                    }
                    ui.painter().hline(
                        rect.x_range(),
                        rect.bottom(),
                        Stroke::new(0.5_f32, palette.border),
                    );
                },
            );
        });
    }

    pub(super) fn task_composer(
        &mut self,
        ui: &mut egui::Ui,
        project: Option<ProjectId>,
        palette: Palette,
    ) {
        let mut submit = false;
        let mut expand = false;
        let mut props = task_capture::Properties {
            due: parse_optional_date(&self.workspace_ui.capture_due)
                .ok()
                .flatten(),
            minutes: self.workspace_ui.capture_minutes.parse().ok(),
            list: self.workspace_ui.capture_list.clone(),
            status: self.workspace_ui.capture_status.clone(),
        };
        let context = task_capture::CaptureContext {
            today: self.now_in_timezone().date(),
            palette,
            lists: &self.lists,
            statuses: &self.statuses,
            project: project.as_ref(),
        };
        egui::Frame::new()
            .stroke(Stroke::new(1.0_f32, palette.border))
            .corner_radius(6.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    expand = components::icon_button(
                        ui,
                        '\u{e145}',
                        "タスク作成を開く · Ctrl+T",
                        palette,
                    )
                    .clicked();
                    ui.allocate_ui_with_layout(
                        egui::vec2(
                            (ui.available_width() - 28.0 - ui.spacing().item_spacing.x).max(80.0),
                            28.0,
                        ),
                        egui::Layout::left_to_right(Align::Center),
                        |ui| {
                            submit = task_capture::title_input(
                                ui,
                                &mut self.quick_capture,
                                &mut self.workspace_ui.capture_commands,
                                &mut props,
                                &context,
                                false,
                            )
                            .1;
                        },
                    );
                    submit |=
                        components::icon_button(ui, '\u{e5d8}', "タスクを追加（Enter）", palette)
                            .clicked();
                });
                if !self.quick_capture.is_empty() || self.workspace_ui.capture_expanded {
                    task_capture::properties(ui, &mut props, &context, true);
                } else if ui.small_button("期限・時間・所属を設定").clicked() {
                    self.workspace_ui.capture_expanded = true;
                }
            });
        self.workspace_ui.capture_due = props.due.map(|d| d.to_string()).unwrap_or_default();
        self.workspace_ui.capture_minutes =
            props.minutes.map(|m| m.to_string()).unwrap_or_default();
        self.workspace_ui.capture_list = props.list;
        self.workspace_ui.capture_status = props.status;
        if expand {
            self.open_task_capture(ui.ctx());
        }
        if submit {
            self.submit_composer(project);
        }
    }

    pub(super) fn submit_composer(&mut self, project: Option<ProjectId>) {
        let result = (|| -> Result<CaptureTaskRequest> {
            let mut request =
                parse_quick_capture(&self.quick_capture, self.now_in_timezone().date())?;
            if !self.workspace_ui.capture_due.is_empty() {
                request.due_date = parse_optional_date_with_today(
                    &self.workspace_ui.capture_due,
                    self.now_in_timezone().date(),
                )?;
            }
            if !self.workspace_ui.capture_minutes.is_empty() {
                request.estimated_minutes = Some(
                    parse_duration_minutes(&self.workspace_ui.capture_minutes)
                        .ok_or_else(|| anyhow!("見積時間を確認してください"))?,
                );
            }
            request.status_id = self.workspace_ui.capture_status.clone();
            Ok(request)
        })();
        let request = match result {
            Ok(r) => r,
            Err(e) => {
                self.set_error(e);
                return;
            }
        };
        let list = self.workspace_ui.capture_list.clone();
        self.capture_task_in_location(request, project, list);
        if self.error.is_none() {
            self.quick_capture.clear();
            self.workspace_ui.capture_due.clear();
            self.workspace_ui.capture_minutes.clear();
            self.workspace_ui.capture_expanded = false;
            self.workspace_ui.capture_status = None;
            self.workspace_ui.capture_commands = task_capture::CommandMenu::default();
        }
    }

    fn task_location_menu(&self, ui: &mut egui::Ui, task: &mut Task) -> bool {
        let mut changed = false;
        ui.label(bold_text("所属リスト"));
        for list in &self.lists {
            if ui
                .selectable_label(task.list_id.as_ref() == Some(&list.id), &list.name)
                .clicked()
            {
                task.list_id = Some(list.id.clone());
                task.project_id = list.project_id.clone();
                if !self.workspace_ui.task_milestones.iter().any(|m| {
                    Some(&m.id) == task.milestone_id.as_ref()
                        && Some(&m.project_id) == task.project_id.as_ref()
                }) {
                    task.milestone_id = None;
                }
                changed = true;
                ui.close();
            }
        }
        changed
    }

    fn save_task_inline(&mut self, task: Task) -> Option<Task> {
        let id = task.id.clone();
        match self.persist_task_details(task) {
            Ok(()) => {
                self.refresh_tasks();
                self.refresh_schedule();
                self.refresh_schedule_month();
                self.tasks
                    .iter()
                    .chain(self.done_tasks.iter())
                    .find(|t| t.id == id)
                    .cloned()
            }
            Err(error) => {
                self.set_error(error);
                None
            }
        }
    }

    pub(super) fn show_task_details(&mut self, ctx: &egui::Context, palette: Palette) {
        if let Some(mut edit) = self.workspace_ui.task_editor.take() {
            if self.workspace_ui.detail_task_id.as_ref() != Some(&edit.id) {
                self.workspace_ui.detail_task_id = Some(edit.id.clone());
                self.workspace_ui.detail_title_editing = false;
                self.workspace_ui.detail_description_editing = false;
                self.workspace_ui.detail_title = edit.title.clone();
                self.workspace_ui.detail_description = edit.description.clone().unwrap_or_default();
            }
            let mut open = true;
            let mut changed = false;
            let mut save_title = false;
            let mut save_description = false;
            let original = edit.clone();
            let width = (ctx.content_rect().width() - 100.0).clamp(520.0, 820.0);
            let detail = egui::Modal::new(egui::Id::new("task_details"))
                .frame(egui::Frame::window(&ctx.global_style()).inner_margin(28))
                .show(ctx, |ui| {
                    ui.set_width(width);
                    ui.set_height((ctx.content_rect().height() - 160.0).clamp(350.0, 640.0));
                    ui.horizontal(|ui| {
                        ui.label(material_icon_text(ICON_FOLDER, 15.0, palette.muted));
                        ui.label(
                            regular_text(task_context_line(&edit, &self.lists, &self.projects))
                                .size(12.0)
                                .color(palette.muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            if components::icon_button(
                                ui,
                                '\u{e5cd}',
                                "詳細を閉じる · Esc",
                                palette,
                            )
                            .clicked()
                            {
                                open = false;
                            }
                        });
                    });
                    ui.separator();
                    ui.add_space(22.0);
                    if let Some(error) = &self.error {
                        ui.colored_label(palette.error, error);
                    }
                    ScrollArea::vertical()
                        .id_salt("task_detail_body")
                        .max_height(ui.available_height())
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if self.workspace_ui.detail_title_editing {
                                let response = ui.add(
                                    TextEdit::singleline(&mut self.workspace_ui.detail_title)
                                        .id(ui.id().with("detail_title_input"))
                                        .font(egui::FontId::proportional(26.0))
                                        .desired_width(f32::INFINITY),
                                );
                                if response.has_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    edit.title = self.workspace_ui.detail_title.clone();
                                    changed = true;
                                    save_title = true;
                                }
                                ui.horizontal(|ui| {
                                    if components::button(ui, "保存", true, palette).clicked() {
                                        edit.title = self.workspace_ui.detail_title.clone();
                                        changed = true;
                                        save_title = true;
                                    }
                                    if ui.button("キャンセル").clicked() {
                                        self.workspace_ui.detail_title_editing = false;
                                        self.workspace_ui.detail_title = edit.title.clone();
                                    }
                                });
                            } else if ui
                                .add(
                                    egui::Label::new(bold_text(&edit.title).size(28.0))
                                        .sense(egui::Sense::click()),
                                )
                                .on_hover_text("タイトルを編集")
                                .clicked()
                            {
                                self.workspace_ui.detail_title_editing = true;
                                ui.memory_mut(|memory| {
                                    memory.request_focus(ui.id().with("detail_title_input"))
                                });
                            }
                            ui.add_space(28.0);
                            egui::Grid::new("task_detail_properties")
                                .num_columns(2)
                                .min_col_width(155.0)
                                .spacing([20.0, 16.0])
                                .show(ui, |ui| {
                                    ui.label(regular_text("ステータス").color(palette.muted));
                                    ui.horizontal(|ui| {
                                        let mut action = None;
                                        task_status_button(
                                            ui,
                                            "task_detail",
                                            &edit,
                                            &self.statuses,
                                            &self.status_groups,
                                            palette,
                                            &mut action,
                                        );
                                        ui.label(status_label_for_task(&edit, &self.statuses));
                                        if let Some(TaskAction::SetStatus(_, id)) = action {
                                            edit.status_id = id;
                                            changed = true;
                                        }
                                    });
                                    ui.end_row();
                                    ui.label(regular_text("期限").color(palette.muted));
                                    if let Some(due) = task_fields::date_field(
                                        ui,
                                        ui.id().with(("detail_due", &edit.id)),
                                        edit.due_date,
                                        self.now_in_timezone().date(),
                                        palette,
                                    ) {
                                        edit.due_date = due;
                                        changed = true;
                                    }
                                    ui.end_row();
                                    ui.label(regular_text("見積時間").color(palette.muted));
                                    if let Some(minutes) = task_fields::estimate_field(
                                        ui,
                                        ui.id().with(("detail_estimate", &edit.id)),
                                        edit.estimated_minutes,
                                        palette,
                                    ) {
                                        edit.estimated_minutes = minutes;
                                        changed = true;
                                    }
                                    ui.end_row();
                                    ui.label(regular_text("所属").color(palette.muted));
                                    let location = ui.add(
                                        egui::Button::new(task_context_line(
                                            &edit,
                                            &self.lists,
                                            &self.projects,
                                        ))
                                        .frame(false),
                                    );
                                    components::popup(&location).show(|ui| {
                                        changed |= self.task_location_menu(ui, &mut edit);
                                    });
                                    ui.end_row();
                                    if let Some(project) = edit.project_id.clone() {
                                        ui.label(
                                            regular_text("マイルストーン").color(palette.muted),
                                        );
                                        let title = self
                                            .workspace_ui
                                            .task_milestones
                                            .iter()
                                            .find(|m| Some(&m.id) == edit.milestone_id.as_ref())
                                            .map(|m| m.title.as_str())
                                            .unwrap_or("なし");
                                        let milestone = ui.button(title);
                                        components::popup(&milestone).show(|ui| {
                                            changed |= ui
                                                .selectable_value(
                                                    &mut edit.milestone_id,
                                                    None,
                                                    "なし",
                                                )
                                                .changed();
                                            for m in self
                                                .workspace_ui
                                                .task_milestones
                                                .iter()
                                                .filter(|m| m.project_id == project)
                                            {
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut edit.milestone_id,
                                                        Some(m.id.clone()),
                                                        &m.title,
                                                    )
                                                    .changed();
                                            }
                                        });
                                        ui.end_row();
                                    }
                                    ui.label(regular_text("優先度").color(palette.muted));
                                    let (label, color, reason) = task_priority(
                                        &edit,
                                        self.now_in_timezone().date(),
                                        palette,
                                    );
                                    ui.colored_label(color, label).on_hover_text(reason);
                                    ui.end_row();
                                });
                            ui.add_space(26.0);
                            ui.separator();
                            ui.add_space(16.0);
                            ui.label(bold_text("説明").size(13.0));
                            ui.add_space(8.0);
                            if self.workspace_ui.detail_description_editing {
                                ui.add_sized(
                                    [ui.available_width(), 130.0],
                                    TextEdit::multiline(&mut self.workspace_ui.detail_description)
                                        .id(ui.id().with("detail_description_input"))
                                        .hint_text("詳細を追加"),
                                );
                                ui.horizontal(|ui| {
                                    if components::button(ui, "説明を保存", true, palette).clicked()
                                    {
                                        edit.description = (!self
                                            .workspace_ui
                                            .detail_description
                                            .trim()
                                            .is_empty())
                                        .then(|| self.workspace_ui.detail_description.clone());
                                        changed = true;
                                        save_description = true;
                                    }
                                    if ui.button("キャンセル").clicked() {
                                        self.workspace_ui.detail_description_editing = false;
                                        self.workspace_ui.detail_description =
                                            edit.description.clone().unwrap_or_default();
                                    }
                                });
                            } else if ui
                                .add(
                                    egui::Label::new(
                                        regular_text(
                                            edit.description
                                                .as_deref()
                                                .filter(|s| !s.is_empty())
                                                .unwrap_or("説明を追加…"),
                                        )
                                        .color(
                                            if edit.description.is_some() {
                                                palette.text
                                            } else {
                                                palette.muted
                                            },
                                        ),
                                    )
                                    .sense(egui::Sense::click()),
                                )
                                .on_hover_text("説明を編集")
                                .clicked()
                            {
                                self.workspace_ui.detail_description_editing = true;
                                ui.memory_mut(|memory| {
                                    memory.request_focus(ui.id().with("detail_description_input"))
                                });
                            }
                        });
                });
            if changed {
                if let Some(saved) = self.save_task_inline(edit.clone()) {
                    edit = saved;
                    if save_title {
                        self.workspace_ui.detail_title_editing = false;
                        self.workspace_ui.detail_title = edit.title.clone();
                    }
                    if save_description {
                        self.workspace_ui.detail_description_editing = false;
                        self.workspace_ui.detail_description =
                            edit.description.clone().unwrap_or_default();
                    }
                } else {
                    edit = original;
                }
            }
            if detail.should_close() {
                open = false;
            }
            if open {
                self.workspace_ui.task_editor = Some(edit);
            } else {
                self.workspace_ui.detail_task_id = None;
            }
        }
        if let Some(id) = self.confirming_delete_task_id.clone() {
            egui::Window::new("タスクを削除")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("このタスクを削除しますか？");
                    ui.horizontal(|ui| {
                        if ui.button("キャンセル").clicked() {
                            self.confirming_delete_task_id = None;
                        }
                        if ui.button("削除する").clicked() {
                            self.handle_task_action(TaskAction::ConfirmDelete(id));
                        }
                    });
                });
        }
        if let Some(payload) = egui::DragAndDrop::payload::<TaskDragPayload>(ctx)
            && let Some(pointer) = ctx.pointer_interact_pos()
        {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("task_drag_preview"),
            ));
            let rect = egui::Rect::from_min_size(
                pointer + egui::vec2(14.0, 14.0),
                egui::vec2(250.0, 36.0),
            );
            painter.rect(
                rect,
                6.0,
                palette.surface,
                Stroke::new(1.0_f32, palette.accent),
                egui::StrokeKind::Inside,
            );
            painter.with_clip_rect(rect.shrink(8.0)).text(
                rect.left_center() + egui::vec2(10.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &payload.title,
                egui::FontId::proportional(13.0),
                palette.text,
            );
        }
    }

    pub(super) fn persist_task_details(&self, mut task: Task) -> Result<()> {
        if task.title.trim().is_empty() {
            return Err(anyhow!("タスク名を入力してください"));
        }
        task.title = task.title.trim().to_owned();
        if !status_candidates(&self.statuses, task.project_id.as_ref())
            .iter()
            .any(|s| s.id == task.status_id)
        {
            task.status_id = status_candidates(&self.statuses, task.project_id.as_ref())
                .first()
                .ok_or_else(|| anyhow!("ステータスがありません"))?
                .id
                .clone();
        }
        let expected_updated_at = task.updated_at;
        task.updated_at = OffsetDateTime::now_utc();
        let vault = self
            .vault
            .as_ref()
            .ok_or_else(|| anyhow!("Vault is not connected"))?;
        self.runtime.block_on(async {
            let repo = vault.task_repo();
            let current = repo
                .find(task.id.clone())
                .await?
                .ok_or_else(|| anyhow!("タスクが見つかりません"))?;
            if current.updated_at != expected_updated_at || current.deleted_at.is_some() {
                return Err(anyhow!(
                    "別の操作で更新されました。タスクを開き直してください"
                ));
            }
            repo.update(task).await.map_err(Into::into)
        })
    }
}

pub(super) fn task_kind(
    task: &Task,
    statuses: &[Status],
    groups: &[StatusGroup],
) -> StatusGroupKind {
    statuses
        .iter()
        .find(|s| s.id == task.status_id)
        .and_then(|s| status_group_kind(s, groups))
        .unwrap_or(StatusGroupKind::NotStarted)
}
fn status_label_for_task<'a>(task: &Task, statuses: &'a [Status]) -> &'a str {
    statuses
        .iter()
        .find(|s| s.id == task.status_id)
        .map(|s| s.name.as_str())
        .unwrap_or("未着手")
}
fn task_priority(
    task: &Task,
    today: Date,
    palette: Palette,
) -> (&'static str, Color32, &'static str) {
    if task.due_date.is_some_and(|d| d <= today) {
        ("高", palette.error, "期限が今日以前")
    } else if task.due_date.is_some_and(|d| (d - today).whole_days() <= 3) {
        ("中", palette.warning, "3日以内に期限")
    } else {
        ("通常", palette.muted, "期限に余裕あり / 期限なし")
    }
}
pub(super) use super::components::icon_button;

fn row_text(
    ui: &mut egui::Ui,
    text: &str,
    width: f32,
    font_size: f32,
    color: Color32,
    sense: egui::Sense,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 30.0), sense);
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        egui::FontId::proportional(font_size),
        color,
    );
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(
            rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0),
            galley,
            color,
        );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    response
}
