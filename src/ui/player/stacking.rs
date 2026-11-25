//! Subtitles overlaid over the video: second subtitles drawn just above the
//! first ones and smaller, and selectable text overlays for both so hovering
//! lets you select and copy any subtitle as text. mpv still decodes both, and
//! draws the first subtitles itself until they're hovered or selected.

use super::Player;
use super::tracks::IMAGE_SUBS;
use adw::prelude::*;
use gtk::{gdk, pango};
use std::cell::Cell;
use std::rc::Rc;

/// Their size, of the first ones'
const SCALE: f64 = 0.75;
/// Between them and the first ones, of a line of theirs
const GAP: f64 = 0.3;
/// libass sizes a font by its line, Pango by its em: a line of Noto Sans
/// (mpv's sans-serif) is about this many ems
const LINE_EMS: f64 = 1.36;
/// Subtitles drawn as pictures (PGS, DVD's), whose lines mpv doesn't know:
/// their usual line and margin, as shares of the picture's height
const PICTURE_LINE: f64 = 0.06;
const PICTURE_MARGIN: f64 = 0.06;
/// Vertical padding of `.video-subtitle` in style.css, so the text's own
/// bottom lines up with `Place::bottom`
const SUBTITLE_PAD_Y: i32 = 3;

/// What subtitles are laid out in, as heights from the top and bottom edges
/// of the area (shares of its height): the picture's black borders, or
/// none, for the whole area
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Frame {
    top: f64,
    bottom: f64,
}

impl Frame {
    fn height(self) -> f64 {
        1.0 - self.top - self.bottom
    }
}

/// How the first subtitles are laid out: their frame, and a line's height
/// and their margin from its bottom, as shares of the frame's height
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    frame: Frame,
    line: f64,
    margin: f64,
}

/// Where subtitles go: the bottom of their last line, and a line's height,
/// as shares of the area's height
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    bottom: f64,
    line: f64,
}

/// Where `first` subtitles themselves sit, with mpv's sub-pos `position`
/// raising them from where they'd be at 100
fn at(first: Layout, position: f64) -> Place {
    let height = first.frame.height();
    let raise = (100.0 - position) / 100.0 * (1.0 - first.margin) * height;
    Place {
        bottom: 1.0 - first.frame.bottom - first.margin * height - raise,
        line: first.line * height,
    }
}

/// Just above `lines` lines of `first` subtitles, which mpv's sub-pos
/// `position` raises from where they'd be at 100
fn above(first: Layout, lines: usize, position: f64) -> Place {
    let height = first.frame.height();
    // By a share of their frame, less their margin
    let raise = (100.0 - position) / 100.0 * (1.0 - first.margin) * height;
    let top = 1.0 - first.frame.bottom - (first.margin + lines as f64 * first.line) * height;
    let line = SCALE * first.line * height;
    Place {
        bottom: top - raise - GAP * line,
        line,
    }
}

/// The lines of the first subtitles shown, as mpv's `sub-text` has them:
/// room for two at least, so that the second ones don't jump at each line
fn lines(text: Option<&str>) -> usize {
    text.map_or(0, |t| t.trim_end().lines().count()).max(2)
}

/// A line's height and the margin of an ASS track's dialogue, as shares of
/// its frame, from its header (mpv's `sub-ass-extradata`): its Default
/// style, else the first at the bottom
fn ass_layout(header: &str) -> Option<(f64, f64)> {
    let (mut res_x, mut res_y) = (None, None);
    let mut in_styles = false;
    let mut format: Vec<String> = Vec::new();
    let mut styles: Vec<Vec<&str>> = Vec::new();
    for line in header.lines().map(str::trim) {
        if line.starts_with('[') {
            in_styles = line.eq_ignore_ascii_case("[V4+ Styles]")
                || line.eq_ignore_ascii_case("[V4 Styles]");
        } else if let Some(value) = line.strip_prefix("PlayResX:") {
            res_x = value.trim().parse::<f64>().ok().filter(|x| *x > 0.0);
        } else if let Some(value) = line.strip_prefix("PlayResY:") {
            res_y = value.trim().parse::<f64>().ok().filter(|y| *y > 0.0);
        } else if let Some(fields) = line.strip_prefix("Format:").filter(|_| in_styles) {
            format = fields.split(',').map(|f| f.trim().to_lowercase()).collect();
        } else if let Some(fields) = line.strip_prefix("Style:").filter(|_| in_styles) {
            styles.push(fields.split(',').map(str::trim).collect());
        }
    }
    let field = |style: &[&str], name: &str| {
        let i = format.iter().position(|f| f == name)?;
        style.get(i).and_then(|v| v.parse::<f64>().ok())
    };
    let at_bottom =
        |style: &&Vec<&str>| field(style, "alignment").is_some_and(|a| (1.0..=3.0).contains(&a));
    let style = styles
        .iter()
        .find(|s| {
            s.first()
                .is_some_and(|name| name.eq_ignore_ascii_case("Default"))
        })
        .or_else(|| styles.iter().find(at_bottom))?;
    // As libass makes up a missing height
    let res_y = res_y.unwrap_or(match res_x {
        Some(1280.0) => 1024.0,
        Some(x) => x * 3.0 / 4.0,
        None => 288.0,
    });
    let scale = field(style, "scaley").unwrap_or(100.0) / 100.0;
    Some((
        field(style, "fontsize")? * scale / res_y,
        field(style, "marginv").unwrap_or(0.0) / res_y,
    ))
}

