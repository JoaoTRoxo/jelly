//! Telling Jellyfin what's playing (resume points, watched state, "now
//! playing"), and keeping the screen on meanwhile.

use super::Player;
use crate::api::{self, TICKS_PER_SECOND};
use adw::prelude::*;
use gtk::gio;
use serde_json::json;
use std::time::Instant;

impl Player {
    pub(super) fn set_inhibit(&self, on: bool) {
        let Some(ctx) = self.ctx.upgrade() else {
            return;
        };
        let mut st = self.state.borrow_mut();
        match (on, st.inhibit) {
            (true, None) => {
                st.inhibit = Some(ctx.app.inhibit(
                    Some(&ctx.window),
                    gtk::ApplicationInhibitFlags::IDLE,
                    Some("Playing a video"),
                ))
            }
            (false, Some(cookie)) => {
                ctx.app.uninhibit(cookie);
                st.inhibit = None;
            }
            _ => {}
        }
    }

    // ---- reporting to Jellyfin ----

    pub(super) fn report(&self, kind: api::Report, event: Option<&str>, blocking: bool) {
        let body = {
            let mut st = self.state.borrow_mut();
            st.last_report = Instant::now();
            let Some(p) = st.playing.as_ref() else { return };
            if !p.started {
                return;
            }
            let selected = |kind: &str| {
                st.tracks
                    .iter()
                    .find(|t| t.kind == kind && t.selected)
                    .and_then(|t| t.jellyfin_index)
            };
            json!({
                "ItemId": p.item.id,
                "MediaSourceId": p.source.id,
                "PlaySessionId": p.play_session_id,
                "PositionTicks": (st.position * TICKS_PER_SECOND) as i64,
                "IsPaused": st.paused,
                "IsMuted": st.muted,
                "VolumeLevel": st.volume.round() as i64,
                "CanSeek": true,
                "PlayMethod": "DirectPlay",
                "AudioStreamIndex": selected("audio"),
                "SubtitleStreamIndex": selected("sub").unwrap_or(-1),
                "EventName": event,
            })
        };
        let Some(client) = self.ctx.upgrade().and_then(|c| c.client()) else {
            return;
        };
        let send = move || {
            if let Err(e) = client.report(kind, body) {
                eprintln!("couldn't report playback ({kind:?}): {e}");
            }
        };
        if blocking {
            send();
        } else {
            gio::spawn_blocking(send);
        }
    }

    pub(super) fn report_stopped(&self, blocking: bool) {
        self.report(api::Report::Stopped, None, blocking);
        let mut st = self.state.borrow_mut();
        if let Some(p) = st.playing.as_mut() {
            p.started = false;
        }
        drop(st);
        if let Some(ctx) = self.ctx.upgrade() {
            ctx.changed();
        }
    }
}
