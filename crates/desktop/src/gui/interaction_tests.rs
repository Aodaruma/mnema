use super::*;

pub(super) fn fixture() -> (tempfile::TempDir, MnemaGuiApp, egui::Context) {
    let directory = tempfile::tempdir().unwrap();
    let config = DesktopConfig {
        vault_path: directory.path().join("vault").display().to_string(),
        storage_backend: DesktopStorageBackend::Sqlite,
        sqlite_path: directory.path().join("test.sqlite").display().to_string(),
        database_url: String::new(),
        dark_mode: true,
        timezone_offset: "Asia/Tokyo".into(),
        timezone_mode: TimezoneMode::Manual,
        manual_timezone: "Asia/Tokyo".into(),
        availability_start: "09:00".into(),
        availability_end: "17:00".into(),
        home_task_density: TaskListDensity::Normal,
        home_task_sort: TaskSortMode::DueDate,
        task_grouping: tasks::TaskGrouping::Schedule,
        llm_provider: DesktopLlmProvider::Disabled,
        ollama_url: String::new(),
        openai_url: String::new(),
        planning_model: String::new(),
        routine_model: String::new(),
        read_notifications: Vec::new(),
    };
    let app = MnemaGuiApp::from_config(config, None);
    assert!(app.error.is_none(), "{:?}", app.error);
    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    (directory, app, ctx)
}

fn seed_task(app: &mut MnemaGuiApp) -> Task {
    app.quick_capture = "Review /due tomorrow /minutes 30".into();
    app.submit_composer(None);
    assert!(app.error.is_none(), "{:?}", app.error);
    app.tasks[0].clone()
}

fn row_frame(
    app: &mut MnemaGuiApp,
    ctx: &egui::Context,
    task: &Task,
    events: Vec<egui::Event>,
) -> egui::Rect {
    let mut rect = egui::Rect::NOTHING;
    let mut action = None;
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            rect = egui::Rect::from_min_size(
                ui.next_widget_position(),
                egui::vec2(ui.available_width(), 46.0),
            );
            app.workspace_task_row(ui, task, Palette::at(1.0), &mut action);
        },
    );
    if let Some(action) = action {
        app.handle_task_action(action);
    }
    rect
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn status_click_opens_menu_without_starting_a_drag() {
    let (_directory, mut app, ctx) = fixture();
    let task = seed_task(&mut app);
    let row = row_frame(&mut app, &ctx, &task, vec![]);
    // Handle (14), spacing (4), then a 30px status button.
    let status = row.left_top() + egui::vec2(33.0, 23.0);
    row_frame(
        &mut app,
        &ctx,
        &task,
        vec![egui::Event::PointerMoved(status)],
    );
    row_frame(&mut app, &ctx, &task, vec![button(status, true)]);
    row_frame(&mut app, &ctx, &task, vec![button(status, false)]);
    assert!(egui::Popup::is_any_open(&ctx));
    assert!(!egui::DragAndDrop::has_any_payload(&ctx));
    assert_eq!(app.tasks[0].status_id, task.status_id);
}

