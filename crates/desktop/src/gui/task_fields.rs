use super::*;

pub(super) fn date_field(
    ui: &mut egui::Ui,
    id: egui::Id,
    current: Option<Date>,
    today: Date,
    palette: Palette,
) -> Option<Option<Date>> {
    let label = current
        .map(|d| format!("{}/{}", d.month() as u8, d.day()))
        .unwrap_or_else(|| "期限なし".into());
    let response = ui
        .push_id(id, |ui| {
            ui.add_sized(
                [75.0, 28.0],
                egui::Button::new(regular_text(label).size(12.0).color(
                    if current.is_some_and(|d| d < today) {
                        palette.error
                    } else {
                        palette.muted
                    },
                ))
                .frame(false),
            )
        })
        .inner
        .on_hover_text("期限を変更");
    date_picker::show(ui, &response, current, today, "期限", palette)
}

pub(super) fn estimate_field(
    ui: &mut egui::Ui,
    id: egui::Id,
    current: Option<u32>,
    palette: Palette,
) -> Option<Option<u32>> {
    let label = current
        .map(|m| format!("{m}分"))
        .unwrap_or_else(|| "見積なし".into());
    let response = ui
        .push_id(id, |ui| {
            ui.add_sized(
                [66.0, 28.0],
                egui::Button::new(regular_text(label).size(12.0).color(palette.muted)).frame(false),
            )
        })
        .inner
        .on_hover_text("見積時間を変更");
    estimate_picker(ui, &response, id, current, palette)
}

pub(super) fn estimate_picker(
    ui: &mut egui::Ui,
    response: &egui::Response,
    id: egui::Id,
    current: Option<u32>,
    palette: Palette,
) -> Option<Option<u32>> {
    let state_id = id.with("estimate_input");
    if response.clicked() {
        ui.data_mut(|data| {
            data.insert_temp(
                state_id,
                current.map(|m| format!("{m}m")).unwrap_or_default(),
            )
        });
    }
    let mut input = ui
        .data(|data| data.get_temp::<String>(state_id))
        .unwrap_or_default();
    let mut selected = None;
    components::popup(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(230.0);
            ui.label(bold_text("見積時間"));
            ui.horizontal_wrapped(|ui| {
                for minutes in [15, 30, 45, 60, 90, 120] {
                    if ui
                        .selectable_label(current == Some(minutes), format!("{minutes}分"))
                        .clicked()
                    {
                        selected = Some(Some(minutes));
                    }
                }
            });
            ui.separator();
            let edit = ui.add(
                TextEdit::singleline(&mut input)
                    .hint_text("45m / 1h 30m")
                    .desired_width(f32::INFINITY),
            );
            let minutes = parse_duration_minutes(&input).filter(|m| *m > 0);
            let enter = edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(minutes.is_some(), egui::Button::new("適用"))
                    .clicked()
                    || (enter && minutes.is_some())
                {
                    selected = Some(minutes);
                }
                if ui.button("クリア").clicked() {
                    selected = Some(None);
                }
            });
            if !input.is_empty() && minutes.is_none() {
                ui.colored_label(palette.error, "1分以上の時間を入力してください");
            }
            if selected.is_some() {
                ui.close();
            }
        });
    ui.data_mut(|data| data.insert_temp(state_id, input));
    selected
}
