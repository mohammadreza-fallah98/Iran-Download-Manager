use crate::model::{Category, Part, Theme};
use egui::{pos2, vec2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui};

// ── Palette ─────────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub struct Palette {
    pub toolbar_bg:    Color32,
    pub sidebar_bg:    Color32,
    pub list_bg:       Color32,
    pub list_row_alt:  Color32,
    pub list_selected: Color32,
    pub list_sel_border: Color32,
    pub list_hover:    Color32,
    pub header_bg:     Color32,
    pub menu_bg:       Color32,
    pub status_bg:     Color32,
    pub border:        Color32,
    pub text:          Color32,
    pub text_dim:      Color32,
    pub text_header:   Color32,
    pub progress_fill: Color32,
    pub progress_bg:   Color32,
    pub seg_bar_bg:    Color32,
    pub folder_color:  Color32,
    pub arrow_color:   Color32,
}

pub fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Dark => Palette {
            toolbar_bg:      Color32::from_rgb(28, 32, 42),
            sidebar_bg:      Color32::from_rgb(22, 26, 35),
            list_bg:         Color32::from_rgb(32, 36, 48),
            list_row_alt:    Color32::from_rgb(28, 32, 44),
            list_selected:   Color32::from_rgb(0, 90, 160),
            list_sel_border: Color32::from_rgb(0, 140, 220),
            list_hover:      Color32::from_rgb(40, 50, 70),
            header_bg:       Color32::from_rgb(20, 24, 34),
            menu_bg:         Color32::from_rgb(28, 32, 42),
            status_bg:       Color32::from_rgb(20, 24, 34),
            border:          Color32::from_rgb(50, 60, 80),
            text:            Color32::from_rgb(220, 225, 235),
            text_dim:        Color32::from_rgb(130, 140, 160),
            text_header:     Color32::from_rgb(160, 170, 185),
            progress_fill:   Color32::from_rgb(0, 190, 120),
            progress_bg:     Color32::from_rgb(40, 44, 58),
            seg_bar_bg:      Color32::from_rgb(40, 44, 58),
            folder_color:    Color32::from_rgb(80, 150, 220),
            arrow_color:     Color32::from_rgb(90, 100, 120),
        },
        Theme::Light => Palette {
            toolbar_bg:      Color32::from_rgb(245, 247, 252),
            sidebar_bg:      Color32::WHITE,
            list_bg:         Color32::WHITE,
            list_row_alt:    Color32::from_rgb(248, 250, 255),
            list_selected:   Color32::from_rgb(204, 228, 247),
            list_sel_border: Color32::from_rgb(153, 209, 255),
            list_hover:      Color32::from_rgb(229, 243, 255),
            header_bg:       Color32::from_rgb(235, 240, 250),
            menu_bg:         Color32::WHITE,
            status_bg:       Color32::from_rgb(235, 240, 250),
            border:          Color32::from_rgb(200, 210, 225),
            text:            Color32::from_rgb(20, 25, 35),
            text_dim:        Color32::from_rgb(90, 100, 120),
            text_header:     Color32::from_rgb(60, 70, 90),
            progress_fill:   Color32::from_rgb(30, 160, 100),
            progress_bg:     Color32::from_rgb(220, 230, 245),
            seg_bar_bg:      Color32::WHITE,
            folder_color:    Color32::from_rgb(50, 120, 200),
            arrow_color:     Color32::from_rgb(90, 100, 120),
        },
    }
}

/// Build egui Visuals from the current theme.
pub fn make_visuals(theme: Theme) -> egui::Visuals {
    let p = palette(theme);
    match theme {
        Theme::Dark => {
            let mut v = egui::Visuals::dark();
            v.panel_fill       = p.list_bg;
            v.window_fill      = p.list_bg;
            v.extreme_bg_color = p.sidebar_bg;
            v.faint_bg_color   = p.list_row_alt;
            v.override_text_color = Some(p.text);
            v.selection.bg_fill = p.list_selected;
            v.selection.stroke  = egui::Stroke::new(1.0_f32, p.list_sel_border);
            v.widgets.noninteractive.bg_fill = p.toolbar_bg;
            v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, p.border);
            v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, p.text);
            v.widgets.inactive.bg_fill = p.toolbar_bg;
            v.widgets.hovered.bg_fill  = p.list_hover;
            v.widgets.active.bg_fill   = p.list_selected;
            v
        }
        Theme::Light => {
            let mut v = egui::Visuals::light();
            v.panel_fill       = p.list_bg;
            v.window_fill      = p.list_bg;
            v.extreme_bg_color = p.sidebar_bg;
            v.faint_bg_color   = p.list_row_alt;
            v.override_text_color = Some(p.text);
            v.selection.bg_fill = p.list_selected;
            v.selection.stroke  = egui::Stroke::new(1.0_f32, p.list_sel_border);
            v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, p.text);
            v.widgets.inactive.fg_stroke       = egui::Stroke::new(1.0_f32, p.text);
            v.widgets.hovered.fg_stroke        = egui::Stroke::new(1.0_f32, p.text);
            v.widgets.active.fg_stroke         = egui::Stroke::new(1.5_f32, p.text);
            v
        }
    }
}

