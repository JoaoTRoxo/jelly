//! Chapters (marked on the seek bar) and the audio and subtitle tracks
//! (matched to Jellyfin's streams, in the menus), with second subtitles on
//! top of the first.

use super::choice::Choice;
use super::{Player, Playing, Track};
use crate::api;
use crate::mpv::Mpv;
use adw::prelude::*;
use std::iter;
use std::rc::Rc;

/// mpv's names for subtitles drawn as pictures
pub(super) const IMAGE_SUBS: [&str; 4] =
    ["hdmv_pgs_subtitle", "dvd_subtitle", "dvb_subtitle", "xsub"];

impl Player {
    // ---- chapters ----

    pub(super) fn read_chapters(&self) {
        let chapters = {
            let mpv = self.mpv.borrow();
            let Some(mpv) = mpv.as_ref() else { return };
            let count: usize = mpv
                .get("chapter-list/count")
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            (0..count)
                .filter_map(|i| {
                    let start = mpv.get(&format!("chapter-list/{i}/time"))?.parse().ok()?;
                    Some((
                        start,
                        mpv.get(&format!("chapter-list/{i}/title"))
                            .unwrap_or_default(),
                    ))
                })
                .collect()
        };
        self.state.borrow_mut().chapters = chapters;
        self.mark_chapters();
    }

    /// A tick on the seek bar where each chapter starts (once the length is known)
    pub(super) fn mark_chapters(&self) {
        self.seek.clear_marks();
        let st = self.state.borrow();
        if st.duration <= 0.0 {
            return;
        }
        // The first chapter starts at the start
        for (start, _) in st
            .chapters
            .iter()
            .filter(|(start, _)| *start > 1.0 && *start < st.duration)
        {
            self.seek.add_mark(*start, gtk::PositionType::Top, None);
        }
    }

    // ---- tracks ----

    /// Read mpv's track list, match it to Jellyfin's streams, and rebuild the menus
    pub(super) fn refresh_tracks(self: &Rc<Self>) {
        let tracks: Vec<Track> = {
            let mpv = self.mpv.borrow();
            let Some(mpv) = mpv.as_ref() else { return };
            let st = self.state.borrow();
            let count: usize = mpv
                .get("track-list/count")
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let second = mpv
                .get("current-tracks/sub2/id")
                .and_then(|id| id.parse().ok());
            (0..count)
                .filter_map(|i| read_track(mpv, st.playing.as_ref(), i, second))
                .collect()
        };
        self.updating.set(true);
        let focus = (
            focused_item(&self.audio_list),
            focused_item(&self.subs_list),
        );
        crate::ui::clear(&self.audio_list);
        let audio = tracks.iter().filter(|t| t.kind == "audio");
        add_choices(
            self,
            &self.audio_list,
            "aid",
            audio.map(|t| (t.label.as_str(), Some(t.id), t.selected)),
        );
        self.fill_subtitles(&tracks);
        refocus(&self.audio_list, focus.0);
        refocus(&self.subs_list, focus.1);
        self.updating.set(false);
        // With one track too: greyed out, it looked broken
        self.audio_button
            .set_sensitive(tracks.iter().any(|t| t.kind == "audio"));
        self.subs_button
            .set_sensitive(tracks.iter().any(|t| t.kind == "sub"));
        self.state.borrow_mut().tracks = tracks;
    }

    /// The subtitle menu: the subtitles, then the second ones, shown on top
    /// (to read two languages at once). Those are text: mpv draws pictures
    /// where they always are, over the first ones
    fn fill_subtitles(self: &Rc<Self>, tracks: &[Track]) {
        let list = &self.subs_list;
        crate::ui::clear(list);
        let subs: Vec<&Track> = tracks.iter().filter(|t| t.kind == "sub").collect();
        let off = |shown: fn(&Track) -> bool| ("Off", None, !subs.iter().any(|t| shown(t)));
        let first = subs
            .iter()
            .map(|t| (t.label.as_str(), Some(t.id), t.selected));
        add_choices(
            self,
            list,
            "sid",
            iter::once(off(|t| t.selected)).chain(first),
        );
        let text: Vec<&Track> = subs
            .iter()
            .copied()
            .filter(|t| !t.image || t.second)
            .collect();
        // Second ones need others to go with
        if subs.len() < 2 || text.is_empty() {
            return;
        }
        list.append(
            &gtk::Separator::builder()
                .margin_top(6)
                .margin_bottom(6)
                .build(),
        );
        list.append(
            &gtk::Label::builder()
                .label("Second Subtitles")
                .xalign(0.0)
                .margin_start(6)
                .margin_bottom(4)
                .css_classes(["heading"])
                .build(),
        );
        let second = text
            .iter()
            .map(|t| (t.label.as_str(), Some(t.id), t.second));
        add_choices(
            self,
            list,
            "secondary-sid",
            iter::once(off(|t| t.second)).chain(second),
        );
    }

