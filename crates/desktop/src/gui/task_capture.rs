use super::*;

#[derive(Default)]
pub(super) struct TaskCapture {
    pub open: bool,
    initialized: bool,
    title: String,
    description: String,
    properties: Properties,
    project: Option<ProjectId>,
    commands: CommandMenu,
    focus_title: bool,
    keep_open: bool,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct Properties {
    pub due: Option<Date>,
    pub minutes: Option<u32>,
    pub list: Option<ListId>,
    pub status: Option<StatusId>,
}

#[derive(Default)]
pub(super) struct CommandMenu {
    section: Option<Command>,
    selected: usize,
    last_query: String,
    dismissed: Option<String>,
    open: bool,
}

#[derive(Clone, Copy)]
enum Command {
    Due,
    Today,
    Tomorrow,
    Estimate,
    Status,
    Location,
}

enum Choice {
    Command(Command),
    Due(Option<Date>),
    Minutes(Option<u32>),
    Status(StatusId),
    List(ListId),
}

pub(super) struct CaptureContext<'a> {
    pub today: Date,
    pub palette: Palette,
    pub lists: &'a [List],
    pub statuses: &'a [Status],
    pub project: Option<&'a ProjectId>,
}

// Only a separate trailing slash token is a command, never a URL or a path.
fn slash_query(title: &str) -> Option<(usize, &str)> {
    let start = title.rfind('/')?;
    if start > 0 && !title[..start].ends_with(char::is_whitespace) {
        return None;
    }
    let query = &title[start + 1..];
    (!query.contains(char::is_whitespace)).then_some((start, query))
}

fn choices(
    section: Option<Command>,
    query: &str,
    props: &Properties,
    context: &CaptureContext<'_>,
) -> Vec<(String, String, Choice)> {
    let today = context.today;
    let mut entries: Vec<(String, String, Choice)> = match section {
        None => [
            ("期日", "due date", Command::Due),
            ("期日を今日に設定", "today", Command::Today),
            ("期日を明日に設定", "tomorrow", Command::Tomorrow),
            ("見積時間", "estimate minutes", Command::Estimate),
            ("ステータス", "status", Command::Status),
            ("所属リスト", "move list", Command::Location),
        ]
        .into_iter()
        .map(|(label, hint, command)| (label.into(), hint.into(), Choice::Command(command)))
        .collect(),
        Some(Command::Due) => [
            ("今日", Some(today)),
            ("明日", today.next_day()),
            (
                "今週の金曜日",
                add_days(
                    today,
                    (4 - today.weekday().number_days_from_monday() as i32).rem_euclid(7),
                ),
            ),
            ("翌週", add_days(calendar_grid_start(today), 7)),
            ("7日後", add_days(today, 7)),
            ("期限をクリア", None),
        ]
        .into_iter()
        .map(|(label, date)| {
            (
                label.into(),
                date.map(|d| d.to_string()).unwrap_or_default(),
                Choice::Due(date),
            )
        })
        .collect(),
        Some(Command::Estimate) => [
            Some(15),
            Some(30),
            Some(45),
            Some(60),
            Some(90),
            Some(120),
            None,
        ]
        .into_iter()
        .map(|m| {
            (
                m.map(|m| format!("{m}分"))
                    .unwrap_or_else(|| "見積をクリア".into()),
                String::new(),
                Choice::Minutes(m),
            )
        })
        .collect(),
        Some(Command::Status) => {
            let list = props
                .list
                .as_ref()
                .and_then(|id| context.lists.iter().find(|l| &l.id == id));
            let project = match list {
                Some(list) => list.project_id.as_ref(),
                None => context.project,
            };
            status_candidates(context.statuses, project)
                .into_iter()
                .map(|s| (s.name.clone(), String::new(), Choice::Status(s.id.clone())))
                .collect()
        }
        Some(Command::Location) => context
            .lists
            .iter()
            .map(|l| (l.name.clone(), String::new(), Choice::List(l.id.clone())))
            .collect(),
        _ => Vec::new(),
    };
    if !query.is_empty() {
        match section {
            Some(Command::Due) => {
                if let Ok(date) = parse_flexible_date(query, today) {
                    entries.insert(0, (query.into(), date.to_string(), Choice::Due(Some(date))));
                }
            }
            Some(Command::Estimate) => {
                if let Some(minutes) = parse_duration_minutes(query).filter(|m| *m > 0) {
                    entries.insert(
                        0,
                        (
                            query.into(),
                            format!("{minutes}分"),
                            Choice::Minutes(Some(minutes)),
                        ),
                    );
                }
            }
            _ => {}
        }
    }
    let query = query.to_lowercase();
    entries
        .into_iter()
        .filter(|(label, hint, _)| format!("{label} {hint}").to_lowercase().contains(&query))
        .collect()
}