#[test]
fn completing_task_does_not_flash_red_diagnostic_lines() {
    for quick_done in [true, false] {
        for grouping in [tasks::TaskGrouping::Schedule, tasks::TaskGrouping::Status] {
            for count in [1, 2] {
                for due in ["yesterday", "today", "tomorrow"] {
                    let (_directory, mut app, ctx) = fixture();
                    configure_style(&ctx, 1.0, true);
                    app.quick_capture = format!("First /due {due}");
                    app.submit_composer(None);
                    let task = app.tasks[0].clone();
                    if count == 2 {
                        app.quick_capture = format!("Second /due {due}");
                        app.submit_composer(None);
                    }
                    // Seed operations show a message. Real startup hides the status bar,
                    // so the first Done action must also exercise its appearance.
                    app.message = "Vault connected".into();
                    app.workspace_ui.task_grouping = grouping;
                    app.view = View::Home;
                    let done = status_candidates(&app.statuses, task.project_id.as_ref())
                        .into_iter()
                        .find(|s| {
                            status_group_kind(s, &app.status_groups) == Some(StatusGroupKind::Done)
                        })
                        .unwrap()
                        .clone();
                    let mut time = 0.0;
                    let mut frame = |events| {
                        time += 1.0 / 60.0;
                        let output = ctx.run_ui(
                            egui::RawInput {
                                time: Some(time),
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(1360.0, 950.0),
                                )),
                                events,
                                ..Default::default()
                            },
                            |ui| app.show_shell(ui, Palette::at(1.0)),
                        );
                        let red_rects: Vec<_> = output
                            .shapes
                            .iter()
                            .filter_map(|shape| match &shape.shape {
                                egui::Shape::Rect(rect) if rect.stroke.color == Color32::RED => {
                                    Some(rect.rect)
                                }
                                _ => None,
                            })
                            .collect();
                        assert!(
                            red_rects.is_empty(),
                            "quick={quick_done}/{grouping:?}/{count}/{due}: red diagnostics at {red_rects:?}, frame {time}"
                        );
                        output
                    };
                    for _ in 0..3 {
                        frame(vec![]);
                    }
                    let initial = frame(vec![]);
                    let title_pos = text_position(&initial, &task.title);
                    if quick_done {
                        frame(vec![egui::Event::PointerMoved(title_pos)]);
                        let hover = frame(vec![]);
                        let done_pos = hover
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Text(text)
                                    if text.galley.text() == ICON_CHECK.to_string()
                                        && text.pos.x > title_pos.x
                                        && (text.pos.y - title_pos.y).abs() < 20.0 =>
                                {
                                    Some(text.pos + egui::vec2(7.0, 7.0))
                                }
                                _ => None,
                            })
                            .expect("hover completion button");
                        frame(vec![egui::Event::PointerMoved(done_pos)]);
                        frame(vec![button(done_pos, true)]);
                        frame(vec![button(done_pos, false)]);
                    } else {
                        let status = title_pos - egui::vec2(24.0, 0.0);
                        frame(vec![egui::Event::PointerMoved(status)]);
                        frame(vec![button(status, true)]);
                        frame(vec![button(status, false)]);
                        let menu = frame(vec![]);
                        assert!(egui::Popup::is_any_open(&ctx));
                        let done_pos = text_position(&menu, &done.name);
                        frame(vec![egui::Event::PointerMoved(done_pos)]);
                        frame(vec![button(done_pos, true)]);
                        frame(vec![button(done_pos, false)]);
                    }
                    for _ in 0..12 {
                        frame(vec![]);
                    }
                    assert_eq!(app.tasks.len(), count - 1);
                    assert_eq!(app.done_tasks[0].id, task.id);
                }
            }
        }
    }
}

#[test]
fn task_handle_drags_without_opening_editor_or_changing_task() {
    let (_directory, mut app, ctx) = fixture();
    let task = seed_task(&mut app);
    let row = row_frame(&mut app, &ctx, &task, vec![]);
    let start = row.left_top() + egui::vec2(7.0, 23.0);
    row_frame(
        &mut app,
        &ctx,
        &task,
        vec![egui::Event::PointerMoved(start)],
    );
    row_frame(&mut app, &ctx, &task, vec![button(start, true)]);
    row_frame(
        &mut app,
        &ctx,
        &task,
        vec![egui::Event::PointerMoved(start + egui::vec2(100.0, 50.0))],
    );
    let payload = egui::DragAndDrop::payload::<TaskDragPayload>(&ctx).expect("task payload");
    assert_eq!(payload.task_id, task.id);
    assert!(app.workspace_ui.task_editor.is_none());
    assert!(!egui::Popup::is_any_open(&ctx));
    assert_eq!(app.tasks[0], task);
}

