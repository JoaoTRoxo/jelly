//! The second subtitles, drawn by Jelly just above the first ones, and
//! smaller. mpv still decodes them, but drew them in its own text style:
//! as large as first subtitles in it, larger than ASS ones (it sizes text
//! by the whole area, ASS by the picture), and laid out apart from the
//! first, each in its own frame (ASS in the video's picture, text over its
//! black borders too).

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

/// Where the second subtitles go: the bottom of their last line, and a
/// line's height, as shares of the area's height
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    bottom: f64,
    line: f64,
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

/// The second subtitles over the video: their label, and where it goes
pub(super) struct Second {
    pub label: gtk::Label,
    place: Cell<Option<Place>>,
}

impl Second {
    pub(super) fn new() -> Second {
        let label = gtk::Label::builder()
            .css_classes(["second-subtitles"])
            .justify(gtk::Justification::Center)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .can_target(false)
            .visible(false)
            .build();
        Second {
            label,
            place: Cell::new(None),
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
        Some(gdk::Rectangle::new(
            (width - label_width) / 2,
            bottom - height,
            label_width,
            height,
        ))
    }
}

impl Player {
    pub(super) fn connect_second(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.overlay
            .connect_get_child_position(move |overlay, child| {
                let p = weak.upgrade()?;
                (child == p.second.label.upcast_ref::<gtk::Widget>())
                    .then(|| p.second.position(overlay))
                    .flatten()
            });
    }

    /// The second subtitles' text, as mpv decodes it (None: none)
    pub(super) fn second_text(&self, text: Option<&str>) {
        self.second
            .label
            .set_text(text.unwrap_or_default().trim_end());
        self.show_second();
    }

    /// The second subtitles just above the first ones, again when those
    /// change (their track, lines or place) or the picture's place does
    pub(super) fn stack_second(&self) {
        let place = self.second_place();
        self.second.place.set(place);
        let height = f64::from(self.overlay.height());
        if let Some(place) = place.filter(|_| height > 0.0) {
            let pixels = place.line * height / LINE_EMS;
            let size =
                pango::AttrSize::new_size_absolute((pixels * f64::from(pango::SCALE)) as i32);
            let attributes = pango::AttrList::new();
            attributes.insert(size);
            self.second.label.set_attributes(Some(&attributes));
        }
        self.show_second();
        self.overlay.queue_allocate();
    }

    fn show_second(&self) {
        let label = &self.second.label;
        label.set_visible(self.second.place.get().is_some() && !label.text().is_empty());
    }

    /// Where the second subtitles go, while there are some
    fn second_place(&self) -> Option<Place> {
        let mpv = self.mpv.borrow();
        let mpv = mpv.as_ref()?;
        mpv.get("current-tracks/sub2/id")?;
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
        // Without first ones, about in their place
        let Some(codec) = mpv.get("current-tracks/sub/codec") else {
            return Some(above(text, 0, position));
        };
        let shown = || lines(mpv.get("sub-text").as_deref());
        let styled = !matches!(
            mpv.get("sub-ass-override").as_deref(),
            Some("force" | "strip")
        );
        let (first, count) = if IMAGE_SUBS.contains(&codec.as_str()) {
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
        Some(above(first, count, position))
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
    fn room_for_two_lines_at_least() {
        assert_eq!(lines(None), 2);
        assert_eq!(lines(Some("")), 2);
        assert_eq!(lines(Some("One\n")), 2);
        assert_eq!(lines(Some("One\nTwo\nThree")), 3);
    }
}
