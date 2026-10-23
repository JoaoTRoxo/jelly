//! The controls over the video: building them, what their buttons, keys and
//! pointer do, and keeping them up to date.

use super::tracks::scrolling;
use super::{HIDE_CONTROLS_AFTER, Media, Player};
use crate::api;
use crate::ui::cards::{self, Shape};
use crate::ui::{Ctx, format_time};
use adw::prelude::*;
use gtk::{gdk, glib};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The widgets over the video, and the buttons only their signals need
pub struct Controls {
    pub overlay: gtk::Overlay,
    pub top: gtk::Revealer,
    pub bottom: gtk::Revealer,
    pub title: gtk::Label,
    pub subtitle: gtk::Label,
    pub back: gtk::Button,
    pub play_button: gtk::Button,
    pub back10: gtk::Button,
    pub forward30: gtk::Button,
    pub next_button: gtk::Button,
    pub seek: gtk::Scale,
    pub position_label: gtk::Label,
    pub duration_label: gtk::Label,
    pub volume_button: gtk::MenuButton,
    pub volume_scale: gtk::Scale,
    pub mute_button: gtk::Button,
    pub audio_button: gtk::MenuButton,
    pub audio_list: gtk::Box,
    pub subs_button: gtk::MenuButton,
    pub subs_list: gtk::Box,
    pub fullscreen_button: gtk::Button,
    pub ends: gtk::Label,
    pub skip_button: gtk::Button,
    pub up_next: UpNextCard,
    pub spinner: adw::Spinner,
    pub error: adw::StatusPage,
    pub error_back: gtk::Button,
}

impl Controls {
    /// Built over `area`, the video
    pub fn new(area: &gtk::GLArea) -> Controls {
        let overlay = gtk::Overlay::builder()
            .child(area)
            .css_classes(["player"])
            .build();

        // Top: back and title
        let back = icon_button("go-previous-symbolic", "Back (Esc)");
        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["title-3"])
            .build();
        let subtitle = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label"])
            .build();
        let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
        titles.append(&title);
        titles.append(&subtitle);
        let top_box = gtk::Box::builder()
            .spacing(12)
            .css_classes(["player-top"])
            .build();
        top_box.append(&back);
        top_box.append(&titles);
        let top = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .reveal_child(true)
            .valign(gtk::Align::Start)
            .child(&top_box)
            .build();

        // Bottom: seek bar and buttons
        let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
        seek.set_hexpand(true);
        seek.set_draw_value(false);
        let position_label = gtk::Label::builder()
            .label("0:00")
            .css_classes(["numeric"])
            .build();
        let duration_label = gtk::Label::builder()
            .label("0:00")
            .css_classes(["numeric"])
            .build();
        let seek_row = gtk::Box::builder().spacing(12).build();
        seek_row.append(&position_label);
        seek_row.append(&seek);
        seek_row.append(&duration_label);

        let play_button = icon_button("media-playback-pause-symbolic", "Pause (Space)");
        play_button.add_css_class("play-button");
        let back10 = icon_button("media-seek-backward-symbolic", "Back 10 s (J)");
        let forward30 = icon_button("media-seek-forward-symbolic", "Forward 30 s");
        let next_button = icon_button("media-skip-forward-symbolic", "Next episode (N)");
        next_button.set_visible(false);

        let volume_scale = gtk::Scale::with_range(gtk::Orientation::Vertical, 0.0, 130.0, 1.0);
        volume_scale.set_inverted(true);
        volume_scale.set_draw_value(false);
        volume_scale.set_height_request(170);
        volume_scale.add_mark(100.0, gtk::PositionType::Right, None);
        let mute_button = gtk::Button::builder()
            .icon_name("audio-volume-muted-symbolic")
            .tooltip_text("Mute (M)")
            .css_classes(["flat", "circular"])
            .build();
        let volume_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        volume_box.append(&volume_scale);
        volume_box.append(&mute_button);
        let volume_button = menu_button("audio-volume-high-symbolic", "Volume (↑ ↓)", &volume_box);