#[test]
fn status_bar_visibility_does_not_change_workspace_widget_ids() {
    let (_directory, mut app, ctx) = fixture();
    seed_task(&mut app);
    app.project_title = "Review project".into();
    app.add_project();
    app.habit_draft.title = "Reading".into();
    app.save_habit_form();
    let mut time = 0.0;
    for dark in [false, true] {
        let factor = if dark { 1.0 } else { 0.0 };
        configure_style(&ctx, factor, dark);
        for width in [860.0, 1360.0, 2560.0] {
            for view in [
                View::Home,
                View::Inbox,
                View::Projects,
                View::Schedule,
                View::Habits,
                View::Activity,
                View::Assistant,
                View::Settings,
            ] {
                app.view = view;
                for (message, error) in [
                    ("Vault connected", None),
                    ("Updated status: Review", None),
                    ("", Some("A recoverable error".to_owned())),
                    ("", None),
                ] {
                    app.message = message.into();
                    app.error = error;
                    for _ in 0..3 {
                        time += 1.0 / 60.0;
                        let output = ctx.run_ui(
                            egui::RawInput {
                                time: Some(time),
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 950.0),
                                )),
                                ..Default::default()
                            },
                            |ui| app.show_shell(ui, Palette::at(factor)),
                        );
                        for shape in &output.shapes {
                            if let egui::Shape::Rect(rect) = &shape.shape {
                                assert_ne!(
                                    rect.stroke.color,
                                    Color32::RED,
                                    "{view:?}/{width}/{dark}/{message}: red diagnostic {:?}, frame {time}",
                                    rect.rect
                                );
                            }
                            if let egui::Shape::Text(text) = &shape.shape {
                                assert!(!text.galley.text().contains("use of widget ID"));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn composer_and_completed_task_details_persist_in_selected_project() {
    let (_directory, mut app, _ctx) = fixture();
    app.project_title = "Project".into();
    app.add_project();
    let project = app.selected_project_id.clone().unwrap();
    app.quick_capture = "Review /due tomorrow /minutes 30".into();
    app.submit_composer(Some(project.clone()));
    assert!(app.error.is_none(), "{:?}", app.error);
    let task = app.tasks[0].clone();
    assert_eq!(task.project_id, Some(project));
    assert!(task.list_id.is_some());
    assert_eq!(task.estimated_minutes, Some(30));
    assert_eq!(task.due_date, app.now_in_timezone().date().next_day());
    let done = status_candidates(&app.statuses, task.project_id.as_ref())
        .into_iter()
        .find(|s| status_group_kind(s, &app.status_groups) == Some(StatusGroupKind::Done))
        .unwrap()
        .id
        .clone();
    app.update_task_status(task.id.clone(), done);
    let mut edit = app.done_tasks[0].clone();
    edit.title = "Reviewed".into();
    edit.description = Some("Saved details".into());
    edit.estimated_minutes = Some(45);
    app.persist_task_details(edit.clone()).unwrap();
    app.refresh_tasks();
    assert!(app.tasks.is_empty());
    assert_eq!(app.done_tasks[0].title, "Reviewed");
    assert_eq!(
        app.done_tasks[0].description.as_deref(),
        Some("Saved details")
    );
    assert_eq!(app.done_tasks[0].estimated_minutes, Some(45));
    assert!(
        app.persist_task_details(edit).is_err(),
        "stale edit must not overwrite a newer version"
    );
}

#[test]
fn missing_capture_destination_does_not_leave_an_inbox_task() {
    let (_directory, mut app, _ctx) = fixture();
    app.capture_task_in_location(
        CaptureTaskRequest {
            status_id: None,
            title: "Do not create".into(),
            description: None,
            due_date: None,
            estimated_minutes: None,
        },
        None,
        Some(ListId::new()),
    );
    assert!(app.error.is_some());
    app.refresh_tasks();
    assert!(app.tasks.is_empty());
}

#[test]
fn workspace_pages_fit_small_and_maximized_viewports() {
    let (_directory, mut app, ctx) = fixture();
    seed_task(&mut app);
    app.project_title = "制作プロジェクト".into();
    app.add_project();
    let mut overflows = Vec::new();
    configure_style(&ctx, 1.0, true);
    for width in [860.0, 1360.0, 2560.0] {
        for view in [
            View::Home,
            View::Inbox,
            View::Projects,
            View::Schedule,
            View::Habits,
            View::Activity,
            View::Assistant,
            View::Settings,
        ] {
            app.view = view;
            for section in [
                pages::SettingsSection::General,
                pages::SettingsSection::Planning,
                pages::SettingsSection::Connections,
                pages::SettingsSection::Assistant,
                pages::SettingsSection::Storage,
            ] {
                if view != View::Settings && section != pages::SettingsSection::General {
                    continue;
                }
                app.workspace_ui.settings_section = section;
                let mut bounds = egui::Rect::NOTHING;
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        app.show_shell(ui, Palette::at(1.0));
                        bounds = ui.min_rect();
                    },
                );
                assert!(bounds.is_finite());
                if bounds.width() > width + 1.0 {
                    overflows.push(format!("{view:?}/{section:?} at {width}: {bounds:?}"));
                }
            }
        }
    }
    assert!(overflows.is_empty(), "{}", overflows.join("\n"));
}

#[test]
fn task_date_groups_use_persisted_future_schedule_beyond_the_visible_month() {
    let (_directory, mut app, _ctx) = fixture();
    let mut task = seed_task(&mut app);
    let today = app.now_in_timezone().date();
    task.due_date = None;
    assert_eq!(
        tasks::date_group(&task, today, &[], &app.timezone_offset),
        tasks::DateGroup::Unscheduled
    );
    let later = add_days(today, 70).unwrap();
    app.create_manual_schedule_block(ManualScheduleRequest {
        task_id: task.id.clone(),
        title: task.title.clone(),
        start_at: calendar::parse_event_time(later, "09:00", &app.timezone_offset, true).unwrap(),
        duration_minutes: 30,
    });
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(
        tasks::date_group(
            &task,
            today,
            &app.workspace_ui.calendar.future_blocks,
            &app.timezone_offset
        ),
        tasks::DateGroup::Next
    );
    let block = app.workspace_ui.calendar.future_blocks[0].clone();
    app.update_schedule_block_state(block.id, ScheduleBlockState::Cancelled);
    assert_eq!(
        tasks::date_group(
            &task,
            today,
            &app.workspace_ui.calendar.future_blocks,
            &app.timezone_offset
        ),
        tasks::DateGroup::Unscheduled
    );
    task.due_date = Some(today);
    assert_eq!(
        tasks::date_group(&task, today, &[], &app.timezone_offset),
        tasks::DateGroup::Today
    );
    task.due_date = today.previous_day();
    assert_eq!(
        tasks::date_group(&task, today, &[], &app.timezone_offset),
        tasks::DateGroup::Overdue
    );
}

#[test]
fn habit_form_saves_and_edits_without_replacing_identity_or_accepting_stale_data() {
    let (_directory, mut app, _ctx) = fixture();
    app.habit_draft.title = "Reading".into();
    app.habit_draft.schedule = HabitScheduleChoice::Weekdays;
    app.habit_draft.weekdays = [true, false, true, false, true, false, false];
    app.habit_draft.duration_minutes = "45".into();
    app.workspace_ui.habit_create_open = true;
    app.save_habit_form();
    assert!(app.error.is_none(), "{:?}", app.error);
    assert!(!app.workspace_ui.habit_create_open);
    let original = app.habits[0].clone();
    app.workspace_ui.editing_habit = Some(original.clone());
    app.habit_draft = HabitDraft::from_habit(&original);
    assert_eq!(
        app.habit_draft.weekdays,
        [true, false, true, false, true, false, false]
    );
    app.habit_draft.title = "Book time".into();
    app.habit_draft.duration_minutes = "20".into();
    app.save_habit_form();
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(app.habits[0].id, original.id);
    assert_eq!(app.habits[0].created_at, original.created_at);
    assert_eq!(app.habits[0].duration_minutes, 20);
    app.workspace_ui.editing_habit = Some(original);
    app.habit_draft.title = "Stale overwrite".into();
    app.save_habit_form();
    assert!(app.error.is_some());
    assert_eq!(app.habits[0].title, "Book time");
}

#[test]
fn schedule_fixed_switch_round_trips_and_does_not_reopen_finished_events() {
    let (_directory, mut app, _ctx) = fixture();
    let task = seed_task(&mut app);
    app.create_manual_schedule_block(ManualScheduleRequest {
        task_id: task.id,
        title: task.title,
        start_at: OffsetDateTime::now_utc(),
        duration_minutes: 30,
    });
    let id = app.workspace_ui.calendar.future_blocks[0].id.clone();
    let vault = app.vault_clone().unwrap();
    app.runtime.block_on(async {
        let repo = vault.schedule_block_repo();
        let service = ScheduleBlockCommandService::new(repo.as_ref());
        let automatic = service.set_fixed(id.clone(), false).await.unwrap();
        assert!(!automatic.locked);
        assert_eq!(automatic.source, ScheduleBlockSource::Scheduler);
        assert_eq!(automatic.state, ScheduleBlockState::Proposed);
        let fixed = service.set_fixed(id.clone(), true).await.unwrap();
        assert!(fixed.locked);
        assert_eq!(fixed.state, ScheduleBlockState::Scheduled);
        service
            .update_state(id.clone(), ScheduleBlockState::Done)
            .await
            .unwrap();
        assert!(service.set_fixed(id.clone(), false).await.is_err());
        assert_eq!(
            repo.find(id).await.unwrap().unwrap().state,
            ScheduleBlockState::Done
        );
    });
}

#[test]
fn calendar_modes_and_habit_forms_fit_supported_window_sizes() {
    let (_directory, mut app, ctx) = fixture();
    app.habit_draft.title = "A habit with a longer title".into();
    app.add_habit();
    configure_style(&ctx, 1.0, true);
    for width in [860.0, 1360.0, 2560.0] {
        for mode in [
            ScheduleViewMode::Day,
            ScheduleViewMode::Days(3),
            ScheduleViewMode::Week,
            ScheduleViewMode::Calendar,
        ] {
            app.view = View::Schedule;
            app.workspace_ui.calendar.schedule_mode = mode;
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 900.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    app.show_shell(ui, Palette::at(1.0));
                    assert!(
                        ui.min_rect().width() <= width + 1.0,
                        "{mode:?}: {:?}",
                        ui.min_rect()
                    );
                    assert!(
                        ui.min_rect().height() <= 901.0,
                        "{mode:?}: {:?}",
                        ui.min_rect()
                    );
                },
            );
        }
        for view in [
            habits::HabitView::Gallery,
            habits::HabitView::Priority,
            habits::HabitView::Templates,
        ] {
            app.view = View::Habits;
            app.workspace_ui.habit_view = view;
            for form in [false, true] {
                app.workspace_ui.habit_create_open = form;
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        app.show_shell(ui, Palette::at(1.0));
                        assert!(
                            ui.min_rect().width() <= width + 1.0,
                            "habit form={form}: {:?}",
                            ui.min_rect()
                        );
                    },
                );
            }
        }
    }
}