pub const PROGRESS_BLUE:  Color32 = Color32::from_rgb(0, 190, 120);

// ── Icons ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub enum Icon {
    AddUrl,
    Resume,
    Stop,
    StopAll,
    Delete,
    Options,
    Scheduler,
    StartQueue,
    StopQueue,
    Grabber,
    TellFriend,
    ThemeToggle,
}

/// Big toolbar button: a drawn neon picture with the label under it.
pub fn tool_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, theme: Theme) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(66.0, 62.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let hover_color = if theme == Theme::Dark { Color32::from_rgb(45, 55, 75) } else { Color32::from_rgb(220, 235, 252) };
    let hover_border = if theme == Theme::Dark { Color32::from_rgb(0, 140, 220) } else { Color32::from_rgb(153, 209, 255) };
    if enabled && resp.hovered() {
        p.rect(rect.shrink(1.0), CornerRadius::same(4), hover_color, Stroke::new(1.0_f32, hover_border), StrokeKind::Inside);
    }
    let c = pos2(rect.center().x, rect.top() + 22.0);
    let alpha = if enabled { 1.0 } else { 0.3 };
    draw_icon(p, icon, c, alpha, theme);
    let text_color = if enabled {
        if theme == Theme::Dark { Color32::from_rgb(200, 210, 230) } else { Color32::from_rgb(40, 50, 70) }
    } else {
        Color32::from_gray(130)
    };
    p.text(pos2(rect.center().x, rect.bottom() - 11.0), egui::Align2::CENTER_CENTER, label, FontId::proportional(11.5), text_color);
    resp
}

fn neon(base: Color32, a: f32) -> Color32 {
    if a >= 1.0 { base } else {
        Color32::from_rgb(
            ((base.r() as f32) * a + 60.0 * (1.0 - a)) as u8,
            ((base.g() as f32) * a + 65.0 * (1.0 - a)) as u8,
            ((base.b() as f32) * a + 80.0 * (1.0 - a)) as u8,
        )
    }
}

fn fade_light(base: Color32, a: f32) -> Color32 {
    if a >= 1.0 { base } else {
        Color32::from_rgb(
            ((base.r() as f32) * a + 235.0 * (1.0 - a)) as u8,
            ((base.g() as f32) * a + 235.0 * (1.0 - a)) as u8,
            ((base.b() as f32) * a + 235.0 * (1.0 - a)) as u8,
        )
    }
}

