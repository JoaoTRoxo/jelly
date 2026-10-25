//! The video player: mpv drawing into a GLArea, with our own controls on top.
//! Playback is reported to Jellyfin (resume points, watched state, "now
//! playing"), intro/recap segments get a skip button, and over the credits a
//! card counts down to the next episode.

mod controls;
mod offers;
mod picks;
mod report;
mod tracks;

use super::Ctx;
use crate::api;
use crate::mpv::{self, EndReason, Event, Format, Mpv, Renderer, Value};
use crate::session;
use adw::prelude::*;
use controls::{Controls, UpNextCard};
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

const HIDE_CONTROLS_AFTER: Duration = Duration::from_secs(3);
const REPORT_EVERY: Duration = Duration::from_secs(10);

/// Hardware decoding. On Windows only Direct3D 11's, copied back to memory:
/// OpenGL can't take its frames directly, and trying DXVA2 (Direct3D 9) can
/// crash the app
const HWDEC: &str = if cfg!(windows) {
    "d3d11va-copy"
} else {
    "auto-safe"
};

thread_local! {
    static CURRENT: RefCell<Option<Rc<Player>>> = const { RefCell::new(None) };
}

pub enum Media {
    /// A local file or URL (for testing without a server)
    File(String),
    Jellyfin {
        item: Box<api::Item>,
        start: f64,
    },
}

pub fn open(ctx: &Rc<Ctx>, media: Media) {
    if let Some(player) = CURRENT.with(|c| c.borrow().clone()) {
        player.load(media);
        return;
    }
    match Player::new(ctx) {
        Ok(player) => {
            CURRENT.with(|c| *c.borrow_mut() = Some(player.clone()));
            ctx.nav.push(&player.page);
            player.load(media);
        }
        Err(e) => ctx.toast(&format!("Couldn't start the video player: {e}")),
    }
}

/// Stop playback, reporting where it stopped (when the app quits)
pub fn shutdown_current() {
    if let Some(player) = CURRENT.with(|c| c.borrow_mut().take()) {
        player.shutdown(true);
    }
}

struct Track {
    kind: String,
    id: i64,
    selected: bool,
    label: String,
    jellyfin_index: Option<i32>,
}

/// What's playing, as Jellyfin knows it
struct Playing {
    item: api::Item,
    source: api::MediaSource,
    play_session_id: Option<String>,
    /// URL and Jellyfin stream index of each external subtitle
    external_subs: Vec<(String, i32)>,
    started: bool,
}

struct State {
    playing: Option<Playing>,
    load_id: u64,
    position: f64,
    duration: f64,
    paused: bool,
    loading: bool,
    waiting_for_cache: bool,
    seeking: bool,
    volume: f64,
    muted: bool,
    tracks: Vec<Track>,
    segments: Vec<api::Segment>,
    /// Start and title of each chapter
    chapters: Vec<(f64, String)>,
    previous: Option<api::Item>,
    next: Option<api::Item>,
    /// The next episode's card was dismissed: don't play it at the end either
    next_declined: bool,
    skip: Option<(String, f64)>,
    last_motion: Instant,
    pointer: (f64, f64),
    /// Where the pointer is on the seek bar (its x there), if it is
    seek_pointer: Option<f64>,
    over_controls: bool,
    seek_hold_until: Instant,
    /// A seek was asked for: see reselect_image_subs
    seeked: bool,
    controls_shown: bool,
    inhibit: Option<u32>,
    last_report: Instant,
    /// A file to open once the video surface exists (mpv can't show video before that)
    pending_file: Option<String>,
    closed: bool,
}