fn task_ui_frame(
    app: &mut MnemaGuiApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let task = app.tasks[0].clone();
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            app.workspace_task_row(ui, &task, Palette::at(1.0), &mut None);
            app.show_task_details(ui.ctx(), Palette::at(1.0));
        },
    )
}

fn text_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + egui::vec2(5.0, 6.0))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing text: {label}"))
}

fn task_click(app: &mut MnemaGuiApp, ctx: &egui::Context, pos: egui::Pos2) -> egui::FullOutput {
    task_ui_frame(app, ctx, vec![egui::Event::PointerMoved(pos)]);
    task_ui_frame(app, ctx, vec![button(pos, true)]);
    task_ui_frame(app, ctx, vec![button(pos, false)]);
    task_ui_frame(app, ctx, vec![])
}

#[test]
fn task_date_and_estimate_edit_inline_and_only_title_opens_details() {
    let (_directory, mut app, ctx) = fixture();
    configure_style(&ctx, 1.0, true);
    let original = seed_task(&mut app);
    let due = original.due_date.unwrap();
    let first = task_ui_frame(&mut app, &ctx, vec![]);
    let date_pos = text_position(&first, &format!("{}/{}", due.month() as u8, due.day()));
    let popup = task_click(&mut app, &ctx, date_pos);
    assert!(egui::Popup::is_any_open(&ctx));
    assert!(app.workspace_ui.task_editor.is_none());
    assert!(!egui::DragAndDrop::has_any_payload(&ctx));
    task_click(&mut app, &ctx, text_position(&popup, "今日"));
    assert_eq!(app.tasks[0].due_date, Some(app.now_in_timezone().date()));
    assert_eq!(app.tasks[0].estimated_minutes, original.estimated_minutes);
    assert_eq!(app.tasks[0].status_id, original.status_id);
    assert!(app.workspace_ui.task_editor.is_none());
    let row = task_ui_frame(&mut app, &ctx, vec![]);
    let popup = task_click(&mut app, &ctx, text_position(&row, "30分"));
    assert!(egui::Popup::is_any_open(&ctx));
    assert!(app.workspace_ui.task_editor.is_none());
    task_click(&mut app, &ctx, text_position(&popup, "15分"));
    assert_eq!(app.tasks[0].estimated_minutes, Some(15));
    app.refresh_tasks();
    assert_eq!(app.tasks[0].estimated_minutes, Some(15));
    assert_eq!(app.tasks[0].due_date, Some(app.now_in_timezone().date()));
    let row = task_ui_frame(&mut app, &ctx, vec![]);
    task_click(&mut app, &ctx, text_position(&row, "Review"));
    assert!(app.workspace_ui.task_editor.is_some());
    assert!(!app.workspace_ui.detail_title_editing);
    assert!(!app.workspace_ui.detail_description_editing);
    let details = task_ui_frame(&mut app, &ctx, vec![]);
    assert!(details.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text() == "説明" && shape.clip_rect.contains(text.pos)
    )), "description must be visible in the detail view without scrolling");
    let today = app.now_in_timezone().date();
    let due_label = format!("{}/{}", today.month() as u8, today.day());
    // The modal is painted last, after the row behind it.
    let date_pos = details
        .shapes
        .iter()
        .rev()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == due_label => {
                Some(text.pos + egui::vec2(5.0, 6.0))
            }
            _ => None,
        })
        .unwrap();
    task_click(&mut app, &ctx, date_pos);
    assert!(egui::Popup::is_any_open(&ctx));
    task_ui_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(
        app.workspace_ui.task_editor.is_some(),
        "Escape dismisses the field popup before the detail view"
    );
}