        let audio_list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        let audio_button = menu_button(
            "audio-x-generic-symbolic",
            "Audio (A)",
            &scrolling(&audio_list),
        );
        let subs_list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        let subs_button = menu_button(
            "media-view-subtitles-symbolic",
            "Subtitles (S)",
            &scrolling(&subs_list),
        );
        let fullscreen_button = icon_button("view-fullscreen-symbolic", "Fullscreen (F)");
        let ends = gtk::Label::builder()
            .css_classes(["dim-label", "numeric"])
            .margin_end(8)
            .visible(false)
            .build();

        let buttons = gtk::Box::builder().spacing(4).build();
        for b in [&play_button, &back10, &forward30, &next_button] {
            buttons.append(b);
        }
        let spacer = gtk::Box::builder().hexpand(true).build();
        buttons.append(&spacer);
        buttons.append(&ends);
        buttons.append(&volume_button);
        buttons.append(&audio_button);
        buttons.append(&subs_button);
        buttons.append(&fullscreen_button);

        let bottom_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .css_classes(["player-bottom"])
            .build();
        bottom_box.append(&seek_row);
        bottom_box.append(&buttons);
        let bottom = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .reveal_child(true)
            .valign(gtk::Align::End)
            .child(&bottom_box)
            .build();

        let skip_button = gtk::Button::builder()
            .css_classes(["pill", "skip-button"])
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .margin_end(32)
            .margin_bottom(130)
            .visible(false)
            .build();
        let up_next = UpNextCard::new();
        let spinner = adw::Spinner::builder()
            .width_request(64)
            .height_request(64)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .can_target(false)
            .build();
        let error_back = gtk::Button::builder()
            .label("Back")
            .halign(gtk::Align::Center)
            .css_classes(["pill"])
            .build();
        let error = adw::StatusPage::builder()
            .icon_name("dialog-warning-symbolic")
            .title("Couldn't play this")
            .child(&error_back)
            .css_classes(["player-error"])
            .visible(false)
            .build();

        for child in [
            top.upcast_ref::<gtk::Widget>(),
            bottom.upcast_ref(),
            skip_button.upcast_ref(),
            up_next.root.upcast_ref(),
            spinner.upcast_ref(),
            error.upcast_ref(),
        ] {
            overlay.add_overlay(child);
        }
        Controls {
            overlay,
            top,
            bottom,
            title,
            subtitle,
            back,
            play_button,
            back10,
            forward30,
            next_button,
            seek,
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
        }
    }
}

/// The next episode's card, counting down over the credits
pub struct UpNextCard {
    pub root: gtk::Box,
    art: gtk::Box,
    caption: gtk::Label,
    title: gtk::Label,
    pub countdown: gtk::Label,
    pub progress: gtk::ProgressBar,
    pub play: gtk::Button,
    pub dismiss: gtk::Button,
}

impl UpNextCard {
    fn new() -> UpNextCard {
        let art = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let caption = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["caption-heading", "dim-label"])
            .build();
        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(26)
            .css_classes(["heading"])
            .build();
        let countdown = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["caption", "dim-label", "numeric"])
            .build();
        let play = gtk::Button::builder()
            .label("Play Now")
            .css_classes(["pill", "suggested-action"])
            .build();
        let dismiss = gtk::Button::builder()
            .label("Cancel")
            .css_classes(["pill", "flat"])
            .build();
        let buttons = gtk::Box::builder().spacing(6).margin_top(6).build();
        buttons.append(&play);
        buttons.append(&dismiss);
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(3)
            .valign(gtk::Align::Center)
            .build();
        for widget in [
            caption.upcast_ref::<gtk::Widget>(),
            title.upcast_ref(),
            countdown.upcast_ref(),
            buttons.upcast_ref(),
        ] {
            text.append(widget);
        }
        let row = gtk::Box::builder().spacing(14).build();
        row.append(&art);
        row.append(&text);
        let progress = gtk::ProgressBar::builder()
            .css_classes(["up-next-progress"])
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .css_classes(["up-next"])
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .margin_end(32)
            .margin_bottom(130)
            .visible(false)
            .build();
        root.append(&row);
        root.append(&progress);
        UpNextCard {
            root,
            art,
            caption,
            title,
            countdown,
            progress,
            play,
            dismiss,
        }
    }

    pub fn set_item(&self, ctx: &Rc<Ctx>, item: &api::Item) {
        crate::ui::clear(&self.art);
        self.art
            .append(&cards::art(ctx, item, Shape::Thumb, 192, 108));
        let code = item
            .episode_code()
            .map(|c| format!(" · {c}"))
            .unwrap_or_default();
        self.caption.set_label(&format!("UP NEXT{code}"));
        self.title.set_label(&item.name);
    }
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .build()
}