impl State {
    fn new(volume: f64) -> State {
        let now = Instant::now();
        State {
            playing: None,
            load_id: 0,
            position: 0.0,
            duration: 0.0,
            paused: false,
            loading: true,
            waiting_for_cache: false,
            seeking: false,
            volume,
            muted: false,
            tracks: Vec::new(),
            segments: Vec::new(),
            chapters: Vec::new(),
            previous: None,
            next: None,
            next_declined: false,
            skip: None,
            last_motion: now,
            pointer: (-1.0, -1.0),
            seek_pointer: None,
            over_controls: false,
            seek_hold_until: now,
            seeked: false,
            controls_shown: true,
            inhibit: None,
            last_report: now,
            pending_file: None,
            closed: false,
        }
    }
}

pub struct Player {
    ctx: Weak<Ctx>,
    pub page: adw::NavigationPage,
    overlay: gtk::Overlay,
    area: gtk::GLArea,
    mpv: RefCell<Option<Mpv>>,
    renderer: RefCell<Option<Renderer>>,
    top: gtk::Revealer,
    bottom: gtk::Revealer,
    title: gtk::Label,
    subtitle: gtk::Label,
    play_button: gtk::Button,
    seek: gtk::Scale,
    seek_time: gtk::Label,
    position_label: gtk::Label,
    duration_label: gtk::Label,
    volume_button: gtk::MenuButton,
    volume_scale: gtk::Scale,
    audio_button: gtk::MenuButton,
    audio_list: gtk::Box,
    subs_button: gtk::MenuButton,
    subs_list: gtk::Box,
    previous_button: gtk::Button,
    next_button: gtk::Button,
    fullscreen_button: gtk::Button,
    /// "Ends at 23:14"
    ends: gtk::Label,
    skip_button: gtk::Button,
    up_next: UpNextCard,
    spinner: adw::Spinner,
    error: adw::StatusPage,
    fullscreen_handler: RefCell<Option<glib::SignalHandlerId>>,
    /// Set while we change widgets, so their signals don't act on mpv
    updating: Cell<bool>,
    state: RefCell<State>,
}