#[test]
fn calendar_shortcuts_match_notion_and_respect_editing_contexts() {
    let (_directory, mut app, ctx) = fixture();
    let task = seed_task(&mut app);
    let frame = |app: &mut MnemaGuiApp, key: egui::Key| {
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| app.handle_keyboard_shortcuts(ui.ctx()),
        );
    };
    for view in [View::Home, View::Schedule] {
        app.view = view;
        for (key, expected) in [
            (egui::Key::Num4, ScheduleViewMode::Days(4)),
            (egui::Key::Num9, ScheduleViewMode::Days(9)),
            (egui::Key::D, ScheduleViewMode::Day),
            (egui::Key::W, ScheduleViewMode::Week),
            (egui::Key::M, ScheduleViewMode::Calendar),
            (egui::Key::Num0, ScheduleViewMode::Week),
        ] {
            frame(&mut app, key);
            assert_eq!(
                if view == View::Home {
                    app.home_calendar_mode
                } else {
                    app.workspace_ui.calendar.schedule_mode
                },
                expected
            );
        }
        app.target_date = "2025-01-01".into();
        app.workspace_ui.calendar.pan = [0.4, 0.7];
        frame(&mut app, egui::Key::T);
        assert_eq!(app.target_date, app.now_in_timezone().date().to_string());
        assert_eq!(app.workspace_ui.calendar.pan, [0.0; 2]);
    }
    app.view = View::Home;
    app.home_calendar_mode = ScheduleViewMode::Day;
    app.workspace_ui.task_editor = Some(task);
    frame(&mut app, egui::Key::M);
    assert_eq!(app.home_calendar_mode, ScheduleViewMode::Day);
    app.workspace_ui.task_editor = None;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.text_edit_singleline(&mut app.quick_capture)
            .request_focus();
    });
    frame(&mut app, egui::Key::M);
    assert_eq!(
        app.home_calendar_mode,
        ScheduleViewMode::Day,
        "typing must not change the calendar"
    );
}

