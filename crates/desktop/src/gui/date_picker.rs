use super::*;

#[derive(Clone)]
struct State {
    month: Date,
    input: String,
    error: bool,
}

pub(super) fn show(
    ui: &mut egui::Ui,
    response: &egui::Response,
    current: Option<Date>,
    today: Date,
    label: &str,
    palette: Palette,
) -> Option<Option<Date>> {
    let id = response.id.with("date_picker");
    let initial = || State {
        month: first_day_of_month(current.unwrap_or(today)).unwrap_or(today),
        input: current.map(|d| d.to_string()).unwrap_or_default(),
        error: false,
    };
    let mut state = if response.clicked() {
        initial()
    } else {
        ui.data(|data| data.get_temp::<State>(id))
            .unwrap_or_else(initial)
    };
    let mut selected = None;
    components::popup(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(469.0);
            ui.label(regular_text(label).size(12.0).color(palette.muted));
            let input = ui.add_sized(
                [ui.available_width(), 36.0],
                text_field(&mut state.input, "日付を入力  ·  YYYY-MM-DD / 明日"),
            );
            if input.changed() {
                state.error = false;
                if let Ok(Some(date)) = parse_optional_date_with_today(&state.input, today) {
                    state.month = first_day_of_month(date).unwrap_or(date);
                }
            }
            if input.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                match parse_optional_date_with_today(&state.input, today) {
                    Ok(value) => selected = Some(value),
                    Err(_) => state.error = true,
                }
            }
            if state.error {
                ui.colored_label(palette.error, "日付を確認してください");
            }
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.allocate_ui_with_layout(
                    egui::vec2(164.0, 256.0),
                    egui::Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(164.0);
                        let next_week = add_days(calendar_grid_start(today), 7).unwrap_or(today);
                        let saturday = add_days(
                            today,
                            (5 - today.weekday().number_days_from_monday() as i32).rem_euclid(7),
                        )
                        .unwrap_or(today);
                        for (index, (label, date)) in [
                            ("今日", today),
                            ("明日", today.next_day().unwrap_or(today)),
                            ("今週末", saturday),
                            ("来週", next_week),
                            ("来週末", add_days(next_week, 5).unwrap_or(today)),
                            ("2週間後", add_days(today, 14).unwrap_or(today)),
                            ("4週間後", add_days(today, 28).unwrap_or(today)),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            let hint = if index < 2 {
                                ["月", "火", "水", "木", "金", "土", "日"]
                                    [date.weekday().number_days_from_monday() as usize]
                                    .to_string()
                            } else {
                                format!("{}/{}", date.month() as u8, date.day())
                            };
                            if components::selection_row(
                                ui,
                                label,
                                &hint,
                                current == Some(date),
                                palette,
                            )
                            .clicked()
                            {
                                selected = Some(Some(date));
                            }
                        }
                    },
                );
                let (separator, _) =
                    ui.allocate_exact_size(egui::vec2(1.0, 264.0), egui::Sense::hover());
                ui.painter().vline(
                    separator.center().x,
                    separator.y_range(),
                    Stroke::new(1.0_f32, palette.border),
                );
                ui.vertical(|ui| {
                    ui.set_width(280.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(280.0, 32.0),
                        egui::Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(
                                bold_text(format!(
                                    "{}年{}月",
                                    state.month.year(),
                                    state.month.month() as u8
                                ))
                                .size(14.0),
                            );
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                if components::icon_button(ui, '\u{e5cc}', "翌月", palette)
                                    .clicked()
                                {
                                    state.month = adjacent_month(state.month, 1);
                                }
                                if components::icon_button(ui, '\u{e5cb}', "前月", palette)
                                    .clicked()
                                {
                                    state.month = adjacent_month(state.month, -1);
                                }
                                if components::button(ui, "今月", false, palette)
                                    .on_hover_text("今日の月を表示")
                                    .clicked()
                                {
                                    state.month = first_day_of_month(today).unwrap_or(today);
                                }
                            });
                        },
                    );
                    let (grid, _) =
                        ui.allocate_exact_size(egui::vec2(280.0, 228.0), egui::Sense::hover());
                    let start = add_days(
                        state.month,
                        -(state.month.weekday().number_days_from_sunday() as i32),
                    )
                    .unwrap_or(state.month);
                    for (i, label) in ["日", "月", "火", "水", "木", "金", "土"]
                        .iter()
                        .enumerate()
                    {
                        ui.painter().text(
                            grid.min + egui::vec2(i as f32 * 40.0 + 20.0, 10.0),
                            egui::Align2::CENTER_CENTER,
                            label,
                            egui::FontId::proportional(12.0),
                            palette.muted,
                        );
                    }
                    for i in 0..42 {
                        let Some(date) = add_days(start, i) else {
                            continue;
                        };
                        let rect = egui::Rect::from_min_size(
                            grid.min
                                + egui::vec2(
                                    (i % 7) as f32 * 40.0 + 4.0,
                                    (i / 7) as f32 * 34.0 + 24.0,
                                ),
                            egui::vec2(32.0, 32.0),
                        );
                        let day = ui.interact(rect, id.with(date), egui::Sense::click());
                        let hover = ui.ctx().animate_bool_with_time(
                            day.id.with("hover"),
                            day.hovered(),
                            components::HOVER_SECONDS,
                        );
                        let is_selected = current == Some(date);
                        ui.painter().rect_filled(
                            rect,
                            6.0,
                            if is_selected {
                                palette.selected_fill
                            } else {
                                palette.control_hover.gamma_multiply(hover)
                            },
                        );
                        if date == today {
                            ui.painter()
                                .circle_filled(rect.center(), 13.0, palette.error);
                        }
                        if is_selected {
                            ui.painter().rect_stroke(
                                rect,
                                6.0,
                                Stroke::new(1.0_f32, palette.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            date.day(),
                            egui::FontId::proportional(13.0),
                            if date == today {
                                Color32::WHITE
                            } else if is_selected {
                                palette.selected_text
                            } else if date.month() == state.month.month() {
                                palette.text
                            } else {
                                palette.muted.gamma_multiply(0.6)
                            },
                        );
                        day.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                true,
                                is_selected,
                                date.to_string(),
                            )
                        });
                        if day
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            selected = Some(Some(date));
                        }
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                if components::button(ui, "日付をクリア", false, palette).clicked() {
                    selected = Some(None);
                }
                let (hint, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 32.0),
                    egui::Sense::hover(),
                );
                ui.painter().text(
                    hint.right_center(),
                    egui::Align2::RIGHT_CENTER,
                    "日付を選択 · Escで閉じる",
                    egui::FontId::proportional(11.0),
                    palette.muted,
                );
            });
            if selected.is_some() {
                ui.close();
            }
        });
    ui.data_mut(|data| data.insert_temp(id, state));
    selected
}

