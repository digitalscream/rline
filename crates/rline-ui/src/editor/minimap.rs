//! Minimap — "thousand-mile view" overlay showing the entire buffer on the
//! right edge, with the current viewport highlighted.
//!
//! The minimap is a `gtk4::DrawingArea` added as an overlay child of the
//! editor tab's `gtk4::Overlay`. Because overlay children do not reduce the
//! main child's allocation, the editor keeps its full width; the minimap
//! simply paints over it with transparency.
//!
//! Each buffer line is drawn as a sequence of runs coloured by the syntax
//! highlighting `TextTag`s present at that offset, so the minimap reads as
//! a tiny faded reflection of the code below it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use sourceview5::prelude::*;

const MINIMAP_WIDTH: i32 = 100;
/// Character count at which a line's content reaches the minimap's full inner width.
const MAX_LINE_CHARS: f64 = 80.0;
/// Pixel thickness of a single line's bar.
const LINE_THICKNESS: f64 = 1.0;
/// Vertical gap between adjacent line bars.
const LINE_GAP: f64 = 1.0;
/// Combined per-line vertical stride.
const LINE_STRIDE: f64 = LINE_THICKNESS + LINE_GAP;
const BG_ALPHA: f64 = 0.06;
const LINE_ALPHA: f64 = 0.28;
const VIEWPORT_ALPHA: f64 = 0.18;

/// A single contiguous span on a buffer line sharing one foreground colour.
#[derive(Debug, Clone, Copy)]
struct LineRun {
    /// Character offset from the start of the line.
    start_char: u32,
    /// Length of the span in characters.
    len_chars: u32,
    /// Foreground colour (r, g, b), each 0.0–1.0.
    color: (f64, f64, f64),
}

/// Minimap widget overlaid on the right edge of the editor.
#[derive(Debug, Clone)]
pub struct Minimap {
    drawing_area: gtk4::DrawingArea,
}

