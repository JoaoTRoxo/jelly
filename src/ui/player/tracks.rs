//! Chapters (marked on the seek bar) and the audio and subtitle tracks
//! (matched to Jellyfin's streams, in the menus).

use super::{Player, Track};
use crate::api;
use adw::prelude::*;
use std::rc::Rc;

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
        let tracks = {
            let mpv = self.mpv.borrow();
            let Some(mpv) = mpv.as_ref() else { return };
            let st = self.state.borrow();
            let count: usize = mpv
                .get("track-list/count")
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let mut tracks = Vec::new();
            for i in 0..count {
                let get = |field: &str| {
                    mpv.get(&format!("track-list/{i}/{field}"))
                        .filter(|v| !v.is_empty())
                };
                let kind = get("type").unwrap_or_default();
                if kind != "audio" && kind != "sub" {
                    continue;
                }
                let Some(id) = get("id").and_then(|v| v.parse().ok()) else {
                    continue;
                };
                let jellyfin_index = st.playing.as_ref().and_then(|p| {
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
                let jellyfin_title = st.playing.as_ref().and_then(|p| {
                    let index = jellyfin_index?;
                    p.source
                        .media_streams
                        .iter()
                        .find(|s| s.index == index)?
                        .display_title
                        .clone()
                });
                let label = jellyfin_title.unwrap_or_else(|| {
                    let mut parts: Vec<String> = Vec::new();
                    if let Some(title) = get("title") {
                        parts.push(title);
                    }
                    if let Some(lang) = get("lang") {
                        parts.push(lang);
                    }
                    if let Some(codec) = get("codec") {
                        parts.push(codec.to_uppercase());
                    }
                    if parts.is_empty() {
                        format!("Track {id}")
                    } else {
                        parts.join(" · ")
                    }
                });
                tracks.push(Track {
                    kind,
                    id,
                    selected: get("selected").as_deref() == Some("yes"),
                    label,
                    jellyfin_index,
                });
            }
            tracks
        };
        self.updating.set(true);
        fill_track_list(self, &self.audio_list, &tracks, "audio", None);
        fill_track_list(self, &self.subs_list, &tracks, "sub", Some("Off"));
        self.updating.set(false);
        let audio_count = tracks.iter().filter(|t| t.kind == "audio").count();
        self.audio_button.set_sensitive(audio_count > 1);
        self.subs_button
            .set_sensitive(tracks.iter().any(|t| t.kind == "sub"));
        self.state.borrow_mut().tracks = tracks;
    }

    pub(super) fn select_track(&self, property: &str, id: Option<i64>) {
        self.mpv_set(property, &id.map_or("no".to_string(), |i| i.to_string()));
        self.report(
            api::Report::Progress,
            Some(if property == "aid" {
                "AudioTrackChange"
            } else {
                "SubtitleTrackChange"
            }),
            false,
        );
    }

    pub(super) fn cycle_track(self: &Rc<Self>, property: &str) {
        self.mpv_command(&["cycle", property]);
        self.refresh_tracks();
        let kind = if property == "aid" { "audio" } else { "sub" };
        let selected = self
            .state
            .borrow()
            .tracks
            .iter()
            .find(|t| t.kind == kind && t.selected)
            .map(|t| t.label.clone());
        let what = if property == "aid" {
            "Audio"
        } else {
            "Subtitles"
        };
        self.osd(&format!(
            "{what}: {}",
            selected.unwrap_or_else(|| "Off".into())
        ));
        self.report(
            api::Report::Progress,
            Some(if property == "aid" {
                "AudioTrackChange"
            } else {
                "SubtitleTrackChange"
            }),
            false,
        );
    }
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

fn fill_track_list(
    player: &Rc<Player>,
    list: &gtk::Box,
    tracks: &[Track],
    kind: &'static str,
    off_label: Option<&str>,
) {
    crate::ui::clear(list);
    let property = if kind == "audio" { "aid" } else { "sid" };
    let mut group: Option<gtk::CheckButton> = None;
    let mut add = |label: &str, id: Option<i64>, active: bool| {
        let button = gtk::CheckButton::builder()
            .label(label)
            .active(active)
            .build();
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
    };
    let of_kind: Vec<&Track> = tracks.iter().filter(|t| t.kind == kind).collect();
    if let Some(off) = off_label {
        add(off, None, !of_kind.iter().any(|t| t.selected));
    }
    for t in of_kind {
        add(&t.label, Some(t.id), t.selected);
    }
}
