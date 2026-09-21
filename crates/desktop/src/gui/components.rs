use super::*;

// Shared motion and geometry for controls across the workspace.
pub(super) const HOVER_SECONDS: f32 = 0.14;
pub(super) const PRESS_SECONDS: f32 = 0.09;
pub(super) const TOGGLE_SECONDS: f32 = 0.18;

pub(super) fn control_style(style: &mut egui::Style) {
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.menu_margin = egui::Margin::same(10);
    style.visuals.window_corner_radius = egui::CornerRadius::same(10);
    style.animation_time = HOVER_SECONDS;
    for visual in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        visual.corner_radius = egui::CornerRadius::same(6);
    }
}

pub(super) fn menu_style(style: &mut egui::Style) {
    control_style(style);
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.interact_size.y = 32.0;
    style.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    for visual in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        visual.bg_stroke = Stroke::NONE;
    }
}

pub(super) fn popup(response: &egui::Response) -> egui::Popup<'_> {
    egui::Popup::menu(response).style(menu_style)
}

pub(super) fn menu<'a, R>(
    ui: &mut egui::Ui,
    label: impl egui::IntoAtoms<'a>,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let config = egui::containers::menu::MenuConfig::new().style(menu_style);
    let (response, inner) = if egui::containers::menu::is_in_menu(ui) {
        egui::containers::menu::SubMenuButton::new(label)
            .config(config)
            .ui(ui, contents)
    } else {
        egui::containers::menu::MenuButton::new(label)
            .config(config)
            .ui(ui, contents)
    };
    egui::InnerResponse {
        response,
        inner: inner.map(|inner| inner.inner),
    }
}

pub(super) fn selection_row(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    selected: bool,
    palette: Palette,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        HOVER_SECONDS,
    );
    ui.painter().rect_filled(
        rect,
        6.0,
        if selected {
            palette.selected_fill
        } else {
            palette.control_hover.gamma_multiply(hover)
        },
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        palette.text,
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(10.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        hint,
        egui::FontId::proportional(11.0),
        palette.muted,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn icon_button(
    ui: &mut egui::Ui,
    icon: char,
    label: &str,
    palette: Palette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        HOVER_SECONDS,
    );
    let press = ui.ctx().animate_bool_with_time(
        response.id.with("press"),
        response.is_pointer_button_down_on(),
        PRESS_SECONDS,
    );
    ui.painter().rect_filled(
        rect.shrink(press),
        6.0,
        palette.control_hover.gamma_multiply(hover),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        material_icon_font(17.0 - press),
        if response.hovered() {
            palette.text
        } else {
            palette.muted
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn button(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    palette: Palette,
) -> egui::Response {
    let response = ui.add(
        egui::Button::new(regular_text(label).size(13.0))
            .fill(if selected {
                palette.selected_fill
            } else {
                Color32::TRANSPARENT
            })
            .corner_radius(6.0)
            .min_size(egui::vec2(0.0, 28.0)),
    );
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        HOVER_SECONDS,
    );
    ui.painter().rect_stroke(
        response.rect,
        6.0,
        Stroke::new(1.0_f32, palette.accent.gamma_multiply(hover * 0.4)),
        egui::StrokeKind::Inside,
    );
    response
}

pub(super) fn menu_button(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    palette: Palette,
) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(label.into(), egui::FontId::proportional(12.0), palette.text);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 30.0, 28.0),
        egui::Sense::click(),
    );
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        HOVER_SECONDS,
    );
    ui.painter().rect_filled(
        rect,
        5.0,
        if selected {
            palette.selected_fill
        } else {
            palette.control_hover.gamma_multiply(hover)
        },
    );
    ui.painter().galley(
        rect.left_center() + egui::vec2(8.0, -galley.size().y / 2.0),
        galley,
        palette.text,
    );
    let center = rect.right_center() - egui::vec2(10.0, 0.0);
    ui.painter().add(egui::Shape::line(
        vec![
            center + egui::vec2(-3.0, -1.5),
            center + egui::vec2(0.0, 1.5),
            center + egui::vec2(3.0, -1.5),
        ],
        Stroke::new(1.3_f32, palette.muted),
    ));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn property_chip(
    ui: &mut egui::Ui,
    icon: char,
    label: &str,
    palette: Palette,
) -> egui::Response {
    ui.add(
        egui::Button::new((
            material_icon_text(icon, 16.0, palette.muted),
            regular_text(label).size(12.0),
        ))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, palette.border))
        .corner_radius(6.0)
        .min_size(egui::vec2(0.0, 30.0)),
    )
}

pub(super) fn theme_toggle(ui: &mut egui::Ui, dark_mode: &mut bool, palette: Palette) {
    // Reserve the expanded width so hovering never moves adjacent controls.
    let (slot, response) = ui.allocate_exact_size(egui::vec2(76.0, 30.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *dark_mode, "ダークモード")
    });
    if response.clicked() {
        *dark_mode = !*dark_mode;
    }
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered(),
        HOVER_SECONDS,
    );
    let press = ui.ctx().animate_bool_with_time(
        response.id.with("press"),
        response.is_pointer_button_down_on(),
        PRESS_SECONDS,
    );
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("theme"), *dark_mode, TOGGLE_SECONDS);
    let width = 64.0 + 10.0 * hover - 3.0 * press;
    let rect = egui::Rect::from_center_size(slot.center(), egui::vec2(width, 28.0 - press));
    let painter = ui.painter();
    painter.rect(
        rect,
        15.0,
        mix_color(palette.switch_bg, palette.control_hover, hover * 0.4),
        Stroke::new(
            1.0_f32,
            mix_color(palette.border, palette.border_strong, hover),
        ),
        egui::StrokeKind::Inside,
    );
    let thumb_width = 28.0 + press * 4.0;
    let left = rect.left() + 3.0 + t * (width - 6.0 - thumb_width);
    painter.rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(left, rect.top() + 3.0),
            egui::vec2(thumb_width, rect.height() - 6.0),
        ),
        12.0,
        palette.selected_fill,
    );
    for (x, icon, active) in [
        (rect.left() + 17.0, ICON_LIGHT_MODE, !*dark_mode),
        (rect.right() - 17.0, ICON_DARK_MODE, *dark_mode),
    ] {
        painter.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::CENTER_CENTER,
            icon,
            material_icon_font(16.0 - press),
            if active { palette.text } else { palette.muted },
        );
    }
    response
        .on_hover_text(if *dark_mode {
            "ライトモードに切り替え"
        } else {
            "ダークモードに切り替え"
        })
        .on_hover_cursor(egui::CursorIcon::PointingHand);
}
