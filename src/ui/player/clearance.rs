//! Moving the subtitles clear of the controls while they're shown (as they
//! are all the time the video is paused), and back once they hide: the
//! bottom bar dimmed the subtitles, and the top bar hid second subtitles at
//! the top.

use super::Player;
use super::tracks::IMAGE_SUBS;
use crate::mpv::Mpv;
use adw::prelude::*;

/// mpv's `sub-pos` and `secondary-sub-pos`: how far down the subtitles and
/// the second ones are, in percent
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SubPositions {
    pub first: f64,
    pub second: f64,
}

/// What mpv lays subtitles out in: the whole area, black borders and all,
/// or only the video's picture
#[derive(Clone, Copy, Debug, PartialEq)]
enum Frame {
    Area,
    Picture,
}

/// Heights from the area's top and bottom edges, as shares of its height:
/// the bars', or the black borders' around the picture
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Edges {
    top: f64,
    bottom: f64,
}

/// What decides where mpv draws the subtitles, from its options as it
/// started (with an mpv.conf of your own, say)
pub(super) struct SubLayout {
    /// Where they are with the controls hidden
    pub usual: SubPositions,
    /// A line of text subtitles, and their margin from the top and bottom,
    /// as shares of the area's height
    line: f64,
    margin: f64,
    /// Where text subtitles (SRT and the like, and the second ones) go, and
    /// where ASS ones do
    text: Frame,
    ass: Frame,
}

impl SubLayout {
    pub(super) fn read(mpv: &Mpv) -> SubLayout {
        let number = |name, default| {
            mpv.get(name)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(default)
        };
        let frame = |name, default| match mpv.get(name).as_deref().unwrap_or(default) {
            "yes" => Frame::Area,
            _ => Frame::Picture,
        };
        let text = frame("sub-use-margins", "yes");
        // Overridden enough, ASS subtitles are laid out like text ones
        let ass = match mpv.get("sub-ass-override").as_deref() {
            Some("force" | "strip") => text,
            _ => frame("sub-ass-force-margins", "no"),
        };
        // mpv's sizes are for an area 720 pixels high, and scale with it
        SubLayout {
            usual: SubPositions {
                first: number("sub-pos", 100.0),
                second: number("secondary-sub-pos", 0.0),
            },
            line: number("sub-font-size", 38.0) * number("sub-scale", 1.0) / 720.0,
            margin: number("sub-margin-y", 34.0) / 720.0,
            text,
            ass,
        }
    }

    /// Where subtitles of mpv's `codec` go
    fn frame(&self, codec: Option<&str>) -> Frame {
        match codec {
            Some("ass") => self.ass,
            Some(c) if IMAGE_SUBS.contains(&c) => Frame::Picture,
            _ => self.text,
        }
    }

    /// Clear of the `bars`, with the subtitles' frame `first` and the
    /// second ones' `second` (their black borders, none for the area)
    fn clear_of(&self, bars: Edges, first: Edges, second: Edges) -> SubPositions {
        // Under 100, sub-pos raises subtitles from the bottom of their frame
        // by 100 less it, in percent of the frame's height (less a margin,
        // but for pictures). Not ASS lines placed by hand or not at the bottom
        // (signs), nor pictures in the top half. Raise them by what the
        // black border under them doesn't clear already
        let first_height = 1.0 - first.top - first.bottom;
        let raise = 100.0 * (bars.bottom - first.bottom).max(0.0) / first_height;
        let usual = self.usual;
        let first_pos = usual.first.min(100.0 - raise);
        // secondary-sub-pos puts the bottom of the second subtitles' last
        // line that far down their frame less a margin, and they grow
        // upwards from it, stopping at the top
        let second_height = 1.0 - second.top - second.bottom;
        let height = second_height - self.margin;
        let at = |y: f64| 100.0 * (y - second.top) / height;
        let second_pos = if usual.second > 50.0 {
            // Over the first ones: up as far as they go, and above the bar
            let follow = (usual.first - first_pos) * first_height / second_height;
            (usual.second - follow).min(at(1.0 - bars.bottom))
        } else {
            // At the top: below the bar, with room for two lines
            usual.second.max(at(bars.top + 2.0 * self.line))
        };
        SubPositions {
            first: first_pos.clamp(0.0, 150.0),
            second: second_pos.clamp(0.0, 100.0),
        }
    }
}

impl Player {
    /// Subtitles clear of the controls while they're shown, in their usual
    /// places otherwise. Again when the bars, the area, the video or the
    /// subtitles change
    pub(super) fn place_subtitles(&self) {
        let positions = self.clear_positions().unwrap_or(self.sub_layout.usual);
        if self.sub_positions.replace(positions) != positions {
            self.mpv_set("sub-pos", &format!("{:.2}", positions.first));
            self.mpv_set("secondary-sub-pos", &format!("{:.2}", positions.second));
        }
    }