fn menu_button(icon: &str, tooltip: &str, content: &impl IsA<gtk::Widget>) -> gtk::MenuButton {
    let popover = gtk::Popover::builder().child(content).build();
    gtk::MenuButton::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .popover(&popover)
        .css_classes(["flat", "circular"])
        .valign(gtk::Align::Center)
        .direction(gtk::ArrowType::Up)
        .build()
}

impl Player {
    pub(super) fn connect_controls(
        self: &Rc<Self>,
        back: &gtk::Button,
        back10: &gtk::Button,
        forward30: &gtk::Button,
        mute: &gtk::Button,
        error_back: &gtk::Button,
    ) {
        let on = |button: &gtk::Button, f: fn(&Rc<Player>)| {
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(p) = weak.upgrade() {
                    f(&p);
                }
            });
        };
        on(back, |p| p.close());
        on(error_back, |p| p.close());
        on(&self.play_button, |p| p.toggle_pause());
        on(back10, |p| p.seek_by(-10.0));
        on(forward30, |p| p.seek_by(30.0));
        on(&self.next_button, |p| p.play_next());
        on(&self.fullscreen_button, |p| p.toggle_fullscreen());
        on(mute, |p| p.toggle_mute());
        on(&self.skip_button, |p| p.skip());
        on(&self.up_next.play, |p| p.play_next());
        on(&self.up_next.dismiss, |p| {
            p.state.borrow_mut().next_declined = true;
            p.update_offers();
        });

        let weak = Rc::downgrade(self);
        self.seek.connect_change_value(move |_, _, value| {
            if let Some(p) = weak.upgrade() {
                p.seek_to(value);
            }
            glib::Propagation::Proceed
        });
        // Hovering the seek bar shows the time there, and the chapter
        self.seek.set_has_tooltip(true);
        let weak = Rc::downgrade(self);
        self.seek
            .connect_query_tooltip(move |seek, x, _, keyboard, tooltip| {
                let Some(p) = weak.upgrade() else {
                    return false;
                };
                let trough = seek.range_rect();
                let st = p.state.borrow();
                if keyboard || st.duration <= 0.0 || trough.width() <= 0 {
                    return false;
                }
                let time =
                    ((x - trough.x()) as f64 / trough.width() as f64).clamp(0.0, 1.0) * st.duration;
                let chapter = st
                    .chapters
                    .iter()
                    .rev()
                    .find(|(start, _)| *start <= time)
                    .map(|(_, title)| title.as_str());
                match chapter.filter(|c| !c.is_empty()) {
                    Some(chapter) => {
                        tooltip.set_text(Some(&format!("{} · {chapter}", format_time(time))))
                    }
                    None => tooltip.set_text(Some(&format_time(time))),
                }
                true
            });
        let weak = Rc::downgrade(self);
        self.volume_scale.connect_value_changed(move |scale| {
            if let Some(p) = weak.upgrade()
                && !p.updating.get()
            {
                p.mpv_set("volume", &format!("{:.0}", scale.value()));
            }
        });
    }

    pub(super) fn connect_input(self: &Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(p) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if modifiers.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
                return glib::Propagation::Proceed;
            }
            use gdk::Key;
            match key {
                Key::space | Key::k | Key::K => p.toggle_pause(),
                Key::Left => p.seek_by(-5.0),
                Key::Right => p.seek_by(5.0),
                Key::j | Key::J => p.seek_by(-10.0),
                Key::l | Key::L => p.seek_by(10.0),
                Key::Up => p.change_volume(5.0),
                Key::Down => p.change_volume(-5.0),
                Key::f | Key::F | Key::F11 => p.toggle_fullscreen(),
                Key::Escape => {
                    if p.window().is_some_and(|w| w.is_fullscreen()) {
                        p.toggle_fullscreen();
                    } else {
                        p.close();
                    }
                }
                Key::m | Key::M => p.toggle_mute(),
                Key::s | Key::S => p.cycle_track("sid"),
                Key::a | Key::A => p.cycle_track("aid"),
                Key::n | Key::N => p.play_next(),
                _ => return glib::Propagation::Proceed,
            }
            p.show_controls();
            glib::Propagation::Stop
        });
        self.page.add_controller(keys);

        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(self);
        motion.connect_motion(move |_, x, y| {
            if let Some(p) = weak.upgrade() {
                let moved = {
                    let mut st = p.state.borrow_mut();
                    let moved = (st.pointer.0 - x).abs() > 1.0 || (st.pointer.1 - y).abs() > 1.0;
                    st.pointer = (x, y);
                    moved
                };
                if moved {
                    p.show_controls();
                }
            }
        });
        self.overlay.add_controller(motion);

        // Keep the controls up while the pointer is on them
        for revealer in [&self.top, &self.bottom] {
            let hover = gtk::EventControllerMotion::new();
            let weak = Rc::downgrade(self);
            hover.connect_enter(move |_, _, _| {
                if let Some(p) = weak.upgrade() {
                    p.state.borrow_mut().over_controls = true;
                }
            });
            let weak = Rc::downgrade(self);
            hover.connect_leave(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.state.borrow_mut().over_controls = false;
                }
            });
            revealer.add_controller(hover);
        }

        // Click to pause, double-click for fullscreen (undoing the first click's pause)
        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        let weak = Rc::downgrade(self);
        click.connect_pressed(move |_, n_press, _, _| {
            if let Some(p) = weak.upgrade() {
                p.toggle_pause();
                if n_press == 2 {
                    p.toggle_fullscreen();
                }
                p.show_controls();
            }
        });
        self.area.add_controller(click);
    }

    // ---- controls ----

    pub(super) fn toggle_pause(&self) {
        if self.state.borrow().playing.is_some()
            || self
                .mpv
                .borrow()
                .as_ref()
                .is_some_and(|m| m.get("path").is_some())
        {
            self.mpv_command(&["cycle", "pause"]);
        }
    }

    pub(super) fn seek_to(&self, seconds: f64) {
        {
            let mut st = self.state.borrow_mut();
            st.seek_hold_until = Instant::now() + Duration::from_millis(400);
            st.position = seconds;
        }
        self.mpv_command(&["seek", &format!("{seconds:.3}"), "absolute"]);
        self.update_time();
    }

    pub(super) fn seek_by(&self, delta: f64) {
        self.mpv_command(&["seek", &format!("{delta}"), "relative"]);
    }

    pub(super) fn change_volume(&self, delta: f64) {
        let volume = (self.state.borrow().volume + delta).clamp(0.0, 130.0);
        self.mpv_set("volume", &format!("{volume:.0}"));
        self.osd(&format!("Volume: {volume:.0}%"));
    }

    pub(super) fn toggle_mute(&self) {
        self.mpv_command(&["cycle", "mute"]);
        let muted = !self.state.borrow().muted;
        self.osd(if muted { "Muted" } else { "Unmuted" });
    }

    pub(super) fn toggle_fullscreen(&self) {
        if let Some(window) = self.window() {
            window.set_fullscreened(!window.is_fullscreen());
        }
    }

    pub(super) fn play_next(self: &Rc<Self>) {
        let next = self.state.borrow().next.clone();
        if let Some(item) = next {
            let start = item.resume_seconds();
            self.load(Media::Jellyfin {
                item: Box::new(item),
                start,
            });
        }
    }

    pub(super) fn skip(&self) {
        let to = self.state.borrow().skip.as_ref().map(|(_, to)| *to);
        if let Some(to) = to {
            self.seek_to(to);
        }
    }

    pub(super) fn close(&self) {
        if let Some(ctx) = self.ctx.upgrade() {
            let visible = ctx.nav.visible_page().and_then(|p| p.tag());
            // With nothing underneath (`--play FILE`), closing the player closes the app
            if visible.as_deref() == Some("player") && !ctx.nav.pop() {
                ctx.window.close();
            }
        }
    }

    // ---- UI state ----

    pub(super) fn show_controls(&self) {
        {
            let mut st = self.state.borrow_mut();
            st.last_motion = Instant::now();
            if st.controls_shown {
                return;
            }
            st.controls_shown = true;
        }
        self.top.set_reveal_child(true);
        self.bottom.set_reveal_child(true);
        self.overlay.set_cursor(None);
    }

    pub(super) fn tick(&self) {
        self.update_ends();
        let hide = {
            let st = self.state.borrow();
            st.controls_shown
                && !st.paused
                && !st.loading
                && !st.over_controls
                && st.last_motion.elapsed() >= HIDE_CONTROLS_AFTER
                && !self.error.is_visible()
                && ![&self.volume_button, &self.audio_button, &self.subs_button]
                    .iter()
                    .any(|b| b.is_active())
        };
        if hide {
            self.state.borrow_mut().controls_shown = false;
            self.top.set_reveal_child(false);
            self.bottom.set_reveal_child(false);
            self.overlay.set_cursor_from_name(Some("none"));
        }
    }

    pub(super) fn update_time(&self) {
        let (position, duration, hold) = {
            let st = self.state.borrow();
            (
                st.position,
                st.duration,
                Instant::now() < st.seek_hold_until,
            )
        };
        self.position_label.set_label(&format_time(position));
        self.duration_label.set_label(&format_time(duration));
        if !hold {
            self.updating.set(true);
            self.seek.set_value(position);
            self.updating.set(false);
        }
    }

    /// When it will end, at this rate: it moves on while paused
    pub(super) fn update_ends(&self) {
        let (position, duration) = {
            let st = self.state.borrow();
            (st.position, st.duration)
        };
        let ends = (duration > 0.0)
            .then(|| {
                glib::DateTime::now_local()
                    .and_then(|now| now.add_seconds(duration - position))
                    .and_then(|end| end.format("%H:%M"))
                    .ok()
            })
            .flatten();
        match ends {
            Some(time) => {
                self.ends.set_label(&format!("Ends at {time}"));
                self.ends.set_visible(true);
            }
            None => self.ends.set_visible(false),
        }
    }

    pub(super) fn update_spinner(&self) {
        let st = self.state.borrow();
        self.spinner.set_visible(
            (st.loading || st.waiting_for_cache || st.seeking) && !self.error.is_visible(),
        );
    }

    pub(super) fn update_volume_icon(&self) {
        let st = self.state.borrow();
        let icon = match st.volume {
            _ if st.muted => "audio-volume-muted-symbolic",
            v if v <= 0.0 => "audio-volume-muted-symbolic",
            v if v < 40.0 => "audio-volume-low-symbolic",
            v if v < 80.0 => "audio-volume-medium-symbolic",
            _ => "audio-volume-high-symbolic",
        };
        self.volume_button.set_icon_name(icon);
    }
}