    /// Once a seek is done, select image subtitles (Blu-ray's, DVD's) again.
    /// With hardware decoding, mpv (0.41) can otherwise lose the line it
    /// should show at the seek's end, so a seek back often left none until
    /// the next line; selecting them again reads them from where it is.
    pub(super) fn reselect_image_subs(&self) {
        if !std::mem::take(&mut self.state.borrow_mut().seeked) {
            return;
        }
        let (sid, codec) = {
            let mpv = self.mpv.borrow();
            let Some(mpv) = mpv.as_ref() else { return };
            (mpv.get("sid"), mpv.get("current-tracks/sub/codec"))
        };
        let image = codec.is_some_and(|c| IMAGE_SUBS.contains(&c.as_str()));
        if let Some(sid) = sid.filter(|_| image) {
            self.mpv_set("sid", "no");
            self.mpv_set("sid", &sid);
        }
    }

    pub(super) fn select_track(&self, property: &str, id: Option<i64>) {
        let event = if property == "aid" {
            let from = self.playing_stream("audio");
            self.mpv_set("aid", &track_value(id));
            self.audio_picked(from, id);
            "AudioTrackChange"
        } else {
            self.pick_subtitles(property, id);
            "SubtitleTrackChange"
        };
        self.report(api::Report::Progress, Some(event), false);
    }

    /// Subtitles picked as the first ones ("sid") or the second ("secondary-sid"),
    /// remembered for the show. Picked as one, a track stops being the other.
    /// Two are remembered together, so that the first don't follow the audio
    pub(super) fn pick_subtitles(&self, property: &str, id: Option<i64>) {
        let before = self.subtitles_shown();
        let (first, second) = after_pick(before, property, id);
        self.show_subtitles(first, second);
        if property == "sid" || first != before.0 || second.is_some() {
            self.remember_pick("sid", first);
        }
        if property == "secondary-sid" || second != before.1 {
            self.remember_pick("secondary-sid", second);
        }
    }

    /// Show mpv's subtitle tracks `first` and `second` (None: off). mpv
    /// won't select a track that's already the other one: one going from
    /// second to first leaves second before
    pub(super) fn show_subtitles(&self, first: Option<i64>, second: Option<i64>) {
        if first.is_some() && first == self.subtitles_shown().1 {
            self.mpv_set("secondary-sid", "no");
        }
        self.mpv_set("sid", &track_value(first));
        let second = second.filter(|s| Some(*s) != first);
        self.mpv_set("secondary-sid", &track_value(second));
    }

    /// mpv's ids of the subtitles shown: the first ones, and the second
    pub(super) fn subtitles_shown(&self) -> (Option<i64>, Option<i64>) {
        let st = self.state.borrow();
        let subs = || st.tracks.iter().filter(|t| t.kind == "sub");
        (
            subs().find(|t| t.selected).map(|t| t.id),
            subs().find(|t| t.second).map(|t| t.id),
        )
    }
}

/// The subtitles to show (mpv's ids, first and second) after picking `id`
/// for `property` ("sid" or "secondary-sid"): mpv won't show one track
/// twice, so picked as one, it's no longer the other
fn after_pick(
    (first, second): (Option<i64>, Option<i64>),
    property: &str,
    id: Option<i64>,
) -> (Option<i64>, Option<i64>) {
    let unless_picked = |shown: Option<i64>| shown.filter(|s| Some(*s) != id);
    if property == "sid" {
        (id, unless_picked(second))
    } else {
        (unless_picked(first), id)
    }
}

/// mpv's value for a track: its id, or "no"
fn track_value(id: Option<i64>) -> String {
    id.map_or("no".to_string(), |id| id.to_string())
}

/// Track `i` in mpv's list, when it's audio or subtitles, with Jellyfin's
/// index and title for it. `second`: mpv's id of the second subtitles
fn read_track(
    mpv: &Mpv,
    playing: Option<&Playing>,
    i: usize,
    second: Option<i64>,
) -> Option<Track> {
    let get = |field: &str| {
        mpv.get(&format!("track-list/{i}/{field}"))
            .filter(|v| !v.is_empty())
    };
    let kind = get("type").unwrap_or_default();
    if kind != "audio" && kind != "sub" {
        return None;
    }
    let id: i64 = get("id")?.parse().ok()?;
    let jellyfin_index = playing.and_then(|p| {
        if get("external").as_deref() == Some("yes") {
            let file = get("external-filename")?;
            p.external_subs
                .iter()
                .find(|(url, _)| *url == file)
                .map(|(_, index)| *index)
        } else {
            let ff_index: i32 = get("ff-index")?.parse().ok()?;
            let kind_name = if kind == "audio" { "Audio" } else { "Subtitle" };
            p.source
                .media_streams
                .iter()
                .find(|s| s.index == ff_index && s.kind == kind_name && !s.is_external)
                .map(|s| s.index)
        }
    });
    let jellyfin_title = playing.and_then(|p| {
        let index = jellyfin_index?;
        p.source
            .media_streams
            .iter()
            .find(|s| s.index == index)?
            .display_title
            .clone()
    });
    let codec = get("codec");
    let label = jellyfin_title.unwrap_or_else(|| {
        let mut parts: Vec<String> = Vec::new();
        if let Some(title) = get("title") {
            parts.push(title);
        }
        if let Some(lang) = get("lang") {
            parts.push(lang);
        }
        if let Some(codec) = &codec {
            parts.push(codec.to_uppercase());
        }
        if parts.is_empty() {
            format!("Track {id}")
        } else {
            parts.join(" · ")
        }
    });
    let second = kind == "sub" && Some(id) == second;
    Some(Track {
        selected: get("selected").as_deref() == Some("yes") && !second,
        second,
        image: codec.is_some_and(|c| IMAGE_SUBS.contains(&c.as_str())),
        kind,
        id,
        label,
        jellyfin_index,
    })
}