#[test]
fn shifted_wheel_moves_dates_smoothly_without_scrolling_the_time_axis() {
    let (_directory, mut app, ctx) = fixture();
    configure_style(&ctx, 1.0, true);
    app.workspace_ui.calendar.schedule_mode = ScheduleViewMode::Days(3);
    app.target_date = "2026-09-30".into();
    app.refresh_schedule_month();
    app.scroll_home_agenda_to_now = false;
    let frame = |app: &mut MnemaGuiApp, events: Vec<egui::Event>| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(950.0, 900.0),
                )),
                modifiers: egui::Modifiers::SHIFT,
                events,
                ..Default::default()
            },
            |ui| app.calendar_panel(ui, false, Palette::at(1.0)),
        )
    };
    let first = frame(&mut app, vec![]);
    let hour = text_position(&first, "09:00");
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(egui::pos2(450.0, 400.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0.0, -420.0),
                modifiers: egui::Modifiers::SHIFT,
            },
        ],
    );
    let mut last = first;
    for _ in 0..30 {
        last = frame(&mut app, vec![]);
    }
    assert!(app.target_date.as_str() > "2026-09-30");
    assert!(app.workspace_ui.calendar.pan[0] > 0.0 && app.workspace_ui.calendar.pan[0] < 1.0);
    assert_eq!(text_position(&last, "09:00"), hour, "time axis stays fixed");
    assert!(app.error.is_none(), "{:?}", app.error);
}