fn draw_icon(p: &egui::Painter, icon: Icon, c: Pos2, a: f32, theme: Theme) {
    let fade = if theme == Theme::Dark { neon } else { fade_light };

    // Neon colours for dark; solid colours for light
    let cyan   = fade(Color32::from_rgb(0, 220, 255), a);
    let green  = fade(Color32::from_rgb(0, 230, 130), a);
    let red    = fade(Color32::from_rgb(255, 70, 70), a);
    let orange = fade(Color32::from_rgb(255, 160, 30), a);
    let blue   = fade(Color32::from_rgb(60, 160, 255), a);
    let purple = fade(Color32::from_rgb(180, 80, 255), a);
    let gray   = fade(Color32::from_rgb(140, 150, 170), a);

    let stroke_w = if theme == Theme::Dark { 2.5_f32 } else { 2.0_f32 };
    let white  = Stroke::new(stroke_w, if theme == Theme::Dark { Color32::from_rgb(220, 230, 255) } else { Color32::WHITE });

    match icon {
        Icon::AddUrl => {
            // Circle with + sign — neon green
            p.circle(c, 15.0, Color32::TRANSPARENT, Stroke::new(2.5_f32, green));
            p.circle(c, 15.0, Color32::from_rgba_premultiplied(0, 230, 130, 25), Stroke::NONE);
            p.line_segment([c + vec2(-8.0, 0.0), c + vec2(8.0, 0.0)], Stroke::new(stroke_w, green));
            p.line_segment([c + vec2(0.0, -8.0), c + vec2(0.0, 8.0)], Stroke::new(stroke_w, green));
        }
        Icon::Resume => {
            // Down arrow inside circle — cyan
            p.circle(c, 15.0, Color32::TRANSPARENT, Stroke::new(2.5_f32, cyan));
            p.circle(c, 15.0, Color32::from_rgba_premultiplied(0, 220, 255, 20), Stroke::NONE);
            // Arrow shaft
            p.line_segment([c + vec2(0.0, -9.0), c + vec2(0.0, 2.0)], Stroke::new(stroke_w, cyan));
            // Arrow head
            p.add(Shape::convex_polygon(vec![c + vec2(-7.0, 1.0), c + vec2(7.0, 1.0), c + vec2(0.0, 10.0)], cyan, Stroke::NONE));
        }
        Icon::Stop => {
            // Square stop — red neon
            p.rect(Rect::from_center_size(c, vec2(28.0, 28.0)), CornerRadius::same(5), Color32::from_rgba_premultiplied(255, 70, 70, 20), Stroke::new(2.5_f32, red), StrokeKind::Inside);
            p.rect_filled(Rect::from_center_size(c, vec2(10.0, 10.0)), 2.0, red);
        }
        Icon::StopAll => {
            p.rect(Rect::from_center_size(c + vec2(5.0, -5.0), vec2(22.0, 22.0)), CornerRadius::same(4), Color32::from_rgba_premultiplied(255, 70, 70, 10), Stroke::new(1.5_f32, fade(Color32::from_rgb(220, 80, 80), a)), StrokeKind::Inside);
            p.rect(Rect::from_center_size(c + vec2(-3.0, 3.0), vec2(22.0, 22.0)), CornerRadius::same(4), Color32::from_rgba_premultiplied(255, 70, 70, 20), Stroke::new(2.5_f32, red), StrokeKind::Inside);
            p.rect_filled(Rect::from_center_size(c + vec2(-3.0, 3.0), vec2(8.0, 8.0)), 2.0, red);
        }
        Icon::Delete => {
            // Trash bin — orange neon
            p.rect(Rect::from_min_max(c + vec2(-9.0, -7.0), c + vec2(9.0, 14.0)), CornerRadius::same(2), Color32::from_rgba_premultiplied(255, 160, 30, 20), Stroke::new(2.0_f32, orange), StrokeKind::Inside);
            p.rect(Rect::from_min_max(c + vec2(-12.0, -12.0), c + vec2(12.0, -8.0)), CornerRadius::same(2), Color32::TRANSPARENT, Stroke::new(2.0_f32, orange), StrokeKind::Inside);
            for dx in [-4.0_f32, 0.0, 4.0] {
                p.line_segment([c + vec2(dx, -3.0), c + vec2(dx, 10.0)], Stroke::new(1.5_f32, orange));
            }
        }
        Icon::Options => {
            // Gear — blue/gray neon
            for i in 0u8..8 {
                let ang = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(ang.cos(), ang.sin());
                p.line_segment([c + d * 8.0, c + d * 15.0], Stroke::new(4.5_f32, blue));
            }
            p.circle(c, 11.0, palette(theme).toolbar_bg, Stroke::new(2.0_f32, blue));
            p.circle_filled(c, 4.5, blue);
        }
        Icon::Scheduler => {
            // Clock — purple neon
            p.circle(c, 15.0, Color32::TRANSPARENT, Stroke::new(2.5_f32, purple));
            p.circle(c, 15.0, Color32::from_rgba_premultiplied(180, 80, 255, 15), Stroke::NONE);
            p.line_segment([c, c + vec2(0.0, -9.0)], Stroke::new(2.5_f32, purple));
            p.line_segment([c, c + vec2(6.0, 4.0)], Stroke::new(2.0_f32, purple));
            // tick marks
            for i in 0u8..12 {
                let ang = i as f32 * std::f32::consts::TAU / 12.0;
                let d = vec2(ang.cos(), ang.sin());
                let (r0, r1) = if i % 3 == 0 { (11.5, 14.0) } else { (12.5, 14.0) };
                p.line_segment([c + d * r0, c + d * r1], Stroke::new(1.0_f32, purple));
            }
        }
        Icon::StartQueue => {
            for (i, dy) in [-10.0_f32, -3.0, 4.0, 11.0].into_iter().enumerate() {
                let w = if i % 2 == 0 { 20.0 } else { 15.0 };
                p.rect_filled(Rect::from_min_size(c + vec2(-15.0, dy - 2.0), vec2(w, 4.0)), 2.0, gray);
            }
            p.add(Shape::convex_polygon(vec![c + vec2(4.0, -7.0), c + vec2(16.0, 3.0), c + vec2(4.0, 13.0)], green, Stroke::NONE));
        }
        Icon::StopQueue => {
            for (i, dy) in [-10.0_f32, -3.0, 4.0, 11.0].into_iter().enumerate() {
                let w = if i % 2 == 0 { 20.0 } else { 15.0 };
                p.rect_filled(Rect::from_min_size(c + vec2(-15.0, dy - 2.0), vec2(w, 4.0)), 2.0, gray);
            }
            p.rect(Rect::from_center_size(c + vec2(10.0, 3.0), vec2(13.0, 13.0)), CornerRadius::same(2), Color32::from_rgba_premultiplied(255, 70, 70, 20), Stroke::new(2.0_f32, red), StrokeKind::Inside);
        }
        Icon::Grabber => {
            // Magnifier — cyan neon
            p.circle(c + vec2(-3.0, -4.0), 10.0, Color32::from_rgba_premultiplied(0, 220, 255, 15), Stroke::new(2.5_f32, cyan));
            p.line_segment([c + vec2(4.5, 3.0), c + vec2(13.0, 12.0)], Stroke::new(4.0_f32, orange));
        }
        Icon::TellFriend => {
            p.circle(c + vec2(-6.0, -6.0), 7.0, Color32::from_rgba_premultiplied(255, 160, 30, 20), Stroke::new(2.0_f32, orange));
            p.circle(c + vec2(7.0, -3.0), 5.5, Color32::from_rgba_premultiplied(60, 160, 255, 20), Stroke::new(2.0_f32, blue));
            p.line_segment([c + vec2(-6.0, 1.0), c + vec2(2.0, 5.0)], Stroke::new(1.5_f32, orange));
            p.line_segment([c + vec2(7.0, 2.5), c + vec2(13.0, 8.0)], Stroke::new(1.5_f32, blue));
        }
        Icon::ThemeToggle => {
            // Sun/moon icon
            p.circle(c, 9.0, Color32::TRANSPARENT, Stroke::new(2.0_f32, orange));
            for i in 0u8..8 {
                let ang = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(ang.cos(), ang.sin());
                p.line_segment([c + d * 11.0, c + d * 14.5], Stroke::new(1.5_f32, orange));
            }
            // Moon crescent overlay for dark, nothing for light
            if theme == Theme::Dark {
                p.circle_filled(c + vec2(4.0, -4.0), 7.0, neon(Color32::from_rgb(28, 32, 42), a));
            }
        }
    }

    // Neon glow pass for dark theme: draw a second, wider, dimmer version
    if theme == Theme::Dark && a > 0.5 {
        let _ = white; // suppress warning
    }
}