/// Whether GTK's right-click context menu (Copy / Select All) is open on `label`
fn has_open_popover(label: &gtk::Label) -> bool {
    let mut child = label.first_child();
    while let Some(widget) = child {
        if widget
            .downcast_ref::<gtk::Popover>()
            .is_some_and(|p| p.is_visible())
        {
            return true;
        }
        child = widget.next_sibling();
    }
    false
}

/// One selectable subtitle label over the video, and where it goes
struct SubLabel {
    label: gtk::Label,
    place: Cell<Option<Place>>,
    hovered: Cell<bool>,
}

impl SubLabel {
    fn new(class: &str) -> SubLabel {
        let label = gtk::Label::builder()
            .css_classes(["video-subtitle", class])
            .justify(gtk::Justification::Center)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .selectable(true)
            .visible(false)
            .build();
        // Clearing an existing selection on a plain primary press stops
        // GtkLabel from starting a text drag-and-drop (whose drag icon can
        // stay stuck floating on Windows); dragging instead starts a new
        // selection, while Shift-click, double-click and right-click keep working
        let press = gtk::GestureClick::builder()
            .button(gdk::BUTTON_PRIMARY)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        let weak = label.downgrade();
        press.connect_pressed(move |gesture, n_press, _, _| {
            if n_press != 1
                || gesture
                    .current_event_state()
                    .contains(gdk::ModifierType::SHIFT_MASK)
            {
                return;
            }
            if let Some(label) = weak.upgrade()
                && label.selection_bounds().is_some()
            {
                label.select_region(0, 0);
            }
        });
        label.add_controller(press);
        SubLabel {
            label,
            place: Cell::new(None),
            hovered: Cell::new(false),
        }
    }

    fn has_selection(&self) -> bool {
        self.label.is_visible() && self.label.selection_bounds().is_some()
    }

    fn is_active(&self) -> bool {
        self.label.is_visible()
            && (self.hovered.get()
                || self.label.selection_bounds().is_some()
                || has_open_popover(&self.label))
    }

    fn set_text(&self, text: Option<&str>, allow: bool) {
        let text = text.unwrap_or_default().trim();
        if self.label.text() != text {
            self.label.set_text(text);
        }
        self.update_visible(allow);
    }

    fn update_visible(&self, allow: bool) {
        let visible = allow && self.place.get().is_some() && !self.label.text().is_empty();
        if !visible {
            self.hovered.set(false);
            self.label.select_region(0, 0);
        }
        self.label.set_visible(visible);
    }

    fn apply_place(&self, place: Option<Place>, overlay_height: f64, allow: bool) {
        self.place.set(place);
        if let Some(place) = place.filter(|_| overlay_height > 0.0) {
            let pixels = place.line * overlay_height / LINE_EMS;
            let size =
                pango::AttrSize::new_size_absolute((pixels * f64::from(pango::SCALE)) as i32);
            let attributes = pango::AttrList::new();
            attributes.insert(size);
            self.label.set_attributes(Some(&attributes));
        }
        self.update_visible(allow);
    }

    fn sync_style(&self) -> bool {
        let active = self.is_active();
        if active {
            self.label.add_css_class("subtitle-active");
        } else {
            self.label.remove_css_class("subtitle-active");
        }
        active
    }