impl Minimap {
    /// Create a minimap bound to the given buffer and view.
    ///
    /// The minimap reads syntax-coloured runs from `buffer` to draw its
    /// schematic representation, and reads / writes `view`'s vertical
    /// adjustment to show and control the viewport position.
    pub fn new(buffer: &sourceview5::Buffer, view: &sourceview5::View) -> Self {
        let drawing_area = gtk4::DrawingArea::new();
        drawing_area.set_content_width(MINIMAP_WIDTH);
        drawing_area.set_content_height(1);
        drawing_area.set_width_request(MINIMAP_WIDTH);
        drawing_area.set_halign(gtk4::Align::End);
        drawing_area.set_valign(gtk4::Align::Fill);
        drawing_area.set_hexpand(false);
        drawing_area.set_vexpand(true);
        drawing_area.add_css_class("minimap");

        let line_runs: Rc<RefCell<Vec<Vec<LineRun>>>> =
            Rc::new(RefCell::new(collect_line_runs(buffer)));

        // ── Draw func ──────────────────────────────────────────
        {
            let line_runs = line_runs.clone();
            let view_weak = view.downgrade();
            drawing_area.set_draw_func(move |_, cr, width, height| {
                let Some(view) = view_weak.upgrade() else {
                    return;
                };
                let Some(vadj) = view.vadjustment() else {
                    return;
                };
                draw_minimap(cr, width, height, &line_runs.borrow(), &vadj);
            });
        }

        // Shared coalescing flag used by both text-change and tag-change
        // handlers so a burst of signals collapses into a single rebuild.
        let scheduled: Rc<Cell<bool>> = Rc::new(Cell::new(false));

        // ── Buffer text changes → rebuild cache + redraw ──────
        {
            let scheduled = scheduled.clone();
            let line_runs_outer = line_runs.clone();
            buffer.connect_changed(glib::clone!(
                #[weak]
                drawing_area,
                move |buf| {
                    schedule_rebuild(
                        buf,
                        &drawing_area,
                        line_runs_outer.clone(),
                        scheduled.clone(),
                    );
                }
            ));
        }

        // ── Tag apply/remove (syntax highlight updates) → rebuild ──
        {
            let scheduled = scheduled.clone();
            let line_runs_outer = line_runs.clone();
            buffer.connect_apply_tag(glib::clone!(
                #[weak]
                drawing_area,
                move |buf, _tag, _start, _end| {
                    schedule_rebuild(
                        buf,
                        &drawing_area,
                        line_runs_outer.clone(),
                        scheduled.clone(),
                    );
                }
            ));
        }
        {
            let scheduled = scheduled.clone();
            let line_runs_outer = line_runs.clone();
            buffer.connect_remove_tag(glib::clone!(
                #[weak]
                drawing_area,
                move |buf, _tag, _start, _end| {
                    schedule_rebuild(
                        buf,
                        &drawing_area,
                        line_runs_outer.clone(),
                        scheduled.clone(),
                    );
                }
            ));
        }

        // ── Adjustment changes → redraw viewport indicator ────
        let Some(vadj) = view.vadjustment() else {
            return Self { drawing_area };
        };
        vadj.connect_value_changed(glib::clone!(
            #[weak]
            drawing_area,
            move |_| drawing_area.queue_draw()
        ));
        vadj.connect_upper_notify(glib::clone!(
            #[weak]
            drawing_area,
            move |_| drawing_area.queue_draw()
        ));
        vadj.connect_page_size_notify(glib::clone!(
            #[weak]
            drawing_area,
            move |_| drawing_area.queue_draw()
        ));

        // ── Click + drag → scroll ─────────────────────────────
        let drag = gtk4::GestureDrag::new();
        {
            let view_weak = view.downgrade();
            let line_runs = line_runs.clone();
            drag.connect_drag_begin(glib::clone!(
                #[weak]
                drawing_area,
                move |_, _x, y| {
                    if let Some(view) = view_weak.upgrade() {
                        let height = drawing_area.height() as f64;
                        scroll_to_y(&view, y, height, line_runs.borrow().len());
                    }
                }
            ));
        }
        {
            let view_weak = view.downgrade();
            let line_runs = line_runs.clone();
            drag.connect_drag_update(glib::clone!(
                #[weak]
                drawing_area,
                move |gesture, off_x, off_y| {
                    let _ = off_x;
                    let Some((_, start_y)) = gesture.start_point() else {
                        return;
                    };
                    if let Some(view) = view_weak.upgrade() {
                        let height = drawing_area.height() as f64;
                        scroll_to_y(&view, start_y + off_y, height, line_runs.borrow().len());
                    }
                }
            ));
        }
        drawing_area.add_controller(drag);

        Self { drawing_area }
    }

    /// The GTK widget to add to an overlay.
    pub fn widget(&self) -> &gtk4::DrawingArea {
        &self.drawing_area
    }
}

/// Coalesce a rebuild request — schedule at most one idle callback that
/// rereads the buffer and redraws the minimap.
fn schedule_rebuild(
    buffer: &sourceview5::Buffer,
    drawing_area: &gtk4::DrawingArea,
    line_runs: Rc<RefCell<Vec<Vec<LineRun>>>>,
    scheduled: Rc<Cell<bool>>,
) {
    if scheduled.get() {
        return;
    }
    scheduled.set(true);
    let buf_clone = buffer.clone();
    let da_weak = drawing_area.downgrade();
    glib::idle_add_local_once(move || {
        *line_runs.borrow_mut() = collect_line_runs(&buf_clone);
        if let Some(da) = da_weak.upgrade() {
            da.queue_draw();
        }
        scheduled.set(false);
    });
}

