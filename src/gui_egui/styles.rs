use egui::{Color32, Style, Visuals, Stroke};

// 主色调 - 现代柔和紫色主题
pub const PRIMARY_COLOR: Color32        = Color32::from_rgb(108, 92, 231);
pub const PRIMARY_LIGHT: Color32        = Color32::from_rgb(139, 92, 246);
pub const PRIMARY_DARK: Color32         = Color32::from_rgb(79, 70, 229);

// 状态色 - 更柔和的配色
pub const SUCCESS_COLOR: Color32        = Color32::from_rgb(16, 124, 16);
pub const SUCCESS_LIGHT: Color32        = Color32::from_rgb(220, 252, 231);
pub const DANGER_COLOR: Color32         = Color32::from_rgb(185, 28, 28);
pub const DANGER_LIGHT: Color32         = Color32::from_rgb(254, 226, 226);
pub const WARNING_COLOR: Color32        = Color32::from_rgb(161, 98, 7);
pub const WARNING_LIGHT: Color32        = Color32::from_rgb(254, 249, 195);
pub const INFO_COLOR: Color32           = Color32::from_rgb(8, 102, 135);
pub const INFO_LIGHT: Color32           = Color32::from_rgb(225, 243, 252);

// 文本色 - 更清晰的深色
pub const TEXT_PRIMARY_COLOR: Color32   = Color32::from_rgb(30, 41, 59);
pub const TEXT_SECONDARY_COLOR: Color32 = Color32::from_rgb(55, 65, 81);
pub const TEXT_MUTED_COLOR: Color32     = Color32::from_rgb(75, 85, 99);

// 背景色 - 更温暖的白色
pub const BG_PRIMARY: Color32           = Color32::from_rgb(249, 250, 251);
pub const BG_SECONDARY: Color32         = Color32::from_rgb(243, 244, 246);
pub const BG_CARD: Color32              = Color32::WHITE;
pub const BG_HEADER: Color32            = Color32::from_rgb(79, 70, 229);

// 边框色 - 更柔和的边框
pub const BORDER_COLOR: Color32         = Color32::from_rgb(229, 231, 235);
pub const BORDER_LIGHT: Color32         = Color32::from_rgb(243, 244, 246);

// 字体大小
pub const FONT_SIZE_XL: f32             = 24.0;
pub const FONT_SIZE_LG: f32             = 18.0;
pub const FONT_SIZE_MD: f32             = 15.0;
pub const FONT_SIZE_SM: f32             = 13.0;
pub const FONT_SIZE_XS: f32             = 11.0;

// 间距
pub const SPACING_XL: f32               = 32.0;
pub const SPACING_LG: f32               = 24.0;
pub const SPACING_MD: f32               = 16.0;
pub const SPACING_SM: f32               = 12.0;
pub const SPACING_XS: f32               = 8.0;

pub fn get_custom_style() -> Style {
    let mut style = Style::default();
    let mut visuals = Visuals::light();

    // 文本颜色
    visuals.override_text_color = Some(TEXT_PRIMARY_COLOR);
    
    // 背景色
    visuals.panel_fill  = BG_PRIMARY;
    visuals.window_fill = BG_CARD;
    visuals.window_stroke = Stroke::new(1.0, BORDER_COLOR);
    visuals.extreme_bg_color = BG_SECONDARY;

    // 非交互状态
    visuals.widgets.noninteractive.bg_fill   = BG_CARD;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY_COLOR);

    // 非激活状态 - 修复输入框文字颜色
    visuals.widgets.inactive.bg_fill   = BG_CARD;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY_COLOR);
    visuals.widgets.inactive.weak_bg_fill = BG_SECONDARY;

    // 悬停状态 - 更柔和的悬停效果
    visuals.widgets.hovered.bg_fill   = Color32::from_rgb(246, 247, 248);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT_PRIMARY_COLOR);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(238, 239, 241);

    // 激活状态
    visuals.widgets.active.bg_fill   = PRIMARY_LIGHT;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.weak_bg_fill = PRIMARY_COLOR;

    // 选中状态
    visuals.selection.bg_fill = Color32::from_rgb(229, 221, 255);
    visuals.selection.stroke  = Stroke::new(2.0, PRIMARY_COLOR);

    // 打开状态
    visuals.widgets.open.bg_fill = BG_CARD;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY_COLOR);

    // 窗口阴影
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(30),
    };

    style.visuals = visuals;
    
    // 增大间距，使界面更舒适
    style.spacing.item_spacing      = egui::vec2(14.0, 12.0);
    style.spacing.button_padding    = egui::vec2(18.0, 10.0);
    style.spacing.window_margin     = egui::Margin::same(20);
    style.spacing.menu_margin       = egui::Margin::same(12);
    style.spacing.indent            = 24.0;
    style.spacing.icon_width        = 20.0;
    style.spacing.icon_width_inner  = 16.0;
    style.spacing.icon_spacing      = 10.0;
    
    // 圆角设置
    style.visuals.window_corner_radius = egui::CornerRadius::same(8);
    
    // 文本样式
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

// 卡片样式
pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, BORDER_COLOR))
        .inner_margin(egui::Margin::same(16))
        .corner_radius(egui::CornerRadius::same(8))
}

// 卡片样式 - 无边框
pub fn card_frame_flat() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_CARD)
        .inner_margin(egui::Margin::same(16))
        .corner_radius(egui::CornerRadius::same(8))
}

// 信息卡片样式
pub fn info_card_frame(color: Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(color)
        .stroke(Stroke::new(1.0, color))
        .inner_margin(egui::Margin::same(16))
        .corner_radius(egui::CornerRadius::same(8))
}

// 输入框样式
pub fn input_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0, BORDER_COLOR))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .corner_radius(egui::CornerRadius::same(6))
}

// 按钮样式 - 主要
pub fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .size(FONT_SIZE_MD)
            .color(Color32::WHITE)
            .strong()
    )
    .fill(PRIMARY_COLOR)
    .corner_radius(egui::CornerRadius::same(6))
}

// 按钮样式 - 成功
pub fn success_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .size(FONT_SIZE_MD)
            .color(Color32::WHITE)
            .strong()
    )
    .fill(SUCCESS_COLOR)
    .corner_radius(egui::CornerRadius::same(6))
}

// 按钮样式 - 危险
pub fn danger_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .size(FONT_SIZE_MD)
            .color(Color32::WHITE)
            .strong()
    )
    .fill(DANGER_COLOR)
    .corner_radius(egui::CornerRadius::same(6))
}

// 按钮样式 - 次要
pub fn secondary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(text)
            .size(FONT_SIZE_MD)
            .color(TEXT_PRIMARY_COLOR)
    )
    .fill(BG_SECONDARY)
    .stroke(Stroke::new(1.0, BORDER_COLOR))
    .corner_radius(egui::CornerRadius::same(6))
}