    fn clear_selection(&self) {
        if self.label.selection_bounds().is_some() {
            self.label.select_region(0, 0);
        }
        if self.label.has_focus()
            && let Some(root) = self.label.root()
        {
            root.set_focus(gtk::Widget::NONE);
        }
    }

    /// Centred in `overlay` and at most nine tenths as wide, its last line
    /// ending at its place
    fn position(&self, overlay: &gtk::Overlay) -> Option<gdk::Rectangle> {
        let place = self.place.get()?;
        let width = overlay.width();
        let (_, natural, _, _) = self.label.measure(gtk::Orientation::Horizontal, -1);
        let label_width = natural.min(width * 9 / 10).max(1);
        let (_, height, _, _) = self.label.measure(gtk::Orientation::Vertical, label_width);
        let bottom = (place.bottom * f64::from(overlay.height())).round() as i32;
        let y = (bottom - height + SUBTITLE_PAD_Y).clamp(0, (overlay.height() - height).max(0));
        Some(gdk::Rectangle::new(
            (width - label_width) / 2,
            y,
            label_width,
            height,
        ))
    }
}

/// First and second subtitle overlays over the video
pub(super) struct Subtitles {
    first: SubLabel,
    second: SubLabel,
    /// Whether mpv itself shows the first subtitles (from mpv.conf at startup)
    mpv_shows_first: bool,
    /// Whether we hid mpv's first subtitles while the selectable label is active
    mpv_first_hidden: Cell<bool>,
}

impl Subtitles {
    pub(super) fn new(mpv_shows_first: bool) -> Subtitles {
        Subtitles {
            first: SubLabel::new("first-subtitles"),
            second: SubLabel::new("second-subtitles"),
            mpv_shows_first,
            mpv_first_hidden: Cell::new(false),
        }
    }

    pub(super) fn first_label(&self) -> &gtk::Label {
        &self.first.label
    }

    pub(super) fn second_label(&self) -> &gtk::Label {
        &self.second.label
    }

    pub(super) fn has_selection(&self) -> bool {
        self.first.has_selection() || self.second.has_selection()
    }

    pub(super) fn is_active(&self) -> bool {
        self.first.is_active() || self.second.is_active()
    }

    /// Only take pointer events while controls (and the cursor) are shown, so
    /// a hidden parked pointer doesn't flash the subtitle pill or eat clicks
    pub(super) fn set_targetable(&self, targetable: bool) {
        self.first
            .label
            .set_can_target(targetable || self.first.is_active());
        self.second
            .label
            .set_can_target(targetable || self.second.is_active());
    }
}