/// The chapter at `time`, when it has a name: not when it's only numbered
/// ("Chapter 01", "Scene 3", "00:05:00.000"), which says nothing the time
/// doesn't
pub fn chapter_name(chapters: &[(f64, String)], time: f64) -> Option<&str> {
    let (_, title) = chapters.iter().rev().find(|(start, _)| *start <= time)?;
    let title = title.trim();
    let number = title
        .trim_start_matches(char::is_alphabetic)
        .trim_start_matches(['.', '#', ' ']);
    let numbered = !number.is_empty()
        && number
            .chars()
            .all(|c| c.is_ascii_digit() || c == ':' || c == '.');
    (!title.is_empty() && !numbered).then_some(title)
}

pub fn scrolling(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .max_content_height(420)
        .build()
}

/// A menu's radio buttons for `property` ("aid", "sid" or "secondary-sid"):
/// each a label, mpv's track id (None: off), and whether it's the one shown
fn add_choices<'a>(
    player: &Rc<Player>,
    list: &gtk::Box,
    property: &'static str,
    choices: impl IntoIterator<Item = (&'a str, Option<i64>, bool)>,
) {
    let mut group: Option<Choice> = None;
    for (label, id, active) in choices {
        let button = Choice::new(label, active);
        match &group {
            Some(g) => button.set_group(Some(g)),
            None => group = Some(button.clone()),
        }
        let weak = Rc::downgrade(player);
        button.connect_toggled(move |b| {
            if let Some(p) = weak.upgrade()
                && b.is_active()
                && !p.updating.get()
            {
                p.select_track(property, id);
            }
        });
        list.append(&button);
    }
}

/// Which of `list`'s items has the focus
fn focused_item(list: &gtk::Box) -> Option<usize> {
    let focus = list.focus_child()?;
    items(list).position(|item| item == focus)
}

/// The focus back on item `index` of `list`, rebuilt by the track picked
/// from it: where the arrow keys and Enter left it
fn refocus(list: &gtk::Box, index: Option<usize>) {
    if let Some(item) = index.and_then(|i| items(list).nth(i)) {
        item.grab_focus();
    }
}

fn items(list: &gtk::Box) -> impl Iterator<Item = gtk::Widget> {
    iter::successors(list.first_child(), |item| item.next_sibling())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapters(titles: &[&str]) -> Vec<(f64, String)> {
        titles
            .iter()
            .enumerate()
            .map(|(i, t)| (i as f64 * 100.0, t.to_string()))
            .collect()
    }

    #[test]
    fn a_track_picked_as_one_subtitles_stops_being_the_other() {
        // Japanese (2) under English (3), on top
        let shown = (Some(2), Some(3));
        assert_eq!(after_pick(shown, "sid", Some(4)), (Some(4), Some(3)));
        assert_eq!(after_pick(shown, "secondary-sid", None), (Some(2), None));
        assert_eq!(after_pick(shown, "sid", None), (None, Some(3)));
        // English moves down, or Japanese up
        assert_eq!(after_pick(shown, "sid", Some(3)), (Some(3), None));
        assert_eq!(after_pick(shown, "secondary-sid", Some(2)), (None, Some(2)));
        assert_eq!(
            after_pick((None, None), "secondary-sid", Some(2)),
            (None, Some(2))
        );
    }

    #[test]
    fn names_chapters_with_a_name() {
        let named = chapters(&["Prologue", "Opening", "Parte A", "1. Ending"]);
        assert_eq!(chapter_name(&named, 50.0), Some("Prologue"));
        assert_eq!(chapter_name(&named, 250.0), Some("Parte A"));
        assert_eq!(chapter_name(&named, 1000.0), Some("1. Ending"));
    }

    #[test]
    fn leaves_out_numbered_chapters() {
        let numbered = chapters(&["Chapter 01", "Scene 2", "Ch. 3", "Capítulo 4", "7", ""]);
        for time in [0.0, 100.0, 200.0, 300.0, 400.0, 500.0] {
            assert_eq!(chapter_name(&numbered, time), None, "at {time}");
        }
        assert_eq!(chapter_name(&chapters(&["00:05:00.000"]), 10.0), None);
        // Before the first chapter
        assert_eq!(chapter_name(&[(30.0, "Opening".into())], 10.0), None);
    }
}
