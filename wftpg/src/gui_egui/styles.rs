use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Style, Visuals};

pub const BASE_FONT_SIZE: f32 = 14.0;

pub const FONT_SCALE: f32 = 1.0;

pub const FONT_SIZE_XL: f32 = BASE_FONT_SIZE * FONT_SCALE * 1.71;
pub const FONT_SIZE_LG: f32 = BASE_FONT_SIZE * FONT_SCALE * 1.29;
pub const FONT_SIZE_MD: f32 = BASE_FONT_SIZE * FONT_SCALE * 1.07;
pub const FONT_SIZE_SM: f32 = BASE_FONT_SIZE * FONT_SCALE * 0.93;
pub const FONT_SIZE_XS: f32 = BASE_FONT_SIZE * FONT_SCALE * 0.79;

pub const PRIMARY_COLOR: Color32 = Color32::from_rgb(108, 92, 231);
pub const PRIMARY_LIGHT: Color32 = Color32::from_rgb(139, 92, 246);

pub const SUCCESS_COLOR: Color32 = Color32::from_rgb(22, 163, 74);
pub const SUCCESS_LIGHT: Color32 = Color32::from_rgb(220, 252, 231);
pub const SUCCESS_DARK: Color32 = Color32::from_rgb(21, 128, 61);
pub const DANGER_COLOR: Color32 = Color32::from_rgb(220, 38, 38);
pub const DANGER_LIGHT: Color32 = Color32::from_rgb(254, 226, 226);
pub const DANGER_DARK: Color32 = Color32::from_rgb(185, 28, 28);
pub const WARNING_COLOR: Color32 = Color32::from_rgb(180, 83, 9);
pub const WARNING_LIGHT: Color32 = Color32::from_rgb(254, 249, 195);
pub const WARNING_BORDER: Color32 = Color32::from_rgb(234, 179, 8);
pub const INFO_COLOR: Color32 = Color32::from_rgb(14, 116, 144);
pub const INFO_LIGHT: Color32 = Color32::from_rgb(207, 250, 254);

pub const TEXT_PRIMARY_COLOR: Color32 = Color32::from_rgb(17, 24, 39);
pub const TEXT_SECONDARY_COLOR: Color32 = Color32::from_rgb(55, 65, 81);
pub const TEXT_MUTED_COLOR: Color32 = Color32::from_rgb(107, 114, 128);
pub const TEXT_LABEL_COLOR: Color32 = Color32::from_rgb(75, 85, 99);

pub const BG_PRIMARY: Color32 = Color32::from_rgb(249, 250, 251);
pub const BG_SECONDARY: Color32 = Color32::from_rgb(243, 244, 246);
pub const BG_CARD: Color32 = Color32::WHITE;
pub const BG_HEADER: Color32 = Color32::from_rgb(79, 70, 229);
pub const BG_INFO: Color32 = Color32::from_rgb(248, 250, 252);

pub const BORDER_COLOR: Color32 = Color32::from_rgb(209, 213, 219);
pub const BORDER_LIGHT: Color32 = Color32::from_rgb(229, 231, 235);

pub const SPACING_XL: f32 = 24.0;
pub const SPACING_LG: f32 = 16.0;
pub const SPACING_MD: f32 = 12.0;
pub const SPACING_SM: f32 = 8.0;
pub const SPACING_XS: f32 = 4.0;

pub fn get_custom_style() -> Style {
    let mut style = Style::default();
    let mut visuals = Visuals::light();

    visuals.override_text_color = Some(TEXT_PRIMARY_COLOR);

    visuals.panel_fill = BG_PRIMARY;
    visuals.window_fill = BG_CARD;
    visuals.window_stroke = Stroke::new(1.0, BORDER_COLOR);
    visuals.extreme_bg_color = BG_SECONDARY;

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY_COLOR);

    visuals.widgets.inactive.bg_fill = BG_CARD;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY_COLOR);
    visuals.widgets.inactive.weak_bg_fill = BG_SECONDARY;

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(243, 244, 246);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT_PRIMARY_COLOR);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(229, 231, 235);

    visuals.widgets.active.bg_fill = PRIMARY_LIGHT;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.weak_bg_fill = PRIMARY_COLOR;

    visuals.selection.bg_fill = Color32::from_rgb(229, 221, 255);
    visuals.selection.stroke = Stroke::new(2.0, PRIMARY_COLOR);

    visuals.widgets.open.bg_fill = BG_CARD;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY_COLOR);

    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(30),
    };

    style.visuals = visuals;

    style.spacing.item_spacing = egui::vec2(14.0, 12.0);
    style.spacing.button_padding = egui::vec2(18.0, 10.0);
    style.spacing.window_margin = egui::Margin::same(24);
    style.spacing.menu_margin = egui::Margin::same(16);
    style.spacing.indent = 24.0;
    style.spacing.icon_width = 20.0;
    style.spacing.icon_width_inner = 16.0;
    style.spacing.icon_spacing = 10.0;

    style.visuals.window_corner_radius = egui::CornerRadius::same(8);

    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(FONT_SIZE_LG, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(FONT_SIZE_MD, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(FONT_SIZE_MD, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(FONT_SIZE_SM, egui::FontFamily::Proportional),
    );

    style
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, BORDER_COLOR))
        .inner_margin(egui::Margin::same(20))
        .corner_radius(egui::CornerRadius::same(8))
}