impl Player {
    pub(super) fn connect_subtitles(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.overlay
            .connect_get_child_position(move |overlay, child| {
                let p = weak.upgrade()?;
                if child == p.subtitles.first.label.upcast_ref::<gtk::Widget>() {
                    p.subtitles.first.position(overlay)
                } else if child == p.subtitles.second.label.upcast_ref::<gtk::Widget>() {
                    p.subtitles.second.position(overlay)
                } else {
                    None
                }
            });
        for is_first in [true, false] {
            let label = if is_first {
                &self.subtitles.first.label
            } else {
                &self.subtitles.second.label
            };
            let hover = gtk::EventControllerMotion::new();
            let weak = Rc::downgrade(self);
            hover.connect_enter(move |_, _, _| {
                if let Some(p) = weak.upgrade()
                    && p.state.borrow().controls_shown
                {
                    p.sub_label(is_first).hovered.set(true);
                    p.sync_subtitles();
                }
            });
            let weak = Rc::downgrade(self);
            hover.connect_motion(move |_, _, _| {
                if let Some(p) = weak.upgrade()
                    && p.state.borrow().controls_shown
                    && !p.sub_label(is_first).hovered.get()
                {
                    p.sub_label(is_first).hovered.set(true);
                    p.sync_subtitles();
                }
            });
            let weak = Rc::downgrade(self);
            hover.connect_leave(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.sub_label(is_first).hovered.set(false);
                    p.sync_subtitles();
                }
            });
            label.add_controller(hover);
            for prop in ["selection-bound", "cursor-position"] {
                let weak = Rc::downgrade(self);
                label.connect_notify_local(Some(prop), move |_, _| {
                    if let Some(p) = weak.upgrade() {
                        p.sync_subtitles();
                    }
                });
            }
        }
    }

    fn sub_label(&self, first: bool) -> &SubLabel {
        if first {
            &self.subtitles.first
        } else {
            &self.subtitles.second
        }
    }

    /// The first subtitles' text, as mpv decodes it (None: none)
    pub(super) fn first_text(&self, text: Option<&str>) {
        self.subtitles
            .first
            .set_text(text, self.subtitles.mpv_shows_first);
        self.sync_subtitles();
    }

    /// The second subtitles' text, as mpv decodes it (None: none)
    pub(super) fn second_text(&self, text: Option<&str>) {
        self.subtitles.second.set_text(text, true);
        self.sync_subtitles();
    }

    /// Clear any selected subtitle text and return focus to the player
    pub(super) fn clear_subtitle_selection(&self) {
        self.subtitles.first.clear_selection();
        self.subtitles.second.clear_selection();
        self.sync_subtitles();
    }

    /// Update hover/selection pill styling and hide mpv's own first subtitles
    /// while the selectable overlay is shown over them
    pub(super) fn sync_subtitles(&self) {
        let first_active = self.subtitles.first.sync_style();
        self.subtitles.second.sync_style();
        let hide_mpv = first_active && self.subtitles.mpv_shows_first;
        if self.subtitles.mpv_first_hidden.replace(hide_mpv) != hide_mpv {
            self.mpv_set("sub-visibility", if hide_mpv { "no" } else { "yes" });
        }
    }

    /// Place both subtitle overlays again when their track, lines, sub-pos
    /// or the picture's dimensions change
    pub(super) fn layout_subtitles(&self) {
        let (first_place, second_place) = self.subtitle_places();
        let height = f64::from(self.overlay.height());
        self.subtitles
            .first
            .apply_place(first_place, height, self.subtitles.mpv_shows_first);
        self.subtitles
            .second
            .apply_place(second_place, height, true);
        self.sync_subtitles();
        self.overlay.queue_allocate();
    }

    /// Where the first and second subtitle overlays go
    fn subtitle_places(&self) -> (Option<Place>, Option<Place>) {
        let mpv = self.mpv.borrow();
        let Some(mpv) = mpv.as_ref() else {
            return (None, None);
        };
        let has_first = mpv.get("current-tracks/sub/id").is_some();
        let has_second = mpv.get("current-tracks/sub2/id").is_some();
        if !has_first && !has_second {
            return (None, None);
        }
        let number = |name: &str, default: f64| {
            mpv.get(name)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(default)
        };
        let yes = |name: &str, default: bool| mpv.get(name).map_or(default, |v| v == "yes");
        let total = number("osd-dimensions/h", 0.0);
        let picture = if total > 0.0 {
            Frame {
                top: number("osd-dimensions/mt", 0.0) / total,
                bottom: number("osd-dimensions/mb", 0.0) / total,
            }
        } else {
            Frame::default()
        };
        // mpv's sizes for text are for an area 720 pixels high
        let scale = number("sub-scale", 1.0);
        let text = Layout {
            frame: if yes("sub-use-margins", true) {
                Frame::default()
            } else {
                picture
            },
            line: number("sub-font-size", 38.0) * scale / 720.0,
            margin: number("sub-margin-y", 34.0) / 720.0,
        };
        let position = number("sub-pos", 100.0);
        // Without first ones, second ones go about in their place
        let Some(codec) = mpv.get("current-tracks/sub/codec").filter(|_| has_first) else {
            return (None, has_second.then(|| above(text, 0, position)));
        };
        let shown = || lines(mpv.get("sub-text").as_deref());
        let styled = !matches!(
            mpv.get("sub-ass-override").as_deref(),
            Some("force" | "strip")
        );
        let image = IMAGE_SUBS.contains(&codec.as_str());
        let (first, count) = if image {
            let first = Layout {
                frame: picture,
                line: PICTURE_LINE,
                margin: PICTURE_MARGIN,
            };
            (first, 2)
        } else if matches!(codec.as_str(), "ass" | "ssa") && styled {
            let (line, margin) = mpv
                .get("sub-ass-extradata")
                .and_then(|header| ass_layout(&header))
                .unwrap_or((text.line, text.margin));
            let frame = if yes("sub-ass-force-margins", false) {
                Frame::default()
            } else {
                picture
            };
            let first = Layout {
                frame,
                line: line * scale,
                margin,
            };
            (first, shown())
        } else {
            (text, shown())
        };
        let first_place = (!image).then(|| at(first, position));
        let second_place = has_second.then(|| above(first, count, position));
        (first_place, second_place)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// mpv's own text style, over the whole area
    const TEXT: Layout = Layout {
        frame: Frame {
            top: 0.0,
            bottom: 0.0,
        },
        line: 38.0 / 720.0,
        margin: 34.0 / 720.0,
    };

    /// Crunchyroll's style: Gandhi Sans 25, MarginV 18, in 360 lines
    const CRUNCHYROLL: Layout = Layout {
        frame: Frame {
            top: 0.0,
            bottom: 0.0,
        },
        line: 25.0 / 360.0,
        margin: 18.0 / 360.0,
    };

    const HEADER: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\n\n\
        [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, \
        OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, \
        Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
        Style: Ep_Title,Times New Roman,20,&H00FFFFFF,&H000000FF,&HD2000000,&HD2000000,0,0,0,0,\
        100,100,0,0,1,1,1,3,18,42,56,0\n\
        Style: Default,Gandhi Sans,25,&H00FFFFFF,&H000000FF,&H00000000,&HA0000000,-1,0,0,0,\
        100,100,0,0,1,1.2,0.5,2,10,10,18,1\n\n[Events]\nFormat: Layer, Start, End, Style, Text\n";

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn reads_the_dialogue_style() {
        let (line, margin) = ass_layout(HEADER).unwrap();
        assert!(close(line, 25.0 / 360.0) && close(margin, 18.0 / 360.0));
        // Without a Default style, the first at the bottom: Ep_Title, at the
        // bottom right
        let other = HEADER.replace("Style: Default,", "Style: Main,");
        assert_eq!(ass_layout(&other).unwrap().1, 56.0 / 360.0);
        // Without its height, as libass makes it up
        let no_height = HEADER.replace("PlayResY: 360\n", "");
        assert_eq!(ass_layout(&no_height).unwrap().0, 25.0 / 480.0);
        assert_eq!(ass_layout("[Script Info]\nPlayResY: 360\n"), None);
    }

    #[test]
    fn second_subtitles_end_just_above_two_lines() {
        // Measured with mpv, down a 1080-line area: two lines of its own
        // style start 0.859 down, two of Crunchyroll's larger ones 0.824
        for (first, top) in [(TEXT, 0.859), (CRUNCHYROLL, 0.824)] {
            let end = above(first, 2, 100.0).bottom;
            assert!(end < top && top - end < 0.04, "{end} {top}");
        }
        // Higher above a third line, by one
        let third = above(CRUNCHYROLL, 2, 100.0).bottom - above(CRUNCHYROLL, 3, 100.0).bottom;
        assert!(close(third, 25.0 / 360.0), "{third}");
    }

    #[test]
    fn second_subtitles_are_smaller() {
        for first in [TEXT, CRUNCHYROLL] {
            assert!(close(above(first, 2, 100.0).line, 0.75 * first.line));
        }
    }

    #[test]
    fn second_subtitles_rise_with_the_first() {
        // Measured with mpv: at sub-pos 80, a line of its own style is
        // 0.1907 higher than at 100
        let rise = above(TEXT, 2, 100.0).bottom - above(TEXT, 2, 80.0).bottom;
        assert!(close(rise, 0.1907), "{rise}");
    }

    #[test]
    fn second_subtitles_follow_first_ones_in_the_picture() {
        // Fullscreen on a 16:10 screen: ASS subtitles are in the 16:9
        // picture, over a black border; the second ones are over them still
        let letterboxed = Layout {
            frame: Frame {
                top: 0.05,
                bottom: 0.05,
            },
            ..CRUNCHYROLL
        };
        let end = above(letterboxed, 2, 100.0).bottom;
        let first_top = 0.95 - (18.0 + 50.0) / 360.0 * 0.9;
        assert!(
            end < first_top && first_top - end < 0.03,
            "{end} {first_top}"
        );
    }

    #[test]
    fn first_subtitles_sit_at_their_margin_and_rise_with_sub_pos() {
        let base = at(TEXT, 100.0);
        assert!(close(base.bottom, 1.0 - 34.0 / 720.0));
        assert!(close(base.line, 38.0 / 720.0));
        let raised = at(TEXT, 80.0);
        assert!(close(base.bottom - raised.bottom, 0.1907));
    }

    #[test]
    fn room_for_two_lines_at_least() {
        assert_eq!(lines(None), 2);
        assert_eq!(lines(Some("")), 2);
        assert_eq!(lines(Some("One\n")), 2);
        assert_eq!(lines(Some("One\nTwo\nThree")), 3);
    }
}