/// Shared title field and command palette for inline capture and the modal.
pub(super) fn title_input(
    ui: &mut egui::Ui,
    title: &mut String,
    menu: &mut CommandMenu,
    props: &mut Properties,
    context: &CaptureContext<'_>,
    expanded: bool,
) -> (egui::Response, bool) {
    let id = ui.make_persistent_id("capture_title");
    let mut choose = false;
    let mut navigate = false;
    let mut submit = false;
    let query = slash_query(title).map(|(_, q)| q.to_owned());
    let above_modal = ui.memory(|m| m.is_above_modal_layer(ui.layer_id()));
    let active = above_modal && query.is_some() && menu.dismissed.as_ref() != Some(title);
    if ui.memory(|m| m.has_focus(id)) && active {
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                menu.selected += 1;
                navigate = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                menu.selected = menu.selected.saturating_sub(1);
                navigate = true;
            }
            choose = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                menu.dismissed = Some(title.clone());
            }
        });
    } else if !expanded && ui.memory(|m| m.has_focus(id)) {
        submit = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    }
    let response = ui.add_sized(
        [ui.available_width(), if expanded { 44.0 } else { 28.0 }],
        TextEdit::singleline(title)
            .id(id)
            .frame(egui::Frame::NONE)
            .font(egui::FontId::proportional(if expanded {
                23.0
            } else {
                14.0
            }))
            .hint_text(if expanded {
                "タスク名、または「/」でコマンドを入力"
            } else {
                "タスクを追加  ·  / で設定"
            }),
    );
    let Some((start, query)) = slash_query(title) else {
        *menu = CommandMenu::default();
        return (response, submit);
    };
    if menu.dismissed.as_ref() == Some(title) {
        return (response, false);
    }
    if !above_modal || (!response.has_focus() && !menu.open) {
        return (response, submit);
    }
    menu.open = true;
    if menu.last_query != query {
        menu.selected = 0;
        menu.last_query = query.into();
    }
    let mut entries = choices(menu.section, query, props, context);
    menu.selected = menu.selected.min(entries.len().saturating_sub(1));
    let mut picked = choose
        .then_some(menu.selected)
        .filter(|i| *i < entries.len());
    let mut open = true;
    egui::Popup::from_response(&response)
        .id(id.with("commands"))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .style(components::menu_style)
        .width(340.0)
        .gap(6.0)
        .show(|ui| {
            ui.set_width(340.0);
            ui.horizontal(|ui| {
                if menu.section.is_some()
                    && components::icon_button(
                        ui,
                        '\u{e5c4}',
                        "コマンド一覧に戻る",
                        context.palette,
                    )
                    .clicked()
                {
                    menu.section = None;
                    menu.selected = 0;
                }
                ui.label(
                    regular_text(match menu.section {
                        Some(Command::Due) => "期日を設定",
                        Some(Command::Estimate) => "見積時間を設定",
                        Some(Command::Status) => "ステータスを設定",
                        Some(Command::Location) => "所属リストを選択",
                        _ => "タスクアクション",
                    })
                    .size(12.0)
                    .color(context.palette.muted),
                );
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(255.0)
                .show(ui, |ui| {
                    for (i, (label, hint, _)) in entries.iter().enumerate() {
                        let row = components::selection_row(
                            ui,
                            label,
                            hint,
                            menu.selected == i,
                            context.palette,
                        );
                        if menu.selected == i && navigate {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            picked = Some(i);
                        }
                    }
                    if entries.is_empty() {
                        ui.label("該当するコマンドがありません");
                    }
                });
            ui.separator();
            ui.label(
                regular_text("↑ ↓ 選択    Enter 適用    Esc 閉じる")
                    .size(10.0)
                    .color(context.palette.muted),
            );
        });
    // Arrow/Tab navigation must keep typing in the title, not focus the next field.
    if open {
        response.request_focus();
    }
    if let Some(index) = picked {
        let (_, _, choice) = entries.remove(index);
        let completed = match choice {
            Choice::Command(Command::Today) => {
                props.due = Some(context.today);
                true
            }
            Choice::Command(Command::Tomorrow) => {
                props.due = context.today.next_day();
                true
            }
            Choice::Command(section) => {
                menu.section = Some(section);
                false
            }
            Choice::Due(date) => {
                props.due = date;
                true
            }
            Choice::Minutes(minutes) => {
                props.minutes = minutes;
                true
            }
            Choice::Status(status) => {
                props.status = Some(status);
                true
            }
            Choice::List(list) => {
                props.list = Some(list);
                props.status = None;
                true
            }
        };
        title.truncate(start);
        if completed {
            *title = title.trim_end().into();
            *menu = CommandMenu::default();
        } else {
            title.push('/');
            menu.selected = 0;
            menu.last_query.clear();
        }
        response.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(title.chars().count()),
                )));
            state.store(ui.ctx(), id);
        }
    } else if !open {
        menu.dismissed = Some(title.clone());
        menu.open = false;
    }
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            },
        )
    });
    (response, false)
}