/// For each buffer line, collect colour-homogeneous character runs based on
/// the `TextTag`s active at each offset. Lines with no tagged runs get a
/// single default-coloured run covering their full width.
fn collect_line_runs(buffer: &sourceview5::Buffer) -> Vec<Vec<LineRun>> {
    let default_fg = default_foreground(buffer);
    let line_count = buffer.line_count();
    let mut lines = Vec::with_capacity(line_count.max(0) as usize);

    for line in 0..line_count {
        let mut runs = Vec::new();
        let Some(start) = buffer.iter_at_line(line) else {
            lines.push(runs);
            continue;
        };
        let mut line_end = start;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }

        let mut iter = start;
        while iter.offset() < line_end.offset() {
            let start_off = iter.line_offset();
            let tags = iter.tags();
            let color = pick_foreground(&tags).unwrap_or(default_fg);

            let mut next = iter;
            let moved = next.forward_to_tag_toggle(None::<&gtk4::TextTag>);
            let use_next =
                moved && next.offset() > iter.offset() && next.offset() <= line_end.offset();
            let end_iter = if use_next { next } else { line_end };

            let end_off = end_iter.line_offset();
            let len = (end_off - start_off).max(0) as u32;
            if len == 0 {
                break;
            }
            runs.push(LineRun {
                start_char: start_off as u32,
                len_chars: len,
                color,
            });

            if end_iter.offset() <= iter.offset() {
                break;
            }
            iter = end_iter;
        }

        lines.push(runs);
    }
    lines
}

/// From the tags at one position, pick the foreground colour of the
/// highest-priority tag that has one set. `TextIter::tags()` returns tags
/// in ascending priority order, so the last matching one wins.
fn pick_foreground(tags: &[gtk4::TextTag]) -> Option<(f64, f64, f64)> {
    let mut result = None;
    for tag in tags {
        let fg_set = tag
            .property_value("foreground-set")
            .get::<bool>()
            .unwrap_or(false);
        if !fg_set {
            continue;
        }
        if let Ok(rgba) = tag
            .property_value("foreground-rgba")
            .get::<gtk4::gdk::RGBA>()
        {
            result = Some((rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64));
        }
    }
    result
}

/// Default foreground colour for untagged text, from the buffer's active
/// style scheme's "text" style. Falls back to a neutral light grey.
fn default_foreground(buffer: &sourceview5::Buffer) -> (f64, f64, f64) {
    if let Some(scheme) = buffer.style_scheme() {
        if let Some(style) = scheme.style("text") {
            let fg_set = style
                .property_value("foreground-set")
                .get::<bool>()
                .unwrap_or(false);
            if fg_set {
                if let Ok(fg_str) = style.property_value("foreground").get::<String>() {
                    if let Ok(rgba) = gtk4::gdk::RGBA::parse(&fg_str) {
                        return (rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64);
                    }
                }
            }
        }
    }
    (0.85, 0.85, 0.85)
}

/// Paint the minimap: background, syntax-coloured line runs, viewport indicator.
fn draw_minimap(
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    line_runs: &[Vec<LineRun>],
    vadj: &gtk4::Adjustment,
) {
    let w = width as f64;
    let h = height as f64;
    tracing::trace!(width, height, lines = line_runs.len(), "minimap draw");
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let line_count = line_runs.len();
    let content_height = (line_count as f64) * LINE_STRIDE;
    let scroll_offset = compute_scroll_offset(
        line_count,
        h,
        vadj.lower(),
        vadj.upper(),
        vadj.value(),
        vadj.page_size(),
    );

    // Background — only covers the area actually occupied by content so a
    // short file doesn't paint a dim strip down the full editor height.
    let bg_height = content_height.min(h);
    if bg_height > 0.0 {
        cr.set_source_rgba(0.0, 0.0, 0.0, BG_ALPHA);
        cr.rectangle(0.0, 0.0, w, bg_height);
        let _ = cr.fill();
    }

    // Line content — only draw lines that fall within the visible band.
    if line_count > 0 {
        let bar_start_x = 4.0;
        let bar_max_w = (w - 8.0).max(1.0);
        let px_per_char = bar_max_w / MAX_LINE_CHARS;

        let first = (scroll_offset / LINE_STRIDE).floor().max(0.0) as usize;
        let last = (((scroll_offset + h) / LINE_STRIDE).ceil() as usize).min(line_count);

        for (i, runs) in line_runs.iter().enumerate().take(last).skip(first) {
            let y = (i as f64) * LINE_STRIDE - scroll_offset;
            for run in runs {
                let run_start_px = (run.start_char as f64) * px_per_char;
                if run_start_px >= bar_max_w {
                    break;
                }
                let run_end_px =
                    (((run.start_char + run.len_chars) as f64) * px_per_char).min(bar_max_w);
                let run_w = run_end_px - run_start_px;
                if run_w <= 0.0 {
                    continue;
                }
                let (r, g, b) = run.color;
                cr.set_source_rgba(r, g, b, LINE_ALPHA);
                cr.rectangle(bar_start_x + run_start_px, y, run_w, LINE_THICKNESS);
                let _ = cr.fill();
            }
        }
    }

    // Viewport indicator — mapped into content space then translated by
    // the minimap's own scroll offset so it tracks the visible band.
    let upper = vadj.upper();
    let lower = vadj.lower();
    let page = vadj.page_size();
    let value = vadj.value();
    let total_h = upper - lower;
    if total_h > 0.0 && content_height > 0.0 {
        let top_frac = ((value - lower) / total_h).clamp(0.0, 1.0);
        let bot_frac = ((value + page - lower) / total_h).clamp(0.0, 1.0);
        let vy = top_frac * content_height - scroll_offset;
        let vh = ((bot_frac - top_frac) * content_height).max(2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, VIEWPORT_ALPHA);
        cr.rectangle(0.0, vy, w, vh);
        let _ = cr.fill();
    }
}