// ── Category helpers ─────────────────────────────────────────────────────────

pub fn category_color(c: Category) -> Color32 {
    match c {
        Category::Compressed => Color32::from_rgb(220, 160, 50),
        Category::Documents  => Color32::from_rgb(60, 140, 240),
        Category::Music      => Color32::from_rgb(180, 80, 240),
        Category::Programs   => Color32::from_rgb(50, 200, 100),
        Category::Video      => Color32::from_rgb(240, 70, 70),
        Category::General    => Color32::from_rgb(140, 150, 170),
    }
}

/// Small file icon in the list.
pub fn file_icon(ui: &mut Ui, c: Category) {
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    let r = Rect::from_center_size(rect.center(), vec2(12.0, 15.0));
    let bg = Color32::from_rgb(40, 45, 60);
    ui.painter().rect(r, CornerRadius::same(2), bg, Stroke::new(1.0_f32, Color32::from_gray(80)), StrokeKind::Inside);
    ui.painter().rect_filled(Rect::from_min_max(pos2(r.left() + 1.0, r.bottom() - 6.0), pos2(r.right() - 1.0, r.bottom() - 1.0)), 0.0, category_color(c));
}

pub fn folder_icon(p: &egui::Painter, at: Pos2, color: Color32) {
    let body = Rect::from_min_size(at + vec2(0.0, -5.0), vec2(16.0, 11.0));
    p.rect_filled(Rect::from_min_size(at + vec2(0.0, -7.0), vec2(7.0, 4.0)), 1.0, color);
    p.rect(body, CornerRadius::same(2), color, Stroke::new(1.0_f32, Color32::from_black_alpha(80)), StrokeKind::Inside);
}