    /// Where the subtitles are clear of the controls, if they're shown
    fn clear_positions(&self) -> Option<SubPositions> {
        let (width, height) = (self.area.width(), f64::from(self.area.height()));
        if !self.state.borrow().controls_shown || width <= 0 || height <= 0.0 {
            return None;
        }
        // Their full height, with the shading over the video
        let bar = |revealer: &gtk::Revealer| {
            f64::from(revealer.measure(gtk::Orientation::Vertical, width).1) / height
        };
        let bars = Edges {
            top: bar(&self.top),
            bottom: bar(&self.bottom),
        };
        let (codec, borders) = {
            let mpv = self.mpv.borrow();
            let mpv = mpv.as_ref()?;
            let pixels = |name: &str| {
                mpv.get(&format!("osd-dimensions/{name}"))
                    .and_then(|v| v.parse::<f64>().ok())
            };
            let total = pixels("h").filter(|h| *h > 0.0);
            let border = |name| total.and_then(|h| Some(pixels(name)? / h)).unwrap_or(0.0);
            let borders = Edges {
                top: border("mt"),
                bottom: border("mb"),
            };
            (mpv.get("current-tracks/sub/codec"), borders)
        };
        let layout = &self.sub_layout;
        let edges = |frame| match frame {
            Frame::Area => Edges::default(),
            Frame::Picture => borders,
        };
        let first = edges(layout.frame(codec.as_deref()));
        Some(layout.clear_of(bars, first, edges(layout.text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BARS: Edges = Edges {
        top: 0.15,
        bottom: 0.2,
    };
    const AREA: Edges = Edges {
        top: 0.0,
        bottom: 0.0,
    };

    fn mpv_defaults() -> SubLayout {
        SubLayout {
            usual: SubPositions {
                first: 100.0,
                second: 0.0,
            },
            line: 38.0 / 720.0,
            margin: 34.0 / 720.0,
            text: Frame::Area,
            ass: Frame::Picture,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn subtitles_rise_by_the_bottom_bar() {
        let clear = mpv_defaults().clear_of(BARS, AREA, AREA);
        assert!(close(clear.first, 80.0));
    }

    #[test]
    fn subtitles_in_the_picture_rise_by_what_the_border_leaves() {
        let layout = mpv_defaults();
        let letterboxed = Edges {
            top: 0.1,
            bottom: 0.1,
        };
        // 0.1 of the 0.2 to clear, a share of the picture's 0.8
        let clear = layout.clear_of(BARS, letterboxed, AREA);
        assert!(close(clear.first, 100.0 - 12.5));
        let deep = Edges {
            top: 0.25,
            bottom: 0.25,
        };
        assert!(close(layout.clear_of(BARS, deep, AREA).first, 100.0));
    }

    #[test]
    fn second_subtitles_drop_below_the_top_bar_with_room_for_two_lines() {
        let layout = mpv_defaults();
        let clear = layout.clear_of(BARS, AREA, AREA);
        // Where the bottom of their last line ends, as a share of the area
        let bottom = clear.second / 100.0 * (1.0 - layout.margin);
        assert!(close(bottom, BARS.top + 2.0 * layout.line));
    }

    #[test]
    fn second_subtitles_over_the_first_rise_with_them() {
        let layout = SubLayout {
            usual: SubPositions {
                first: 100.0,
                second: 85.0,
            },
            ..mpv_defaults()
        };
        let clear = layout.clear_of(BARS, AREA, AREA);
        assert!(close(clear.second, 65.0));
        // With the first ones in a picture whose border clears the bar, they
        // stay, and the second ones only come off the bar
        let deep = Edges {
            top: 0.25,
            bottom: 0.25,
        };
        let clear = layout.clear_of(BARS, deep, AREA);
        assert!(close(clear.first, 100.0));
        assert!(close(clear.second / 100.0 * (1.0 - layout.margin), 0.8));
    }

    #[test]
    fn positions_of_your_own_that_are_already_clear_stay() {
        let layout = SubLayout {
            usual: SubPositions {
                first: 70.0,
                second: 50.0,
            },
            ..mpv_defaults()
        };
        assert_eq!(layout.clear_of(BARS, AREA, AREA), layout.usual);
    }

    #[test]
    fn bars_over_most_of_a_tiny_area_keep_positions_in_range() {
        let bars = Edges {
            top: 1.1,
            bottom: 1.2,
        };
        assert_eq!(
            mpv_defaults().clear_of(bars, AREA, AREA),
            SubPositions {
                first: 0.0,
                second: 100.0
            }
        );
    }

    #[test]
    fn frames_follow_the_codec() {
        let layout = mpv_defaults();
        assert_eq!(layout.frame(Some("subrip")), Frame::Area);
        assert_eq!(layout.frame(Some("ass")), Frame::Picture);
        assert_eq!(layout.frame(Some("hdmv_pgs_subtitle")), Frame::Picture);
        assert_eq!(layout.frame(None), Frame::Area);
    }
}