#[test]
fn workspace_hover_has_no_widget_id_clashes() {
    let (_directory, mut app, ctx) = fixture();
    seed_task(&mut app);
    app.project_title = "Review project".into();
    app.add_project();
    app.habit_draft.title = "Reading".into();
    app.save_habit_form();
    let mut time = 0.0;
    for dark in [false, true] {
        let factor = if dark { 1.0 } else { 0.0 };
        configure_style(&ctx, factor, dark);
        app.dark_mode = dark;
        for width in [860.0, 1360.0, 2560.0] {
            for view in [
                View::Home,
                View::Inbox,
                View::Projects,
                View::Schedule,
                View::Habits,
                View::Activity,
                View::Assistant,
                View::Settings,
            ] {
                app.view = view;
                app.workspace_ui.settings_section = pages::SettingsSection::Planning;
                let mut positions = vec![egui::pos2(10.0, 10.0)];
                for step in 0..2 {
                    let mut more = vec![];
                    for position in &positions {
                        for _ in 0..3 {
                            time += 0.6;
                            let output = ctx.run_ui(
                                egui::RawInput {
                                    time: Some(time),
                                    screen_rect: Some(egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(width, 900.0),
                                    )),
                                    events: vec![egui::Event::PointerMoved(*position)],
                                    ..Default::default()
                                },
                                |ui| app.show_shell(ui, Palette::at(factor)),
                            );
                            for shape in &output.shapes {
                                if let egui::Shape::Rect(rect) = &shape.shape {
                                    assert!(
                                        rect.stroke.color != Color32::RED,
                                        "{view:?} {width} {dark}: Red debug frame on hover at {position:?}: {:?}",
                                        rect.rect
                                    );
                                }
                                if let egui::Shape::Text(text) = &shape.shape {
                                    assert!(
                                        !text.galley.text().contains("use of widget ID"),
                                        "Hover at {position:?}: {}",
                                        text.galley.text()
                                    );
                                    if step == 0 && shape.clip_rect.contains(text.pos) {
                                        more.push(text.pos + egui::vec2(4.0, 5.0));
                                    }
                                }
                            }
                        }
                    }
                    if step == 0 {
                        positions = more;
                    }
                }
            }
        }
    }
}
