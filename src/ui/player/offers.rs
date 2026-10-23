//! What to offer during playback: "Skip Intro" (and recaps, previews and
//! credits) inside a segment, and the next episode's card over the credits.

use super::Player;
use crate::api;
use adw::prelude::*;
use std::rc::Rc;

/// Offer the next episode this close to the end when there's no credits segment
const NEXT_UP_BEFORE_END: f64 = 30.0;
/// How long the next episode's card counts down over the credits
const UP_NEXT_COUNTDOWN: f64 = 15.0;

/// The next episode's card
#[derive(Debug, Clone, PartialEq)]
enum UpNext {
    /// Counting down: seconds left, how far along (0 to 1), and whether over the credits
    Countdown {
        seconds: f64,
        fraction: f64,
        credits: bool,
    },
    /// The countdown ran out
    Now,
}

/// What to offer at `position`: a skip button (label, and where it skips to)
/// and the next episode's card. `declined`: the card was dismissed.
fn offers(
    position: f64,
    duration: f64,
    segments: &[api::Segment],
    has_next: bool,
    declined: bool,
) -> (Option<(String, f64)>, Option<UpNext>) {
    let outro = segments.iter().find(|s| s.kind == "Outro");
    let up_next = if !has_next || declined || duration <= 0.0 {
        None
    } else {
        // Shown from `from`, plays the next episode at `until`, and leaves
        // what's after the credits (`end`) alone
        let (from, until, end) = match outro {
            Some(s) => (
                s.start(),
                (s.start() + UP_NEXT_COUNTDOWN).min(s.end()),
                s.end(),
            ),
            None => (duration - NEXT_UP_BEFORE_END, duration, duration),
        };
        if position < from || position >= end {
            None
        } else if position >= until {
            Some(UpNext::Now)
        } else {
            Some(UpNext::Countdown {
                seconds: until - position,
                fraction: (position - from) / (until - from),
                credits: outro.is_some(),
            })
        }
    };
    // With a next episode, its card takes the credits' place
    let skip = segments
        .iter()
        .find(|s| position >= s.start() && position < s.end() - 1.0)
        .filter(|s| !(s.kind == "Outro" && has_next) && up_next.is_none())
        .map(|s| {
            let label = match s.kind.as_str() {
                "Intro" => "Skip Intro",
                "Recap" => "Skip Recap",
                "Preview" => "Skip Preview",
                "Outro" => "Skip Credits",
                _ => "Skip",
            };
            (label.to_string(), s.end())
        });
    (skip, up_next)
}

impl Player {
    /// Show "Skip Intro" (etc.) inside a segment, and over the credits (or
    /// near the end) the next episode's card, playing it when its countdown ends
    pub(super) fn update_offers(self: &Rc<Self>) {
        let (skip, up_next) = {
            let st = self.state.borrow();
            offers(
                st.position,
                st.duration,
                &st.segments,
                st.next.is_some(),
                st.next_declined,
            )
        };
        if self.state.borrow().skip != skip {
            match &skip {
                Some((label, _)) => {
                    self.skip_button.set_label(label);
                    self.skip_button.set_visible(true);
                }
                None => self.skip_button.set_visible(false),
            }
            self.state.borrow_mut().skip = skip;
        }
        match up_next {
            Some(UpNext::Now) => self.play_next(),
            Some(UpNext::Countdown {
                seconds,
                fraction,
                credits,
            }) => {
                let card = &self.up_next;
                card.countdown
                    .set_label(&format!("Starts in {} s", seconds.ceil() as i64));
                card.progress.set_fraction(fraction);
                card.dismiss
                    .set_label(if credits { "Watch Credits" } else { "Cancel" });
                card.root.set_visible(true);
            }
            None => self.up_next.root.set_visible(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TICKS_PER_SECOND;

    fn segment(kind: &str, start: f64, end: f64) -> api::Segment {
        api::Segment {
            kind: kind.into(),
            start_ticks: (start * TICKS_PER_SECOND) as i64,
            end_ticks: (end * TICKS_PER_SECOND) as i64,
        }
    }

    #[test]
    fn skips_intros() {
        let segments = [segment("Intro", 60.0, 150.0)];
        assert_eq!(
            offers(70.0, 1400.0, &segments, true, false),
            (Some(("Skip Intro".into(), 150.0)), None)
        );
        assert_eq!(offers(200.0, 1400.0, &segments, true, false), (None, None));
    }

    #[test]
    fn counts_down_over_the_credits() {
        let segments = [segment("Outro", 1300.0, 1380.0)];
        assert_eq!(offers(1290.0, 1400.0, &segments, true, false), (None, None));
        assert_eq!(
            offers(1305.0, 1400.0, &segments, true, false),
            (
                None,
                Some(UpNext::Countdown {
                    seconds: 10.0,
                    fraction: 5.0 / 15.0,
                    credits: true
                })
            )
        );
        assert_eq!(
            offers(1315.0, 1400.0, &segments, true, false),
            (None, Some(UpNext::Now))
        );
        // After the credits (a scene there): left alone
        assert_eq!(offers(1385.0, 1400.0, &segments, true, false), (None, None));
    }

    #[test]
    fn watching_the_credits() {
        let segments = [segment("Outro", 1300.0, 1380.0)];
        assert_eq!(offers(1305.0, 1400.0, &segments, true, true), (None, None));
        // The last episode: just a skip button
        assert_eq!(
            offers(1305.0, 1400.0, &segments, false, false),
            (Some(("Skip Credits".into(), 1380.0)), None)
        );
    }

    #[test]
    fn counts_down_to_the_end_without_credits() {
        assert_eq!(offers(1360.0, 1400.0, &[], true, false), (None, None));
        assert_eq!(
            offers(1380.0, 1400.0, &[], true, false),
            (
                None,
                Some(UpNext::Countdown {
                    seconds: 20.0,
                    fraction: 10.0 / 30.0,
                    credits: false
                })
            )
        );
        assert_eq!(offers(1380.0, 1400.0, &[], false, false), (None, None));
    }
}
