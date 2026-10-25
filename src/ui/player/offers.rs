//! What to offer during playback: "Skip Intro" (and recaps, previews and
//! credits) inside a segment, and once the credits are over, the next
//! episode's screen, counting down to it.

use super::Player;
use crate::api;
use adw::prelude::*;
use std::rc::Rc;

/// How long the next episode's screen counts down, in seconds
pub(super) const NEXT_COUNTDOWN: f64 = 10.0;

/// What to offer at `position`: a skip button (label, and where it skips
/// to), and whether the credits are over: after the credits segment, or at
/// the end (`ended`) without one.
fn offers(position: f64, segments: &[api::Segment], ended: bool) -> (Option<(String, f64)>, bool) {
    let outro = segments.iter().find(|s| s.kind == "Outro");
    let after_credits = ended || outro.is_some_and(|s| position >= s.end());
    let skip = segments
        .iter()
        .find(|s| position >= s.start() && position < s.end() - 1.0)
        .map(|s| {
            let label = match s.kind.as_str() {
                "Intro" => "Skip Intro",
                "Recap" => "Skip Recap",
                "Preview" => "Skip Preview",
                "Outro" => "Skip Outro",
                _ => "Skip",
            };
            (label.to_string(), s.end())
        });
    (skip, after_credits)
}

impl Player {
    /// Show "Skip Intro" (etc.) inside a segment, and once the credits are
    /// over, the next episode's screen (see count_down). A seek there starts
    /// its countdown from the top, so it never plays the next one at once.
    pub(super) fn update_offers(self: &Rc<Self>) {
        let (skip, ask, start) = {
            let mut st = self.state.borrow_mut();
            let (skip, after_credits) = offers(st.position, &st.segments, st.ended);
            // Back before the end of the credits: ask again when it comes
            if !after_credits {
                st.next_declined = false;
            }
            let ask = after_credits && st.next.is_some() && !st.next_declined;
            let start = ask && st.next_countdown.is_none();
            if start {
                st.next_countdown = Some(NEXT_COUNTDOWN);
            } else if !ask {
                st.next_countdown = None;
            }
            (skip.filter(|_| !ask), ask, start)
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
        if start {
            self.show_countdown(NEXT_COUNTDOWN);
        }
        self.up_next.root.set_visible(ask);
    }

    /// The next episode's countdown, `seconds` on: held while paused (not
    /// after the end), and playing it when it runs out
    pub(super) fn count_down(self: &Rc<Self>, seconds: f64) {
        let left = {
            let mut st = self.state.borrow_mut();
            let running = !st.paused || st.ended;
            match st.next_countdown.as_mut() {
                Some(left) if running => {
                    *left -= seconds;
                    *left
                }
                _ => return,
            }
        };
        if left <= 0.0 {
            self.play_next();
        } else {
            self.show_countdown(left);
        }
    }

    fn show_countdown(&self, left: f64) {
        let screen = &self.up_next;
        screen
            .countdown
            .set_label(&format!("Starts in {} s", left.ceil() as i64));
        screen.progress.set_fraction(1.0 - left / NEXT_COUNTDOWN);
    }

    /// Cancel on the next episode's screen: back to the video, or out of
    /// the player if it's over
    pub(super) fn decline_next(self: &Rc<Self>) {
        let ended = {
            let mut st = self.state.borrow_mut();
            st.next_declined = true;
            st.ended
        };
        if ended {
            self.close();
        } else {
            self.update_offers();
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
            offers(70.0, &segments, false),
            (Some(("Skip Intro".into(), 150.0)), false)
        );
        assert_eq!(offers(200.0, &segments, false), (None, false));
    }

    #[test]
    fn skips_the_credits_then_asks() {
        let segments = [segment("Outro", 1300.0, 1380.0)];
        assert_eq!(offers(1290.0, &segments, false), (None, false));
        assert_eq!(
            offers(1305.0, &segments, false),
            (Some(("Skip Outro".into(), 1380.0)), false)
        );
        // Where Skip Outro goes, or a seek past the credits
        assert_eq!(offers(1380.0, &segments, false), (None, true));
        assert_eq!(offers(1395.0, &segments, false), (None, true));
    }

    #[test]
    fn asks_at_the_end_without_credits() {
        assert_eq!(offers(1390.0, &[], false), (None, false));
        assert_eq!(offers(1400.0, &[], true), (None, true));
        // Credits to the very end
        let segments = [segment("Outro", 1300.0, 1400.0)];
        assert!(!offers(1399.5, &segments, false).1);
        assert!(offers(1400.0, &segments, true).1);
    }
}