/// The Wayland or X11 display, so mpv can show hardware-decoded frames
/// without copying them (Windows and macOS manage without)
#[cfg(all(unix, not(target_os = "macos")))]
fn native_display(widget: &impl IsA<gtk::Widget>) -> Option<mpv::NativeDisplay> {
    use gtk::glib::translate::ToGlibPtr;
    use std::ffi::c_void;
    unsafe extern "C" {
        fn gdk_wayland_display_get_wl_display(
            display: *mut gtk::gdk::ffi::GdkDisplay,
        ) -> *mut c_void;
        fn gdk_x11_display_get_xdisplay(display: *mut gtk::gdk::ffi::GdkDisplay) -> *mut c_void;
    }
    let display = widget.display();
    let raw: *mut gtk::gdk::ffi::GdkDisplay = display.to_glib_none().0;
    let native = match display.type_().name() {
        "GdkWaylandDisplay" => {
            mpv::NativeDisplay::Wayland(unsafe { gdk_wayland_display_get_wl_display(raw) })
        }
        "GdkX11Display" => mpv::NativeDisplay::X11(unsafe { gdk_x11_display_get_xdisplay(raw) }),
        _ => return None,
    };
    match native {
        mpv::NativeDisplay::Wayland(p) | mpv::NativeDisplay::X11(p) if p.is_null() => None,
        _ => Some(native),
    }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn native_display(_widget: &impl IsA<gtk::Widget>) -> Option<mpv::NativeDisplay> {
    None
}

/// mpv, set up to play into the app (and its config folder's mpv.conf)
fn start_mpv(volume: f64) -> Result<Mpv, String> {
    let mpv_dir = session::config_dir().join("mpv");
    let volume = volume.to_string();
    let user_agent = format!("{}/{}", api::CLIENT_NAME, api::CLIENT_VERSION);
    let log_level = std::env::var("JELLY_MPV_LOG").ok();
    let mut options = vec![
        ("vo", "libmpv"),
        ("hwdec", HWDEC),
        // An mpv.conf in ~/.config/jelly/mpv is applied, if you make one
        ("config-dir", mpv_dir.to_str().unwrap_or("")),
        ("config", "yes"),
        ("load-scripts", "no"),
        ("ytdl", "no"),
        ("input-default-bindings", "no"),
        ("input-vo-keyboard", "no"),
        ("idle", "yes"),
        ("keep-open", "no"),
        ("osd-bar", "no"),
        ("sub-auto", "no"),
        ("audio-file-auto", "no"),
        ("cache", "yes"),
        ("demuxer-max-bytes", "300MiB"),
        ("demuxer-max-back-bytes", "100MiB"),
        ("network-timeout", "30"),
        // mpv takes any certificate unless told otherwise, and its requests
        // carry the Jellyfin token
        ("tls-verify", "yes"),
        ("audio-client-name", api::CLIENT_NAME),
        // Keep the video going if the sound server is unavailable
        ("audio-fallback-to-null", "yes"),
        ("user-agent", &user_agent),
        ("volume", &volume),
    ];
    match &log_level {
        Some(level) => options.extend([("terminal", "yes"), ("msg-level", level.as_str())]),
        None => options.push(("terminal", "no")),
    }
    // For debugging: JELLY_MPV_OPTIONS="sid=1,hwdec=no"
    let extra = std::env::var("JELLY_MPV_OPTIONS").unwrap_or_default();
    options.extend(extra.split(',').filter_map(|kv| kv.split_once('=')));
    let mpv = Mpv::new(&options)?;
    for (name, format) in [
        ("time-pos", Format::Double),
        ("duration", Format::Double),
        ("pause", Format::Flag),
        ("paused-for-cache", Format::Flag),
        ("seeking", Format::Flag),
        ("volume", Format::Double),
        ("mute", Format::Flag),
        ("aid", Format::Str),
        ("sid", Format::Str),
        ("track-list", Format::None),
    ] {
        mpv.observe(name, format);
    }
    Ok(mpv)
}

impl Player {
    fn new(ctx: &Rc<Ctx>) -> Result<Rc<Player>, String> {
        let volume = session::settings().volume;
        let mut mpv = start_mpv(volume)?;
        let area = gtk::GLArea::builder().hexpand(true).vexpand(true).build();
        let Controls {
            overlay,
            top,
            bottom,
            title,
            subtitle,
            back,
            play_button,
            back10,
            forward30,
            previous_button,
            next_button,
            seek,
            seek_time,
            position_label,
            duration_label,
            volume_button,
            volume_scale,
            mute_button,
            audio_button,
            audio_list,
            subs_button,
            subs_list,
            fullscreen_button,
            ends,
            skip_button,
            up_next,
            spinner,
            error,
            error_back,
        } = Controls::new(&area);

        let page = adw::NavigationPage::builder()
            .title("Player")
            .tag("player")
            .child(&overlay)
            .build();
        let player = Rc::new(Player {
            ctx: Rc::downgrade(ctx),
            page,
            overlay,
            area,
            mpv: RefCell::new(None),
            renderer: RefCell::new(None),
            top,
            bottom,
            title,
            subtitle,
            play_button,
            seek,
            seek_time,
            position_label,
            duration_label,
            volume_button,
            volume_scale,
            audio_button,
            audio_list,
            subs_button,
            subs_list,
            previous_button,
            next_button,
            fullscreen_button,
            ends,
            skip_button,
            up_next,
            spinner,
            error,
            fullscreen_handler: RefCell::new(None),
            updating: Cell::new(false),
            state: RefCell::new(State::new(volume)),
        });

        // mpv events: its wakeup callback runs on an mpv thread, so hop to the main loop
        let (wake_tx, wake_rx) = async_channel::bounded::<()>(1);
        mpv.set_wakeup(Some(Box::new(move || {
            let _ = wake_tx.try_send(());
        })));
        *player.mpv.borrow_mut() = Some(mpv);
        let weak = Rc::downgrade(&player);
        glib::spawn_future_local(async move {
            while wake_rx.recv().await.is_ok() {
                let Some(p) = weak.upgrade() else { break };
                p.drain_events();
            }
        });

        player.connect_video();
        player.connect_controls(&back, &back10, &forward30, &mute_button, &error_back);
        player.connect_input();

        let weak = Rc::downgrade(&player);
        player.page.connect_hidden(move |_| {
            // Popped (back button, Esc, swipe, or the last episode ended)
            if let Some(p) = weak.upgrade() {
                CURRENT.with(|c| {
                    let mut current = c.borrow_mut();
                    if current.as_ref().is_some_and(|cur| Rc::ptr_eq(cur, &p)) {
                        current.take();
                    }
                });
                p.shutdown(false);
            }
        });

        let weak = Rc::downgrade(&player);
        let handler = ctx.window.connect_fullscreened_notify(move |window| {
            if let Some(p) = weak.upgrade() {
                let icon = if window.is_fullscreen() {
                    "view-restore-symbolic"
                } else {
                    "view-fullscreen-symbolic"
                };
                p.fullscreen_button.set_icon_name(icon);
            }
        });
        *player.fullscreen_handler.borrow_mut() = Some(handler);

        let weak = Rc::downgrade(&player);
        glib::timeout_add_local(Duration::from_millis(250), move || match weak.upgrade() {
            Some(p) if !p.state.borrow().closed => {
                p.tick();
                glib::ControlFlow::Continue
            }
            _ => glib::ControlFlow::Break,
        });

        Ok(player)
    }

    fn connect_video(self: &Rc<Self>) {
        let (frame_tx, frame_rx) = async_channel::bounded::<()>(1);
        let weak = Rc::downgrade(self);
        self.area.connect_realize(move |area| {
            let Some(p) = weak.upgrade() else { return };
            area.make_current();
            if let Some(e) = area.error() {
                p.show_error(&format!("OpenGL isn't available: {e}"));
                return;
            }
            let renderer = p
                .mpv
                .borrow()
                .as_ref()
                .map(|mpv| Renderer::new(mpv, native_display(area)));
            match renderer {
                Some(Ok(mut renderer)) => {
                    let tx = frame_tx.clone();
                    renderer.set_update_callback(Box::new(move || {
                        let _ = tx.try_send(());
                    }));
                    *p.renderer.borrow_mut() = Some(renderer);
                    let pending = p.state.borrow_mut().pending_file.take();
                    if let Some(file) = pending {
                        p.mpv_command(&["loadfile", &file, "replace"]);
                    }
                }
                Some(Err(e)) => p.show_error(&e),
                None => {}
            }
        });
        let weak = Rc::downgrade(self);
        self.area.connect_unrealize(move |area| {
            if let Some(p) = weak.upgrade() {
                area.make_current();
                p.renderer.borrow_mut().take();
            }
        });
        let weak = Rc::downgrade(self);
        self.area.connect_render(move |_, _| {
            if let Some(p) = weak.upgrade()
                && let Some(renderer) = p.renderer.borrow().as_ref()
            {
                let (fbo, w, h) = mpv::current_framebuffer();
                renderer.render(fbo, w, h);
            }
            glib::Propagation::Stop
        });
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while frame_rx.recv().await.is_ok() {
                let Some(p) = weak.upgrade() else { break };
                let new_frame = p.renderer.borrow().as_ref().is_some_and(|r| r.update());
                if new_frame {
                    p.area.queue_render();
                }
            }
        });
    }

    fn window(&self) -> Option<gtk::Window> {
        self.ctx.upgrade().map(|c| c.window.clone().upcast())
    }

    fn mpv_set(&self, name: &str, value: &str) {
        if let Some(mpv) = self.mpv.borrow().as_ref()
            && let Err(e) = mpv.set(name, value)
        {
            eprintln!("mpv: {e}");
        }
    }

    fn mpv_command(&self, args: &[&str]) {
        if let Some(mpv) = self.mpv.borrow().as_ref()
            && let Err(e) = mpv.command(args)
        {
            eprintln!("mpv: {e}");
        }
    }

    fn open_file(&self, file: &str) {
        if self.renderer.borrow().is_some() {
            self.mpv_command(&["loadfile", file, "replace"]);
        } else {
            self.state.borrow_mut().pending_file = Some(file.to_string());
        }
    }

    fn osd(&self, text: &str) {
        self.mpv_command(&["show-text", text, "1500"]);
    }

    // ---- loading ----

    fn load(self: &Rc<Self>, media: Media) {
        self.report_stopped(false);
        let load_id = {
            let mut st = self.state.borrow_mut();
            st.load_id += 1;
            st.playing = None;
            st.segments.clear();
            st.chapters.clear();
            st.previous = None;
            st.next = None;
            st.next_declined = false;
            st.skip = None;
            st.tracks.clear();
            st.position = 0.0;
            st.duration = 0.0;
            st.loading = true;
            st.load_id
        };
        self.error.set_visible(false);
        self.skip_button.set_visible(false);
        self.up_next.root.set_visible(false);
        self.previous_button.set_visible(false);
        self.next_button.set_visible(false);
        self.seek.clear_marks();
        self.update_spinner();
        self.update_time();
        self.update_ends();
        self.show_controls();

        match media {
            Media::File(path) => {
                let name = std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or(path.clone());
                self.set_titles(&name, "");
                self.mpv_set("start", "none");
                self.open_file(&path);
            }
            Media::Jellyfin { item, start } => {
                let item = *item;
                let (title, subtitle) = titles_for(&item);
                self.set_titles(&title, &subtitle);
                let Some(ctx) = self.ctx.upgrade() else {
                    return;
                };
                let weak = Rc::downgrade(self);
                let id = item.id.clone();
                let episode = (item.kind == "Episode").then(|| item.clone());
                super::run_result(
                    &ctx,
                    move |client| {
                        let info = client.playback_info(&id)?;
                        // Optional extras: skip buttons and the episodes either side
                        let segments = client.segments(&id).unwrap_or_default();
                        let adjacent = episode
                            .and_then(|e| client.adjacent_episodes(&e).ok())
                            .unwrap_or_default();
                        Ok((info, segments, adjacent))
                    },
                    move |result| {
                        let Some(p) = weak.upgrade() else { return };
                        if p.state.borrow().load_id != load_id {
                            return; // something else was started meanwhile
                        }
                        match result {
                            Ok((info, segments, adjacent)) => {
                                p.start(item, start, info, segments, adjacent)
                            }
                            Err(e) => p.show_error(&e.to_string()),
                        }
                    },
                );
            }
        }
    }

    fn start(
        self: &Rc<Self>,
        item: api::Item,
        start: f64,
        info: api::PlaybackInfo,
        segments: Vec<api::Segment>,
        (previous, next): (Option<api::Item>, Option<api::Item>),
    ) {
        let Some(client) = self.ctx.upgrade().and_then(|c| c.client()) else {
            return;
        };
        let Some(source) = info.media_sources.into_iter().next() else {
            self.show_error(&format!(
                "The server has no playable file for this{}",
                info.error_code
                    .map(|c| format!(" ({c})"))
                    .unwrap_or_default()
            ));
            return;
        };
        let url = client.stream_url(&item.id, &source, info.play_session_id.as_deref());
        let external_subs: Vec<(String, i32)> = source
            .media_streams
            .iter()
            .filter(|s| s.kind == "Subtitle" && s.is_external)
            .map(|s| (client.subtitle_url(&item.id, &source, s), s.index))
            .collect();

        self.mpv_set("http-header-fields", &client.mpv_header());
        self.mpv_command(&["change-list", "sub-files", "clr", ""]);
        for (sub_url, _) in &external_subs {
            self.mpv_command(&["change-list", "sub-files", "append", sub_url]);
        }
        self.mpv_set(
            "start",
            &if start > 1.0 {
                format!("{start:.3}")
            } else {
                "none".to_string()
            },
        );
        self.mpv_set("pause", "no");
        self.previous_button.set_visible(previous.is_some());
        self.next_button.set_visible(next.is_some());
        if let Some(ctx) = self.ctx.upgrade() {
            ctx.window.set_title(Some(&item.name));
            if let Some(next) = &next {
                self.up_next.set_item(&ctx, next);
            }
        }
        {
            let mut st = self.state.borrow_mut();
            st.playing = Some(Playing {
                item,
                source,
                play_session_id: info.play_session_id,
                external_subs,
                started: false,
            });
            st.segments = segments;
            st.previous = previous;
            st.next = next;
        }
        self.open_file(&url);
    }

    fn set_titles(&self, title: &str, subtitle: &str) {
        self.title.set_label(title);
        self.subtitle.set_label(subtitle);
        self.subtitle.set_visible(!subtitle.is_empty());
    }

    fn show_error(&self, message: &str) {
        self.state.borrow_mut().loading = false;
        self.update_spinner();
        self.error
            .set_description(Some(&glib::markup_escape_text(message)));
        self.error.set_visible(true);
        self.show_controls();
    }

    // ---- mpv events ----

    fn drain_events(self: &Rc<Self>) {
        loop {
            let event = match self.mpv.borrow().as_ref() {
                Some(mpv) => mpv.next_event(),
                None => return,
            };
            let Some(event) = event else { return };
            match event {
                Event::StartFile => {
                    self.state.borrow_mut().loading = true;
                    self.update_spinner();
                }
                Event::FileLoaded => self.file_loaded(),
                Event::PlaybackRestart => {
                    self.reselect_image_subs();
                    self.state.borrow_mut().loading = false;
                    self.update_spinner();
                }
                Event::EndFile { reason, error } => self.file_ended(reason, error),
                Event::Property { name, value } => self.property(&name, value),
                Event::Shutdown => {}
            }
        }
    }

    fn file_loaded(self: &Rc<Self>) {
        self.refresh_tracks();
        self.read_chapters();
        // Jellyfin's choice of tracks follows the user's language settings,
        // unless others were picked in the show
        let picked = self.show_picks();
        let defaults = self
            .state
            .borrow()
            .playing
            .as_ref()
            .map(|p| picks::streams(&p.source, picked.as_ref()));
        if let Some((audio, subtitle)) = defaults {
            let find = |kind: &str, index: Option<i32>| {
                let st = self.state.borrow();
                index.and_then(|i| {
                    st.tracks
                        .iter()
                        .find(|t| t.kind == kind && t.jellyfin_index == Some(i))
                        .map(|t| t.id)
                })
            };
            if let Some(id) = find("audio", audio) {
                self.mpv_set("aid", &id.to_string());
            }
            match find("sub", subtitle) {
                Some(id) => self.mpv_set("sid", &id.to_string()),
                None => self.mpv_set("sid", "no"),
            }
        }
        let first_start = {
            let mut st = self.state.borrow_mut();
            st.last_report = Instant::now();
            match st.playing.as_mut() {
                Some(p) if !p.started => {
                    p.started = true;
                    true
                }
                _ => false,
            }
        };
        if first_start {
            self.report(api::Report::Start, None, false);
        }
    }

    fn file_ended(self: &Rc<Self>, reason: EndReason, error: Option<String>) {
        match reason {
            EndReason::Eof => {
                {
                    let mut st = self.state.borrow_mut();
                    st.position = st.duration;
                }
                let next = self.state.borrow().next.is_some() && !self.state.borrow().next_declined;
                if next {
                    self.play_next();
                } else {
                    self.close();
                }
            }
            EndReason::Error => {
                let detail = error.unwrap_or_else(|| "unknown error".into());
                self.show_error(&format!("mpv couldn't open the file ({detail})"));
            }
            _ => {}
        }
    }

    fn property(self: &Rc<Self>, name: &str, value: Value) {
        match (name, value) {
            ("time-pos", Value::Double(t)) => {
                let report_due = {
                    let mut st = self.state.borrow_mut();
                    st.position = t;
                    !st.paused && st.playing.is_some() && st.last_report.elapsed() >= REPORT_EVERY
                };
                self.update_time();
                self.update_offers();
                if report_due {
                    self.report(api::Report::Progress, Some("TimeUpdate"), false);
                }
            }
            ("duration", Value::Double(d)) => {
                self.state.borrow_mut().duration = d;
                self.updating.set(true);
                self.seek.set_range(0.0, d.max(1.0));
                self.updating.set(false);
                self.mark_chapters();
                self.update_time();
            }
            ("pause", Value::Flag(paused)) => {
                let changed = {
                    let mut st = self.state.borrow_mut();
                    let changed = st.paused != paused;
                    st.paused = paused;
                    changed
                };
                let (icon, tip) = if paused {
                    ("media-playback-start-symbolic", "Play (Space)")
                } else {
                    ("media-playback-pause-symbolic", "Pause (Space)")
                };
                self.play_button.set_icon_name(icon);
                self.play_button.set_tooltip_text(Some(tip));
                self.set_inhibit(!paused);
                if changed {
                    self.report(
                        api::Report::Progress,
                        Some(if paused { "Pause" } else { "Unpause" }),
                        false,
                    );
                }
                if paused {
                    self.show_controls();
                }
            }
            ("paused-for-cache", Value::Flag(waiting)) => {
                self.state.borrow_mut().waiting_for_cache = waiting;
                self.update_spinner();
            }
            ("seeking", Value::Flag(seeking)) => {
                self.state.borrow_mut().seeking = seeking;
                self.update_spinner();
            }
            ("volume", Value::Double(v)) => {
                self.state.borrow_mut().volume = v;
                self.updating.set(true);
                self.volume_scale.set_value(v);
                self.updating.set(false);
                self.update_volume_icon();
            }
            ("mute", Value::Flag(muted)) => {
                self.state.borrow_mut().muted = muted;
                self.update_volume_icon();
            }
            ("aid" | "sid" | "track-list", _) => self.refresh_tracks(),
            _ => {}
        }
    }

    fn shutdown(&self, blocking: bool) {
        if std::mem::replace(&mut self.state.borrow_mut().closed, true) {
            return;
        }
        self.report_stopped(blocking);
        let mut settings = session::settings();
        settings.volume = self.state.borrow().volume;
        session::save_settings(&settings);
        self.set_inhibit(false);
        self.mpv_command(&["stop"]);
        // The render context has to go first, with its OpenGL context current
        if let Some(renderer) = self.renderer.borrow_mut().take() {
            self.area.make_current();
            drop(renderer);
        }
        self.mpv.borrow_mut().take();
        if let Some(ctx) = self.ctx.upgrade() {
            if let Some(handler) = self.fullscreen_handler.borrow_mut().take() {
                ctx.window.disconnect(handler);
            }
            if ctx.window.is_fullscreen() {
                ctx.window.unfullscreen();
            }
            ctx.window.set_title(Some(api::CLIENT_NAME));
        }
    }
}

fn titles_for(item: &api::Item) -> (String, String) {
    match item.kind.as_str() {
        "Episode" => {
            let code = item
                .episode_code()
                .map(|c| format!("{c} · "))
                .unwrap_or_default();
            (
                item.series_name.clone().unwrap_or_default(),
                format!("{code}{}", item.name),
            )
        }
        _ => (
            item.name.clone(),
            item.production_year
                .map(|y| y.to_string())
                .unwrap_or_default(),
        ),
    }
}