pub(super) fn properties(
    ui: &mut egui::Ui,
    props: &mut Properties,
    context: &CaptureContext<'_>,
    show_location: bool,
) {
    ui.horizontal_wrapped(|ui| {
        let project = match props
            .list
            .as_ref()
            .and_then(|id| context.lists.iter().find(|l| &l.id == id))
        {
            Some(list) => list.project_id.as_ref(),
            None => context.project,
        };
        let label = props
            .status
            .as_ref()
            .and_then(|id| context.statuses.iter().find(|s| &s.id == id))
            .map(|s| s.name.as_str())
            .unwrap_or("未着手");
        let status = components::property_chip(ui, '\u{e837}', label, context.palette);
        components::popup(&status).show(|ui| {
            for status in status_candidates(context.statuses, project) {
                if ui
                    .selectable_label(props.status.as_ref() == Some(&status.id), &status.name)
                    .clicked()
                {
                    props.status = Some(status.id.clone());
                    ui.close();
                }
            }
        });
        let date_label = props
            .due
            .map(|d| format!("{}/{}", d.month() as u8, d.day()))
            .unwrap_or_else(|| "期日".into());
        let date_button = components::property_chip(ui, '\u{e935}', &date_label, context.palette);
        if let Some(date) = date_picker::show(
            ui,
            &date_button,
            props.due,
            context.today,
            "期日",
            context.palette,
        ) {
            props.due = date;
        }
        let minutes_label = props
            .minutes
            .map(|m| format!("{m}分"))
            .unwrap_or_else(|| "見積時間".into());
        let minutes_button =
            components::property_chip(ui, '\u{e425}', &minutes_label, context.palette);
        if let Some(minutes) = task_fields::estimate_picker(
            ui,
            &minutes_button,
            ui.make_persistent_id("estimate"),
            props.minutes,
            context.palette,
        ) {
            props.minutes = minutes;
        }
        if show_location {
            location_button(ui, props, context);
        }
    });
}

fn location_button(ui: &mut egui::Ui, props: &mut Properties, context: &CaptureContext<'_>) {
    let name = props
        .list
        .as_ref()
        .and_then(|id| context.lists.iter().find(|l| &l.id == id))
        .map(|l| l.name.as_str())
        .unwrap_or("既定のリスト");
    let location = components::property_chip(ui, ICON_FOLDER, name, context.palette);
    components::popup(&location).show(|ui| {
        ui.set_min_width(200.0);
        ui.label(
            regular_text("所属リスト")
                .size(11.0)
                .color(context.palette.muted),
        );
        for list in context.lists {
            if ui
                .selectable_label(props.list.as_ref() == Some(&list.id), &list.name)
                .clicked()
            {
                props.list = Some(list.id.clone());
                props.status = None;
                ui.close();
            }
        }
    });
}

impl MnemaGuiApp {
    pub(super) fn open_task_capture(&mut self, ctx: &egui::Context) {
        let capture = &mut self.workspace_ui.capture;
        if !capture.initialized {
            capture.initialized = true;
            capture.project = (self.view == View::Projects)
                .then(|| self.selected_project_id.clone())
                .flatten();
            capture.properties.list = self
                .lists
                .iter()
                .find(|l| {
                    if let Some(id) = &capture.project {
                        l.project_id.as_ref() == Some(id)
                    } else {
                        l.kind == ListKind::Inbox
                    }
                })
                .map(|l| l.id.clone());
        }
        capture.open = true;
        capture.focus_title = true;
        egui::Popup::close_all(ctx);
    }