/// How much of the minimap's content is scrolled off the top when the
/// full content can't fit. Returns 0 when the content fits.
fn compute_scroll_offset(
    line_count: usize,
    minimap_height: f64,
    vadj_lower: f64,
    vadj_upper: f64,
    vadj_value: f64,
    vadj_page_size: f64,
) -> f64 {
    let content_height = (line_count as f64) * LINE_STRIDE;
    if content_height <= minimap_height {
        return 0.0;
    }
    let scrollable = vadj_upper - vadj_lower - vadj_page_size;
    if scrollable <= 0.0 {
        return 0.0;
    }
    let frac = ((vadj_value - vadj_lower) / scrollable).clamp(0.0, 1.0);
    frac * (content_height - minimap_height)
}

/// Map a minimap-space Y coordinate to a scroll value and apply it to the
/// view's vertical adjustment (no animation). Centers the viewport on the
/// clicked point when possible.
fn scroll_to_y(view: &sourceview5::View, y: f64, height: f64, line_count: usize) {
    let Some(vadj) = view.vadjustment() else {
        return;
    };
    let target = minimap_click_to_scroll_value(
        y,
        height,
        line_count,
        vadj.lower(),
        vadj.upper(),
        vadj.value(),
        vadj.page_size(),
    );
    vadj.set_value(target);
}