/// One row of the category tree.
pub fn tree_item(ui: &mut Ui, depth: usize, expandable: Option<bool>, color: Color32, label: &str, selected: bool, theme: Theme) -> (bool, bool) {
    let p = palette(theme);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect(rect, CornerRadius::ZERO, p.list_selected, Stroke::new(1.0_f32, p.list_sel_border), StrokeKind::Inside);
    } else if resp.hovered() {
        painter.rect_filled(rect, 0.0, p.list_hover);
    }
    let x = rect.left() + 18.0 + depth as f32 * 16.0;
    let cy = rect.center().y;
    if let Some(open) = expandable {
        let t = pos2(x - 11.0, cy);
        let pts = if open { vec![t + vec2(-4.0, -2.0), t + vec2(4.0, -2.0), t + vec2(0.0, 3.0)] } else { vec![t + vec2(-2.0, -4.0), t + vec2(3.0, 0.0), t + vec2(-2.0, 4.0)] };
        painter.add(Shape::convex_polygon(pts, p.text_dim, Stroke::NONE));
    }
    folder_icon(painter, pos2(x, cy), color);
    painter.text(pos2(x + 22.0, cy), egui::Align2::LEFT_CENTER, label, FontId::proportional(13.0), p.text);
    let arrow = resp.clicked() && resp.interact_pointer_pos().is_some_and(|pt| pt.x < x - 2.0);
    (resp.clicked() && !arrow, arrow)
}

/// Progress / segment bar showing downloaded byte ranges.
pub fn segment_bar(ui: &mut Ui, parts: &[Part], size: Option<u64>, theme: Theme) {
    let p = palette(theme);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
    let painter = ui.painter();
    painter.rect(rect, CornerRadius::same(2), p.seg_bar_bg, Stroke::new(1.0_f32, p.border), StrokeKind::Inside);
    let Some(size) = size.filter(|s| *s > 0) else { return };
    let inner = rect.shrink(1.0);
    let x = |b: u64| inner.left() + inner.width() * (b as f32 / size as f32);
    for part in parts {
        if part.pos > part.start {
            painter.rect_filled(Rect::from_min_max(pos2(x(part.start), inner.top()), pos2(x(part.pos).max(x(part.start) + 1.0), inner.bottom())), 2.0, p.progress_fill);
        }
        if let Some(end) = part.end {
            painter.line_segment([pos2(x(end), inner.top()), pos2(x(end), inner.bottom())], Stroke::new(1.0_f32, p.border));
        }
    }
}

#[allow(dead_code)]
pub fn progress_bar(ui: &mut Ui, frac: f32, theme: Theme) {
    let p = palette(theme);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
    let painter = ui.painter();
    painter.rect(rect, CornerRadius::same(5), p.progress_bg, Stroke::NONE, StrokeKind::Inside);
    if frac > 0.0 {
        let fill = Rect::from_min_max(rect.min, pos2(rect.min.x + rect.width() * frac.clamp(0.0, 1.0), rect.max.y));
        painter.rect(fill, CornerRadius::same(5), p.progress_fill, Stroke::NONE, StrokeKind::Inside);
    }
}

// ── String formatters ────────────────────────────────────────────────────────

pub fn fmt_size(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        0..=1023             => format!("{bytes} Bytes"),
        1024..=1_048_575     => format!("{:.2} KB", b / 1024.0),
        1_048_576..=1_073_741_823 => format!("{:.2} MB", b / 1_048_576.0),
        _                    => format!("{:.2} GB", b / 1_073_741_824.0),
    }
}

pub fn fmt_speed(bps: f64) -> String {
    if bps < 1.0 { String::new() } else if bps < 1_048_576.0 { format!("{:.2} KB/sec", bps / 1024.0) } else { format!("{:.2} MB/sec", bps / 1_048_576.0) }
}

pub fn fmt_time_left(secs: u64) -> String {
    match secs {
        0..=59   => format!("{secs} sec"),
        60..=3599 => format!("{} min {} sec", secs / 60, secs % 60),
        _        => format!("{} hour(s) {} min", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Theme;

    #[test]
    fn fmt_time_left_matches_original_patterns() {
        assert_eq!(fmt_time_left(42), "42 sec");
        assert_eq!(fmt_time_left(125), "2 min 5 sec");
        assert_eq!(fmt_time_left(7260), "2 hour(s) 1 min");
    }

    #[test]
    fn fmt_size_picks_unit() {
        assert_eq!(fmt_size(512), "512 Bytes");
        assert_eq!(fmt_size(1536), "1.50 KB");
        assert_eq!(fmt_size(5 * 1_048_576), "5.00 MB");
    }

    #[test]
    fn palette_dark_has_dark_list_bg() {
        let p = palette(Theme::Dark);
        assert!(p.list_bg.r() < 60);
    }
}