    pub(super) fn show_task_capture(&mut self, ctx: &egui::Context, palette: Palette) {
        if !self.workspace_ui.capture.open {
            return;
        }
        let mut draft = std::mem::take(&mut self.workspace_ui.capture);
        let mut submit = false;
        let modal = egui::Modal::new(egui::Id::new("create_task"))
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(24))
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 96.0).clamp(400.0, 760.0));
                ui.horizontal(|ui| {
                    ui.label(material_icon_text(ICON_CHECK, 19.0, palette.accent));
                    ui.label(bold_text("タスクを作成").size(17.0));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if components::icon_button(
                            ui,
                            '\u{e5cd}',
                            "閉じる · 入力は保持されます",
                            palette,
                        )
                        .clicked()
                        {
                            draft.open = false;
                        }
                        ui.label(regular_text("Ctrl + T").size(11.0).color(palette.muted));
                    });
                });
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(20.0);
                let context = CaptureContext {
                    today: self.now_in_timezone().date(),
                    palette,
                    lists: &self.lists,
                    statuses: &self.statuses,
                    project: draft.project.as_ref(),
                };
                ui.horizontal(|ui| {
                    location_button(ui, &mut draft.properties, &context);
                    ui.label(regular_text(" / タスク").size(12.0).color(palette.muted));
                });
                ui.add_space(12.0);
                let (title, _) = title_input(
                    ui,
                    &mut draft.title,
                    &mut draft.commands,
                    &mut draft.properties,
                    &context,
                    true,
                );
                if draft.focus_title {
                    title.request_focus();
                    draft.focus_title = false;
                }
                ui.add_space(14.0);
                ui.add_sized(
                    [
                        ui.available_width(),
                        (ctx.content_rect().height() - 440.0).clamp(85.0, 180.0),
                    ],
                    TextEdit::multiline(&mut draft.description)
                        .id_salt("description")
                        .frame(egui::Frame::NONE)
                        .hint_text("説明を追加…"),
                );
                ui.add_space(18.0);
                properties(ui, &mut draft.properties, &context, false);
                ui.add_space(24.0);
                ui.separator();
                ui.add_space(12.0);
                if let Some(error) = &draft.error {
                    ui.colored_label(palette.error, error);
                }
                ui.horizontal(|ui| {
                    ui.checkbox(&mut draft.keep_open, "続けて作成");
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let valid =
                            !draft.title.trim().is_empty() && slash_query(&draft.title).is_none();
                        if ui
                            .add_enabled(
                                valid,
                                egui::Button::new(bold_text("タスクを作成").color(Color32::WHITE))
                                    .fill(palette.accent)
                                    .min_size(egui::vec2(132.0, 36.0)),
                            )
                            .clicked()
                        {
                            submit = true;
                        }
                        ui.label(regular_text("Ctrl + Enter").size(11.0).color(palette.muted));
                        if valid
                            && ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter)
                            })
                        {
                            submit = true;
                        }
                    });
                });
            });
        if modal.should_close() {
            draft.open = false;
        }
        if submit {
            match parse_quick_capture(&draft.title, self.now_in_timezone().date()) {
                Ok(mut request) => {
                    request.description = (!draft.description.trim().is_empty())
                        .then(|| draft.description.trim().to_owned());
                    request.due_date = draft.properties.due.or(request.due_date);
                    request.estimated_minutes =
                        draft.properties.minutes.or(request.estimated_minutes);
                    request.status_id = draft.properties.status.clone();
                    self.capture_task_in_location(
                        request,
                        draft.project.clone(),
                        draft.properties.list.clone(),
                    );
                    if let Some(error) = &self.error {
                        draft.error = Some(error.clone());
                    } else {
                        draft.title.clear();
                        draft.description.clear();
                        draft.properties.due = None;
                        draft.properties.minutes = None;
                        draft.error = None;
                        draft.commands = CommandMenu::default();
                        draft.open = draft.keep_open;
                        draft.initialized = draft.keep_open;
                        draft.focus_title = true;
                    }
                }
                Err(error) => draft.error = Some(error.to_string()),
            }
        }
        self.workspace_ui.capture = draft;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn frame(
        app: &mut MnemaGuiApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 720.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                app.handle_keyboard_shortcuts(ui.ctx());
                app.show_shell(ui, Palette::at(1.0));
                app.show_task_capture(ui.ctx(), Palette::at(1.0));
            },
        )
    }

    #[test]
    fn capture_shortcut_commands_draft_and_save_round_trip() {
        let (_directory, mut app, ctx) = interaction_tests::fixture();
        configure_style(&ctx, 1.0, true);
        // Ctrl+T must work even while another text field has focus.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.text_edit_singleline(&mut app.quick_capture)
                .request_focus();
        });
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::T, egui::Modifiers::CTRL)],
        );
        assert!(app.workspace_ui.capture.open);
        for _ in 0..3 {
            frame(&mut app, &ctx, vec![]);
        }
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Text("Review /due".into())],
        );
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(matches!(
            app.workspace_ui.capture.commands.section,
            Some(Command::Due)
        ));
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        );
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(
            app.workspace_ui.capture.properties.due,
            app.now_in_timezone().date().next_day()
        );
        assert_eq!(app.workspace_ui.capture.title, "Review");
        assert!(
            app.tasks.is_empty(),
            "Enter in the command menu must not submit"
        );
        frame(&mut app, &ctx, vec![egui::Event::Text(" /estimate".into())]);
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        );
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
        );
        assert_eq!(app.workspace_ui.capture.properties.minutes, Some(30));
        app.workspace_ui.capture.description = "Notes for review".into();
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(!app.workspace_ui.capture.open);
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::T, egui::Modifiers::CTRL)],
        );
        assert_eq!(app.workspace_ui.capture.title, "Review");
        assert_eq!(app.workspace_ui.capture.description, "Notes for review");
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Enter, egui::Modifiers::CTRL)],
        );
        assert!(app.error.is_none(), "{:?}", app.error);
        assert!(!app.workspace_ui.capture.open);
        assert_eq!(app.tasks.len(), 1);
        assert_eq!(app.tasks[0].title, "Review");
        assert_eq!(
            app.tasks[0].description.as_deref(),
            Some("Notes for review")
        );
        assert_eq!(app.tasks[0].estimated_minutes, Some(30));
        assert_eq!(
            app.tasks[0].due_date,
            app.now_in_timezone().date().next_day()
        );
        let vault = app.vault_clone().unwrap();
        let stored = app
            .runtime
            .block_on(vault.task_repo().find(app.tasks[0].id.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(stored, app.tasks[0]);
    }

    #[test]
    fn slash_query_does_not_interpret_paths_or_urls_as_commands() {
        assert_eq!(slash_query("確認 /期日"), Some((7, "期日")));
        assert_eq!(slash_query("https://example.com/due"), None);
        assert_eq!(slash_query("src/main.rs"), None);
        assert_eq!(slash_query("Review /due tomorrow"), None);
    }

    #[test]
    fn inline_commands_and_modal_do_not_steal_each_others_keyboard_focus() {
        let (_directory, mut app, ctx) = interaction_tests::fixture();
        configure_style(&ctx, 1.0, true);
        app.view = View::Inbox;
        let output = frame(&mut app, &ctx, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text().contains("タスクを追加  ·") => {
                    Some(t.pos + egui::vec2(12.0, 6.0))
                }
                _ => None,
            })
            .unwrap();
        frame(&mut app, &ctx, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Text("Inline /today".into())],
        );
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(app.tasks.is_empty());
        assert_eq!(app.quick_capture, "Inline");
        assert_eq!(
            app.workspace_ui.capture_due,
            app.now_in_timezone().date().to_string()
        );
        frame(&mut app, &ctx, vec![egui::Event::Text(" /".into())]);
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::T, egui::Modifiers::CTRL)],
        );
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![egui::Event::Text("Separate /".into())]);
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(
            app.workspace_ui.capture.open,
            "Escape closes commands first"
        );
        assert_eq!(app.workspace_ui.capture.title, "Separate /");
        assert_eq!(app.quick_capture, "Inline /");
        frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(!app.workspace_ui.capture.open);
    }

    #[test]
    fn creation_modal_and_commands_fit_small_windows_in_both_themes() {
        let (_directory, mut app, ctx) = interaction_tests::fixture();
        for dark in [false, true] {
            let factor = if dark { 1.0 } else { 0.0 };
            configure_style(&ctx, factor, dark);
            for (width, height) in [(860.0, 620.0), (1360.0, 950.0), (2560.0, 1440.0)] {
                app.open_task_capture(&ctx);
                for title in ["", "Review /", "Review /status"] {
                    app.workspace_ui.capture.title = title.into();
                    app.workspace_ui.capture.commands = CommandMenu::default();
                    app.workspace_ui.capture.focus_title = true;
                    for _ in 0..3 {
                        let bounds =
                            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
                        let output = ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(bounds),
                                ..Default::default()
                            },
                            |ui| app.show_task_capture(ui.ctx(), Palette::at(factor)),
                        );
                        for shape in output.shapes {
                            if let egui::Shape::Rect(rect) = &shape.shape {
                                assert_ne!(
                                    rect.stroke.color,
                                    Color32::RED,
                                    "{width}/{height}/{title}: {:?}",
                                    rect.rect
                                );
                            }
                            if let egui::Shape::Text(text) = &shape.shape {
                                assert!(
                                    bounds.expand(1.0).contains_rect(egui::Rect::from_min_size(
                                        text.pos,
                                        text.galley.size()
                                    )),
                                    "{width}/{height}: {} at {:?}",
                                    text.galley.text(),
                                    text.pos
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