/// Pure mapping from a click in minimap-visible coordinates to the target
/// scroll `value` for the vertical adjustment. Accounts for the minimap's
/// own scroll offset when the content doesn't fit.
fn minimap_click_to_scroll_value(
    y: f64,
    minimap_height: f64,
    line_count: usize,
    lower: f64,
    upper: f64,
    value: f64,
    page_size: f64,
) -> f64 {
    if minimap_height <= 0.0 || upper <= lower || line_count == 0 {
        return lower;
    }
    let content_height = (line_count as f64) * LINE_STRIDE;
    let scroll_offset =
        compute_scroll_offset(line_count, minimap_height, lower, upper, value, page_size);
    let content_y = (y + scroll_offset).clamp(0.0, content_height);
    let frac = if content_height > 0.0 {
        content_y / content_height
    } else {
        0.0
    };
    let doc_center = lower + frac * (upper - lower);
    let max_value = (upper - page_size).max(lower);
    (doc_center - page_size / 2.0).clamp(lower, max_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A "short" buffer whose content fits in the minimap height so no
    // minimap scrolling occurs. 10 lines × 3 px stride = 30 px ≤ 200 px.
    const SHORT_LINES: usize = 10;
    // A "long" buffer that overflows the minimap. 500 lines × 3 px = 1500 px > 200 px.
    const LONG_LINES: usize = 500;

    #[test]
    fn test_compute_scroll_offset_content_fits_returns_zero() {
        let offset = compute_scroll_offset(SHORT_LINES, 200.0, 0.0, 1000.0, 500.0, 100.0);
        assert_eq!(offset, 0.0, "short content should not scroll the minimap");
    }

    #[test]
    fn test_compute_scroll_offset_top_of_long_buffer_returns_zero() {
        let offset = compute_scroll_offset(LONG_LINES, 200.0, 0.0, 1000.0, 0.0, 100.0);
        assert_eq!(offset, 0.0, "buffer at top should show minimap top");
    }

    #[test]
    fn test_compute_scroll_offset_bottom_of_long_buffer_shows_bottom() {
        let offset = compute_scroll_offset(LONG_LINES, 200.0, 0.0, 1000.0, 900.0, 100.0);
        let content_height = (LONG_LINES as f64) * LINE_STRIDE;
        assert_eq!(
            offset,
            content_height - 200.0,
            "buffer at bottom should scroll minimap so its bottom is visible"
        );
    }

    #[test]
    fn test_compute_scroll_offset_midway_interpolates() {
        let offset = compute_scroll_offset(LONG_LINES, 200.0, 0.0, 1000.0, 450.0, 100.0);
        let content_height = (LONG_LINES as f64) * LINE_STRIDE;
        let expected = 0.5 * (content_height - 200.0);
        assert!(
            (offset - expected).abs() < 1e-9,
            "halfway buffer scroll should place minimap halfway"
        );
    }

    #[test]
    fn test_click_short_buffer_top_maps_to_lower() {
        let v = minimap_click_to_scroll_value(0.0, 200.0, SHORT_LINES, 0.0, 1000.0, 0.0, 100.0);
        assert_eq!(v, 0.0, "top click on short buffer → scroll to top");
    }

    #[test]
    fn test_click_short_buffer_below_content_still_clamps_to_bottom() {
        // Click below the content region — all content y beyond content_height
        // clamps, so it maps to the max scroll.
        let v = minimap_click_to_scroll_value(150.0, 200.0, SHORT_LINES, 0.0, 1000.0, 0.0, 100.0);
        assert_eq!(
            v, 900.0,
            "click below drawn content should scroll to document end"
        );
    }

    #[test]
    fn test_click_middle_of_long_buffer_at_rest() {
        // Minimap is at scroll_offset = 0 (buffer at top). Clicking minimap
        // at y=100 maps to content_y=100, frac = 100/content_height.
        let v = minimap_click_to_scroll_value(100.0, 200.0, LONG_LINES, 0.0, 1000.0, 0.0, 100.0);
        let content_height = (LONG_LINES as f64) * LINE_STRIDE;
        let expected = (100.0 / content_height) * 1000.0 - 50.0;
        assert!((v - expected).abs() < 1e-6, "expected ~{expected}, got {v}");
    }

    #[test]
    fn test_click_respects_minimap_scroll_offset() {
        // Buffer halfway → minimap scroll_offset = 0.5 * (1500 - 200) = 650.
        // Clicking minimap y=100 should map to content_y=750, frac=0.5.
        let v = minimap_click_to_scroll_value(100.0, 200.0, LONG_LINES, 0.0, 1000.0, 450.0, 100.0);
        let expected = 0.5 * 1000.0 - 50.0; // doc_center - page/2
        assert!((v - expected).abs() < 1e-6, "expected {expected}, got {v}");
    }

    #[test]
    fn test_click_empty_buffer_returns_lower() {
        let v = minimap_click_to_scroll_value(50.0, 200.0, 0, 0.0, 1000.0, 0.0, 100.0);
        assert_eq!(v, 0.0, "empty buffer returns lower bound");
    }

    #[test]
    fn test_click_negative_y_clamps_to_lower() {
        let v = minimap_click_to_scroll_value(-50.0, 200.0, LONG_LINES, 0.0, 1000.0, 0.0, 100.0);
        assert_eq!(v, 0.0, "negative y clamps to document top");
    }

    #[test]
    fn test_click_respects_nonzero_lower() {
        let v = minimap_click_to_scroll_value(0.0, 200.0, SHORT_LINES, 50.0, 1050.0, 50.0, 100.0);
        assert_eq!(v, 50.0, "top click respects nonzero lower bound");
    }
}
