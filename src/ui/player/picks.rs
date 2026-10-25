//! The audio and subtitles picked in a show, picked again in its other
//! episodes. Jellyfin's own choice follows the account's languages, and
//! remembers a pick only for the episode it was made in.

use super::Player;
use crate::api;
use crate::session::{self, ShowTracks, Subtitles, Track};

/// The audio and subtitle streams (Jellyfin's indexes) to start with: as
/// last picked in the show, else as Jellyfin chose. No subtitles: off.
pub(super) fn streams(
    source: &api::MediaSource,
    picked: Option<&ShowTracks>,
) -> (Option<i32>, Option<i32>) {
    let audio = source.default_audio_stream_index;
    let subtitles = source.default_subtitle_stream_index;
    let Some(picked) = picked else {
        return (audio, subtitles);
    };
    let audio = picked
        .audio
        .as_ref()
        .and_then(|track| like(track, &source.media_streams, "Audio", audio))
        .or(audio);
    let subtitles = match &picked.subtitles {
        Some(Subtitles::Off) => None,
        Some(Subtitles::On(track)) => {
            like(track, &source.media_streams, "Subtitle", subtitles).or(subtitles)
        }
        None => subtitles,
    };
    (audio, subtitles)
}

/// The stream most like `track`: in its language, then with its title,
/// then Jellyfin's choice (`default`). None without one in its language.
fn like(
    track: &Track,
    streams: &[api::MediaStream],
    kind: &str,
    default: Option<i32>,
) -> Option<i32> {
    let same = |a: &Option<String>, b: &Option<String>| {
        a.as_ref()
            .zip(b.as_ref())
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
    };
    streams
        .iter()
        .filter(|s| s.kind == kind)
        .filter(|s| track.language.is_none() || same(&s.language, &track.language))
        .max_by_key(|s| {
            (
                same(&s.title, &track.title),
                Some(s.index) == default,
                std::cmp::Reverse(s.index),
            )
        })
        .map(|s| s.index)
}

impl Player {
    /// The tracks last picked in this episode's show
    pub(super) fn show_picks(&self) -> Option<ShowTracks> {
        let st = self.state.borrow();
        let show = st.playing.as_ref()?.item.series_id.as_ref()?;
        session::settings().show_tracks.remove(show)
    }

    /// Remember a track picked (by mpv's id, "audio" or "sub"; None: off)
    /// for the show's other episodes
    pub(super) fn remember_pick(&self, kind: &str, id: Option<i64>) {
        let (show, track) = {
            let st = self.state.borrow();
            let Some(playing) = st.playing.as_ref() else {
                return;
            };
            let Some(show) = playing.item.series_id.clone() else {
                return;
            };
            let track = match id {
                Some(id) => {
                    let index = st
                        .tracks
                        .iter()
                        .find(|t| t.kind == kind && t.id == id)
                        .and_then(|t| t.jellyfin_index);
                    let stream = playing
                        .source
                        .media_streams
                        .iter()
                        .find(|s| Some(s.index) == index);
                    let Some(stream) = stream else { return };
                    Some(Track {
                        language: stream.language.clone(),
                        title: stream.title.clone(),
                    })
                }
                None => None,
            };
            (show, track)
        };
        let mut settings = session::settings();
        let picked = settings.show_tracks.entry(show).or_default();
        match (kind, track) {
            ("audio", Some(track)) => picked.audio = Some(track),
            ("sub", track) => picked.subtitles = Some(track.map_or(Subtitles::Off, Subtitles::On)),
            _ => return,
        }
        session::save_settings(&settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(index: i32, kind: &str, language: &str, title: &str) -> api::MediaStream {
        let text = |s: &str| (!s.is_empty()).then(|| s.to_string());
        api::MediaStream {
            index,
            kind: kind.into(),
            language: text(language),
            title: text(title),
            ..Default::default()
        }
    }

    fn track(language: &str, title: &str) -> Track {
        Track {
            language: Some(language.into()),
            title: (!title.is_empty()).then(|| title.into()),
        }
    }

    /// An episode as its show has them: English first, Japanese second, and
    /// two English subtitles, for the dub's signs and for the dialogue
    fn episode() -> Vec<api::MediaStream> {
        vec![
            stream(0, "Video", "", ""),
            stream(1, "Audio", "eng", ""),
            stream(2, "Audio", "jpn", ""),
            stream(3, "Subtitle", "eng", "Signs/Songs"),
            stream(4, "Subtitle", "eng", "Dialogue"),
        ]
    }

    #[test]
    fn picks_the_language_then_the_title() {
        let streams = episode();
        assert_eq!(like(&track("jpn", ""), &streams, "Audio", Some(1)), Some(2));
        assert_eq!(like(&track("JPN", ""), &streams, "Audio", Some(1)), Some(2));
        assert_eq!(
            like(&track("eng", "Dialogue"), &streams, "Subtitle", Some(3)),
            Some(4)
        );
        // A title this episode doesn't have: Jellyfin's choice, in the language
        assert_eq!(
            like(&track("eng", "Full"), &streams, "Subtitle", Some(3)),
            Some(3)
        );
        assert_eq!(
            like(&track("eng", "Full"), &streams, "Subtitle", None),
            Some(3)
        );
        // A language it doesn't have: nothing
        assert_eq!(like(&track("por", ""), &streams, "Audio", Some(1)), None);
    }

    #[test]
    fn starts_as_last_picked_in_the_show() {
        let source = api::MediaSource {
            media_streams: episode(),
            default_audio_stream_index: Some(1),
            default_subtitle_stream_index: Some(3),
            ..Default::default()
        };
        assert_eq!(streams(&source, None), (Some(1), Some(3)));
        let mut picked = ShowTracks {
            audio: Some(track("jpn", "")),
            subtitles: Some(Subtitles::On(track("eng", "Dialogue"))),
        };
        assert_eq!(streams(&source, Some(&picked)), (Some(2), Some(4)));
        picked.subtitles = Some(Subtitles::Off);
        assert_eq!(streams(&source, Some(&picked)), (Some(2), None));
        // Picked only the audio, or a language this episode doesn't have
        picked.subtitles = None;
        picked.audio = Some(track("por", ""));
        assert_eq!(streams(&source, Some(&picked)), (Some(1), Some(3)));
    }

    #[test]
    fn kept_in_the_settings() {
        let picked = ShowTracks {
            audio: Some(track("jpn", "")),
            subtitles: Some(Subtitles::Off),
        };
        let json = serde_json::to_string(&picked).unwrap();
        assert_eq!(
            json,
            r#"{"audio":{"language":"jpn","title":null},"subtitles":"off"}"#
        );
        assert_eq!(serde_json::from_str::<ShowTracks>(&json).unwrap(), picked);
        let on = r#"{"subtitles":{"on":{"language":"eng","title":"Dialogue"}}}"#;
        assert_eq!(
            serde_json::from_str::<ShowTracks>(on).unwrap().subtitles,
            Some(Subtitles::On(track("eng", "Dialogue")))
        );
    }

    #[test]
    fn a_track_without_a_language_goes_by_title() {
        let streams = vec![
            stream(1, "Audio", "", "Original"),
            stream(2, "Audio", "", "Commentary"),
        ];
        let commentary = Track {
            language: None,
            title: Some("Commentary".into()),
        };
        assert_eq!(like(&commentary, &streams, "Audio", Some(1)), Some(2));
    }
}