fn adjacent_month(date: Date, direction: i32) -> Date {
    let mut text = date.to_string();
    shift_month(&mut text, direction);
    parse_required_date(&text).unwrap_or(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_fits_screen_edges_and_hover_does_not_commit_a_date() {
        let today = parse_required_date("2026-09-21").unwrap();
        for dark in [false, true] {
            for width in [860.0, 1360.0] {
                let ctx = egui::Context::default();
                configure_fonts(&ctx);
                let factor = if dark { 1.0 } else { 0.0 };
                configure_style(&ctx, factor, dark);
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 650.0));
                let mut selected = None;
                let mut frame = |events: Vec<egui::Event>| {
                    let output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            egui::Area::new(egui::Id::new("picker_anchor"))
                                .fixed_pos(egui::pos2(width - 100.0, 590.0))
                                .show(ui.ctx(), |ui| {
                                    selected = task_fields::date_field(
                                        ui,
                                        egui::Id::new("test_due"),
                                        Some(today),
                                        today,
                                        Palette::at(factor),
                                    );
                                });
                        },
                    );
                    (output, selected)
                };
                let position = |output: &egui::FullOutput, label: &str| {
                    output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == label => {
                                Some(text.pos + egui::vec2(4.0, 6.0))
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("Missing {label}"))
                };
                let press = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                frame(vec![]);
                let (initial, _) = frame(vec![]);
                let anchor = position(&initial, "9/21");
                frame(vec![egui::Event::PointerMoved(anchor)]);
                frame(vec![press(anchor, true)]);
                frame(vec![press(anchor, false)]);
                let (popup, _) = frame(vec![]);
                assert!(egui::Popup::is_any_open(&ctx), "open at {anchor:?}");
                for shape in &popup.shapes {
                    if let egui::Shape::Text(text) = &shape.shape {
                        let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                        assert!(
                            screen.expand(1.0).contains_rect(rect),
                            "{width} {dark}: {} at {rect:?}",
                            text.galley.text()
                        );
                    }
                }
                for label in [
                    "今日",
                    "明日",
                    "今週末",
                    "来週",
                    "4週間後",
                    "日付をクリア",
                    "2026年9月",
                ] {
                    let pos = position(&popup, label);
                    assert!(screen.contains(pos), "{label} clipped at {pos:?}");
                    let (hovered, result) = frame(vec![egui::Event::PointerMoved(pos)]);
                    assert!(result.is_none(), "hover must not commit a date");
                    assert!(!hovered.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.stroke.color == Color32::RED)));
                }
                let tomorrow = position(&popup, "明日");
                frame(vec![egui::Event::PointerMoved(tomorrow)]);
                frame(vec![press(tomorrow, true)]);
                let (_, result) = frame(vec![press(tomorrow, false)]);
                assert_eq!(result, Some(today.next_day()));
                assert!(!egui::Popup::is_any_open(&ctx));
            }
        }
    }
}