pub fn info_card_frame(color: Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(color)
        .stroke(Stroke::new(1.0, color))
        .inner_margin(egui::Margin::same(16))
        .corner_radius(egui::CornerRadius::same(8))
}

pub fn input_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, BORDER_COLOR))
        .inner_margin(egui::Margin::same(0))
        .corner_radius(egui::CornerRadius::same(6))
}

pub fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .size(FONT_SIZE_MD)
            .color(Color32::WHITE)
            .strong(),
    )
    .fill(PRIMARY_COLOR)
    .corner_radius(egui::CornerRadius::same(6))
}

pub fn secondary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .size(FONT_SIZE_MD)
            .color(TEXT_PRIMARY_COLOR),
    )
    .fill(BG_SECONDARY)
    .stroke(Stroke::new(1.0, BORDER_COLOR))
    .corner_radius(egui::CornerRadius::same(6))
}

pub fn danger_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .size(FONT_SIZE_MD)
            .color(Color32::WHITE),
    )
    .fill(DANGER_DARK)
    .corner_radius(egui::CornerRadius::same(6))
}

pub fn small_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).size(FONT_SIZE_SM))
        .fill(BG_SECONDARY)
        .stroke(Stroke::new(1.0, BORDER_COLOR))
        .corner_radius(egui::CornerRadius::same(4))
}

pub fn status_message(ui: &mut egui::Ui, msg: &str, success: bool) {
    let (bg_color, text_color, icon) = if success {
        (SUCCESS_LIGHT, SUCCESS_COLOR, "√ ")
    } else {
        (DANGER_LIGHT, DANGER_COLOR, "×")
    };

    info_card_frame(bg_color).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).size(FONT_SIZE_MD).color(text_color));
            ui.label(RichText::new(msg).size(FONT_SIZE_MD).color(text_color));
        });
    });
}

pub fn page_header(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(FONT_SIZE_XL));
        ui.label(
            RichText::new(title)
                .size(FONT_SIZE_XL)
                .strong()
                .color(TEXT_PRIMARY_COLOR),
        );
    });
    ui.add_space(SPACING_SM);
}

pub fn section_header(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(FONT_SIZE_LG));
        ui.label(
            RichText::new(title)
                .size(FONT_SIZE_LG)
                .strong()
                .color(TEXT_PRIMARY_COLOR),
        );
    });
    ui.add_space(SPACING_SM);
}

pub fn empty_state(ui: &mut egui::Ui, icon: &str, title: &str, subtitle: &str) {
    card_frame().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(SPACING_LG);
            ui.label(
                RichText::new(icon)
                    .size(FONT_SIZE_LG * 1.5)
                    .color(TEXT_MUTED_COLOR),
            );
            ui.add_space(SPACING_MD);
            ui.label(
                RichText::new(title)
                    .size(FONT_SIZE_LG)
                    .color(TEXT_MUTED_COLOR),
            );
            ui.add_space(SPACING_SM);
            ui.label(
                RichText::new(subtitle)
                    .size(FONT_SIZE_MD)
                    .color(TEXT_LABEL_COLOR),
            );
            ui.add_space(SPACING_LG);
        });
    });
}

pub fn warning_box(ui: &mut egui::Ui, title: &str, notes: &[&str]) {
    Frame::new()
        .fill(WARNING_LIGHT)
        .stroke(Stroke::new(1.0, WARNING_BORDER))
        .inner_margin(Margin::same(16))
        .corner_radius(CornerRadius::same(8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(format!("⚠ {}", title))
                    .strong()
                    .size(FONT_SIZE_MD)
                    .color(WARNING_COLOR),
            );
            ui.add_space(SPACING_SM);

            for note in notes {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("•")
                            .size(FONT_SIZE_MD)
                            .color(TEXT_LABEL_COLOR),
                    );
                    ui.label(
                        RichText::new(*note)
                            .size(FONT_SIZE_MD)
                            .color(TEXT_SECONDARY_COLOR),
                    );
                });
            }
        });
}

pub fn form_row<F>(ui: &mut egui::Ui, label_text: &str, label_width: f32, add_content: F)
where
    F: FnOnce(&mut egui::Ui),
{
    ui.horizontal(|ui| {
        ui.add_sized(
            [label_width, 24.0],
            egui::Label::new(
                RichText::new(format!("{}:", label_text))
                    .size(FONT_SIZE_MD)
                    .color(TEXT_SECONDARY_COLOR),
            ),
        );
        add_content(ui);
    });
}

pub fn form_row_with_suffix<F>(
    ui: &mut egui::Ui,
    label_text: &str,
    label_width: f32,
    add_content: F,
    suffix: &str,
) where
    F: FnOnce(&mut egui::Ui),
{
    ui.horizontal(|ui| {
        ui.add_sized(
            [label_width, 24.0],
            egui::Label::new(
                RichText::new(format!("{}:", label_text))
                    .size(FONT_SIZE_MD)
                    .color(TEXT_SECONDARY_COLOR),
            ),
        );
        add_content(ui);
        ui.label(
            RichText::new(suffix)
                .size(FONT_SIZE_SM)
                .color(TEXT_MUTED_COLOR),
        );
    });
}

pub fn table_column_percent(
    available_width: f32,
    percent: f32,
    min_width: f32,
) -> egui_extras::Column {
    egui_extras::Column::initial(available_width * percent).at_least(min_width)
}

pub fn table_column_remainder(min_width: f32) -> egui_extras::Column {
    egui_extras::Column::remainder().at_least(min_width)
}
