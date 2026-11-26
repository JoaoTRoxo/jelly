//! The window, navigation between pages, and helpers shared by the pages.

pub mod cards;
pub mod home;
pub mod item;
pub mod library;
pub mod login;
pub mod news;
pub mod player;
pub mod request;

use crate::{api, session};
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct Ctx {
    pub app: adw::Application,
    pub window: adw::ApplicationWindow,
    pub nav: adw::NavigationView,
    pub toasts: adw::ToastOverlay,
    client: RefCell<Option<api::Client>>,
    pub device_id: String,
    /// Bumped when watch state changes, so pages reload when shown again
    pub generation: Cell<u64>,
}

impl Ctx {
    pub fn new(app: &adw::Application) -> Rc<Ctx> {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(api::CLIENT_NAME)
            .default_width(1400)
            .default_height(860)
            .build();
        let nav = adw::NavigationView::new();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&nav));
        window.set_content(Some(&toasts));
        let ctx = Rc::new(Ctx {
            app: app.clone(),
            window,
            nav,
            toasts,
            client: RefCell::new(None),
            device_id: session::device_id(),
            generation: Cell::new(0),
        });
        ctx.add_shortcut("search", &["<Control>f"], |ctx| {
            ctx.nav.push(&library::search_page(ctx))
        });
        ctx.add_shortcut("refresh", &["F5", "<Control>r"], |ctx| {
            ctx.refresh_visible()
        });
        ctx
    }

    /// A window action with keyboard shortcuts, active on the browsing pages
    fn add_shortcut(self: &Rc<Self>, name: &str, accels: &[&str], f: impl Fn(&Rc<Ctx>) + 'static) {
        let action = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            let Some(ctx) = weak.upgrade() else { return };
            let tag = ctx.nav.visible_page().and_then(|p| p.tag());
            if ctx.client().is_some()
                && !matches!(tag.as_deref(), Some("player" | "search" | "login"))
            {
                f(&ctx);
            }
        });
        self.window.add_action(&action);
        self.app
            .set_accels_for_action(&format!("win.{name}"), accels);
    }

    pub fn client(&self) -> Option<api::Client> {
        self.client.borrow().clone()
    }

    pub fn toast(&self, message: &str) {
        // Toasts take markup: "Fast & Furious" would show nothing
        self.toasts
            .add_toast(adw::Toast::new(&glib::markup_escape_text(message)));
    }

    /// Watch state changed (played something, marked watched)
    pub fn changed(&self) {
        self.generation.set(self.generation.get() + 1);
    }

    /// Reload the page on screen (pages reload when shown after a change)
    pub fn refresh_visible(&self) {
        self.changed();
        if let Some(page) = self.nav.visible_page() {
            page.emit_by_name::<()>("showing", &[]);
        }
    }

    /// Start (or restore) a signed-in session and show the home page
    pub fn sign_in(self: &Rc<Self>, s: session::Session) {
        let client = api::Client::new(&s.server, &self.device_id).signed_in(&s.token, &s.user_id);
        *self.client.borrow_mut() = Some(client);
        let mut settings = session::settings();
        settings.last_server = s.server.clone();
        session::save_settings(&settings);
        session::save(&s);
        self.nav.replace(&[home::page(self)]);
    }

    pub fn sign_out(self: &Rc<Self>, tell_server: bool) {
        if let Some(client) = self.client.borrow_mut().take()
            && tell_server
        {
            gio::spawn_blocking(move || {
                let _ = client.logout();
            });
        }
        session::clear();
        self.nav.replace(&[login::page(self)]);
    }

    /// Pages currently in the navigation stack, bottom to top
    fn nav_pages(&self) -> Vec<adw::NavigationPage> {
        let stack = self.nav.navigation_stack();
        (0..stack.n_items())
            .filter_map(|i| stack.item(i)?.downcast::<adw::NavigationPage>().ok())
            .collect()
    }

    /// Show `player` with `item`'s (or its show's) page directly underneath it
    /// when it isn't already there, so going back lands on that page instead
    /// of Home
    pub(crate) fn show_player(
        self: &Rc<Self>,
        player: &adw::NavigationPage,
        item: Option<&api::Item>,
    ) {
        let target = item.and_then(item_page_target);
        let mut pages = self.nav_pages();
        let on_top = pages.last() == Some(player);
        let below = if on_top {
            pages.len().checked_sub(2).and_then(|i| pages.get(i))
        } else {
            pages.last()
        };
        let needs_page = target
            .as_ref()
            .filter(|(t, _)| !below.is_some_and(|p| p.widget_name() == item::page_key(&t.id)));
        match (on_top, needs_page) {
            (true, None) => {}
            (false, None) => self.nav.push(player),
            (true, Some((t, season))) => {
                let pos = pages.len() - 1;
                pages.insert(pos, item::page(self, t.clone(), season.clone()));
                self.nav.replace(&pages);
            }
            (false, Some((t, season))) => {
                // Push `player` first so NavigationView unmaps the previous
                // page cleanly (calling `replace` directly from Home leaves
                // Home mapped and focusable underneath), then slip the item's
                // page in below once `player` is shown.
                let slot: Rc<Cell<Option<glib::SignalHandlerId>>> = Rc::new(Cell::new(None));
                let handler = slot.clone();
                let (ctx, target, season) = (Rc::downgrade(self), t.clone(), season.clone());
                slot.set(Some(player.connect_shown(move |player| {
                    if let Some(id) = handler.take() {
                        player.disconnect(id);
                    }
                    let Some(ctx) = ctx.upgrade() else { return };
                    let mut pages = ctx.nav_pages();
                    let below = pages.len().checked_sub(2).and_then(|i| pages.get(i));
                    if pages.last() == Some(player)
                        && !below.is_some_and(|p| p.widget_name() == item::page_key(&target.id))
                    {
                        let pos = pages.len() - 1;
                        pages.insert(pos, item::page(&ctx, target.clone(), season.clone()));
                        ctx.nav.replace(&pages);
                    }
                })));
                self.nav.push(player);
            }
        }
    }
}

/// Run `work` with the API client on a worker thread, then `done` with its
/// result on the main thread. Signs out if the session was revoked.
pub fn run_result<T: Send + 'static>(
    ctx: &Rc<Ctx>,
    work: impl FnOnce(api::Client) -> api::Result<T> + Send + 'static,
    done: impl FnOnce(api::Result<T>) + 'static,
) {
    let Some(client) = ctx.client() else { return };
    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || work(client))
            .await
            .unwrap_or_else(|_| Err(api::Error::Decode("internal error".into())));
        if let Err(api::Error::Unauthorized) = result {
            ctx.toast("Your session ended, sign in again");
            ctx.sign_out(false);
            return;
        }
        done(result);
    });
}

/// Like `run_result`, showing errors as a toast
pub fn run<T: Send + 'static>(
    ctx: &Rc<Ctx>,
    work: impl FnOnce(api::Client) -> api::Result<T> + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let toast_ctx = ctx.clone();
    run_result(ctx, work, move |result| match result {
        Ok(value) => done(value),
        Err(e) => toast_ctx.toast(&e.to_string()),
    });
}

/// The movie or series whose detail page represents `item`, and which season
/// to show first
pub(crate) fn item_page_target(item: &api::Item) -> Option<(api::Item, Option<String>)> {
    let series = |season| {
        item.series_id.as_ref().map(|id| {
            (
                api::Item {
                    id: id.clone(),
                    kind: "Series".into(),
                    name: item.series_name.clone().unwrap_or_default(),
                    ..Default::default()
                },
                season,
            )
        })
    };
    match item.kind.as_str() {
        "Series" | "Movie" | "Video" => Some((item.clone(), None)),
        "Season" => series(Some(item.id.clone())),
        "Episode" => series(item.season_id.clone()),
        _ => None,
    }
}

/// Open an item's page (or play it, for things that have no page)
pub fn open(ctx: &Rc<Ctx>, item: &api::Item) {
    if let Some((target, season)) = item_page_target(item) {
        ctx.nav.push(&item::page(ctx, target, season));
    } else if item.is_folder_like() {
        ctx.nav.push(&library::page(ctx, item.clone()));
    } else if !matches!(item.kind.as_str(), "Season" | "Episode") {
        play(ctx, item, false);
    }
}

/// Play an item, resuming where it was left unless `from_start`
pub fn play(ctx: &Rc<Ctx>, item: &api::Item, from_start: bool) {
    let start = if from_start {
        0.0
    } else {
        item.resume_seconds()
    };
    player::open(
        ctx,
        player::Media::Jellyfin {
            item: Box::new(item.clone()),
            start,
        },
    );
}

/// "1:02:03" or "12:03"
pub fn format_time(seconds: f64) -> String {
    let s = seconds.max(0.0) as i64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "1 h 42 min" or "24 min"
pub fn format_runtime(minutes: i64) -> String {
    if minutes >= 60 {
        format!("{} h {} min", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}

/// Remove every child of a box
pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// A page whose content reloads when it's shown again after watch state changed
pub fn reloading_page(ctx: &Rc<Ctx>, page: &adw::NavigationPage, load: impl Fn() + 'static) {
    let seen = Cell::new(u64::MAX);
    let ctx = Rc::downgrade(ctx);
    page.connect_showing(move |_| {
        let Some(ctx) = ctx.upgrade() else { return };
        if seen.get() != ctx.generation.get() {
            seen.set(ctx.generation.get());
            load();
        }
    });
}

/// Whether `window` is maximized (using Win32's `IsZoomed` on Windows)
pub(crate) fn is_window_maximized(window: &gtk::Window) -> bool {
    #[cfg(windows)]
    if let Some(hwnd) = window_hwnd(window) {
        return win32::is_zoomed(hwnd);
    }
    window.is_maximized()
}

/// Enter or leave fullscreen on `window`. When `restore_maximized` is true,
/// keep a maximized window maximized across the transition: GTK4's Win32
/// backend unmaximizes when entering fullscreen and restores the small rect
/// when leaving it, so on Windows we sync to DWM (`DwmFlush`), disable DWM
/// transition animations, suppress GTK4's intermediate small-rect resize and
/// `SWP_NOCOPYBITS`, and re-maximize synchronously right after
/// `set_fullscreened(false)`.
pub(crate) fn set_window_fullscreen(
    window: &gtk::Window,
    fullscreen: bool,
    restore_maximized: bool,
) {
    #[cfg(windows)]
    {
        let hwnd = window_hwnd(window);
        if let Some(hwnd) = hwnd {
            win32::dwm_flush();
        }
        if !restore_maximized {
            let _guard = hwnd.map(|h| win32::WindowPosGuard::new(h, win32::GuardMode::KeepPixels));
            window.set_fullscreened(fullscreen);
            return;
        }
        let token = next_fullscreen_token();
        if let Some(hwnd) = hwnd {
            if fullscreen {
                win32::save_normal_rect(hwnd);
            }
            win32::set_transitions_disabled(hwnd, true);
        }
        {
            let mode = if fullscreen {
                win32::GuardMode::EnterFromMaximized
            } else {
                win32::GuardMode::ExitToMaximized
            };
            let _guard = hwnd.map(|h| win32::WindowPosGuard::new(h, mode));
            window.set_fullscreened(fullscreen);
            if !fullscreen {
                window.maximize();
            }
        }
        if !fullscreen && let Some(hwnd) = hwnd {
            win32::restore_normal_rect(hwnd);
        }
        // Re-enable DWM transitions once DWM has presented the new state, and
        // re-apply maximize if a layout pass raced with it
        let weak = window.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_millis(60), move || {
            if current_fullscreen_token() != token {
                return;
            }
            if let Some(window) = weak.upgrade() {
                if !fullscreen && !window.is_fullscreen() && !is_window_maximized(&window) {
                    window.maximize();
                }
                if let Some(hwnd) = window_hwnd(&window) {
                    win32::set_transitions_disabled(hwnd, false);
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        if !fullscreen && restore_maximized {
            restore_maximized_on_unfullscreen(window);
        }
        window.set_fullscreened(fullscreen);
    }
}

#[cfg(not(windows))]
fn restore_maximized_on_unfullscreen(window: &gtk::Window) {
    let slot: Rc<Cell<Option<glib::SignalHandlerId>>> = Rc::new(Cell::new(None));
    let handler = slot.clone();
    slot.set(Some(window.connect_fullscreened_notify(move |window| {
        if window.is_fullscreen() {
            return;
        }
        if let Some(id) = handler.take() {
            window.disconnect(id);
        }
        schedule_maximize(window);
    })));
}

pub(crate) fn schedule_maximize(window: &gtk::Window) {
    let weak = window.downgrade();
    glib::idle_add_local_once(move || {
        if let Some(window) = weak.upgrade()
            && !window.is_fullscreen()
            && !is_window_maximized(&window)
        {
            window.maximize();
        }
    });
}

#[cfg(windows)]
thread_local! {
    static FULLSCREEN_TOKEN: Cell<u64> = const { Cell::new(0) };
}

#[cfg(windows)]
fn next_fullscreen_token() -> u64 {
    FULLSCREEN_TOKEN.with(|t| {
        let next = t.get().wrapping_add(1);
        t.set(next);
        next
    })
}

#[cfg(windows)]
fn current_fullscreen_token() -> u64 {
    FULLSCREEN_TOKEN.with(Cell::get)
}

#[cfg(windows)]
fn window_hwnd(window: &gtk::Window) -> Option<*mut std::ffi::c_void> {
    use gtk::glib::translate::ToGlibPtr;
    use std::ffi::c_void;
    unsafe extern "C" {
        fn gdk_win32_surface_get_handle(surface: *mut gtk::gdk::ffi::GdkSurface) -> *mut c_void;
    }
    let surface = window.surface()?;
    if !surface.type_().name().starts_with("GdkWin32") {
        return None;
    }
    let raw: *mut gtk::gdk::ffi::GdkSurface = surface.to_glib_none().0;
    let hwnd = unsafe { gdk_win32_surface_get_handle(raw) };
    (!hwnd.is_null()).then_some(hwnd)
}

#[cfg(windows)]
mod win32 {
    use std::cell::Cell;
    use std::ffi::c_void;

    const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3;
    const GWL_STYLE: i32 = -16;
    const GWLP_WNDPROC: i32 = -4;
    const WS_POPUP: u32 = 0x8000_0000;
    const WM_WINDOWPOSCHANGING: u32 = 0x0046;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOCOPYBITS: u32 = 0x0100;
    const SWP_STATECHANGED: u32 = 0x8000;
    const SW_SHOWMAXIMIZED: u32 = 3;

    #[repr(C)]
    #[allow(dead_code)]
    struct WindowPos {
        hwnd: *mut c_void,
        hwnd_insert_after: *mut c_void,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    #[allow(dead_code)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    #[allow(dead_code)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    #[allow(dead_code)]
    struct WindowPlacement {
        length: u32,
        flags: u32,
        show_cmd: u32,
        pt_min_position: Point,
        pt_max_position: Point,
        rc_normal_position: Rect,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn IsZoomed(hwnd: *mut c_void) -> i32;
        fn GetWindowLongW(hwnd: *mut c_void, index: i32) -> i32;
        fn SetWindowLongPtrW(hwnd: *mut c_void, index: i32, new_long: isize) -> isize;
        fn CallWindowProcW(
            prev: isize,
            hwnd: *mut c_void,
            msg: u32,
            wparam: usize,
            lparam: isize,
        ) -> isize;
        fn GetWindowPlacement(hwnd: *mut c_void, placement: *mut WindowPlacement) -> i32;
        fn SetWindowPlacement(hwnd: *mut c_void, placement: *const WindowPlacement) -> i32;
    }

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmFlush() -> i32;
        fn DwmSetWindowAttribute(
            hwnd: *mut c_void,
            attribute: u32,
            value: *const c_void,
            size: u32,
        ) -> i32;
    }

    #[derive(Clone, Copy)]
    pub enum GuardMode {
        KeepPixels,
        EnterFromMaximized,
        ExitToMaximized,
    }

    thread_local! {
        static GUARD: Cell<Option<(*mut c_void, isize, GuardMode)>> = const { Cell::new(None) };
        static SAVED_NORMAL_RECT: Cell<Option<Rect>> = const { Cell::new(None) };
    }

    pub fn is_zoomed(hwnd: *mut c_void) -> bool {
        unsafe { IsZoomed(hwnd) != 0 }
    }

    pub fn dwm_flush() {
        unsafe {
            DwmFlush();
        }
    }

    pub fn set_transitions_disabled(hwnd: *mut c_void, disabled: bool) {
        let value = i32::from(disabled);
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                std::ptr::from_ref(&value).cast(),
                std::mem::size_of::<i32>() as u32,
            );
        }
    }

    /// Save the window's restored bounds before fullscreen so unmaximizing
    /// later still returns to the original normal size
    pub fn save_normal_rect(hwnd: *mut c_void) {
        let mut wp = WindowPlacement {
            length: std::mem::size_of::<WindowPlacement>() as u32,
            ..Default::default()
        };
        if unsafe { GetWindowPlacement(hwnd, &mut wp) } != 0 {
            SAVED_NORMAL_RECT.with(|r| r.set(Some(wp.rc_normal_position)));
        }
    }

    pub fn restore_normal_rect(hwnd: *mut c_void) {
        let Some(rect) = SAVED_NORMAL_RECT.with(Cell::take) else {
            return;
        };
        if !is_zoomed(hwnd) {
            return;
        }
        let mut wp = WindowPlacement {
            length: std::mem::size_of::<WindowPlacement>() as u32,
            ..Default::default()
        };
        if unsafe { GetWindowPlacement(hwnd, &mut wp) } != 0 {
            wp.rc_normal_position = rect;
            wp.show_cmd = SW_SHOWMAXIMIZED;
            unsafe {
                SetWindowPlacement(hwnd, &wp);
            }
        }
    }

    /// Temporarily intercepts `WM_WINDOWPOSCHANGING` during GTK4's fullscreen
    /// toggle to clear `SWP_NOCOPYBITS` and suppress intermediate small-rect
    /// resizes when transitioning between maximized and fullscreen
    pub struct WindowPosGuard {
        hwnd: *mut c_void,
        prev: isize,
    }

    impl WindowPosGuard {
        pub fn new(hwnd: *mut c_void, mode: GuardMode) -> Self {
            let prev = unsafe {
                SetWindowLongPtrW(hwnd, GWLP_WNDPROC, fullscreen_wndproc as *const () as isize)
            };
            if prev != 0 {
                GUARD.with(|g| g.set(Some((hwnd, prev, mode))));
            }
            Self { hwnd, prev }
        }
    }

    impl Drop for WindowPosGuard {
        fn drop(&mut self) {
            if self.prev != 0 {
                unsafe {
                    SetWindowLongPtrW(self.hwnd, GWLP_WNDPROC, self.prev);
                }
            }
            GUARD.with(|g| g.set(None));
        }
    }

    unsafe extern "system" fn fullscreen_wndproc(
        hwnd: *mut c_void,
        msg: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        let Some((target, prev, mode)) = GUARD.with(Cell::get) else {
            return 0;
        };
        let result = unsafe { CallWindowProcW(prev, hwnd, msg, wparam, lparam) };
        if hwnd == target && msg == WM_WINDOWPOSCHANGING && lparam != 0 {
            let wp = unsafe { &mut *(lparam as *mut WindowPos) };
            wp.flags &= !SWP_NOCOPYBITS;
            match mode {
                GuardMode::KeepPixels => {}
                GuardMode::EnterFromMaximized => {
                    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
                    if (style & WS_POPUP) == 0 {
                        wp.flags |= SWP_NOMOVE | SWP_NOSIZE;
                    }
                }
                GuardMode::ExitToMaximized => {
                    if !is_zoomed(hwnd) && (wp.flags & SWP_STATECHANGED) == 0 {
                        wp.flags |= SWP_NOMOVE | SWP_NOSIZE;
                    }
                }
            }
        }
        result
    }
}

/// A header bar over a hero picture at the top of `scroller`: clear at the
/// top, solid once anything scrolls under it
pub fn header_over_hero(header: &adw::HeaderBar, scroller: &gtk::ScrolledWindow) {
    header.add_css_class("over-hero");
    let header = header.downgrade();
    let update = move |adjustment: &gtk::Adjustment| {
        let Some(header) = header.upgrade() else {
            return;
        };
        if adjustment.value() > 0.0 {
            header.add_css_class("scrolled");
        } else {
            header.remove_css_class("scrolled");
        }
    };
    let adjustment = scroller.vadjustment();
    update(&adjustment);
    adjustment.connect_value_changed(update);
}

pub fn loading() -> gtk::Widget {
    let spinner = adw::Spinner::builder()
        .width_request(48)
        .height_request(48)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();
    spinner.upcast()
}

pub fn error_page(message: &str, retry: impl Fn() + 'static) -> gtk::Widget {
    let button = gtk::Button::builder()
        .label("Try Again")
        .halign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    button.connect_clicked(move |_| retry());
    adw::StatusPage::builder()
        .icon_name("network-error-symbolic")
        .title("Couldn't load this")
        .description(glib::markup_escape_text(message).as_str())
        .child(&button)
        .vexpand(true)
        .build()
        .upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_page_target_maps_episodes_and_seasons_to_their_series() {
        let episode = api::Item {
            id: "ep1".into(),
            kind: "Episode".into(),
            name: "Pilot".into(),
            series_id: Some("show1".into()),
            series_name: Some("Show".into()),
            season_id: Some("s1".into()),
            ..Default::default()
        };
        let (target, season) = item_page_target(&episode).unwrap();
        assert_eq!(target.id, "show1");
        assert_eq!(target.kind, "Series");
        assert_eq!(target.name, "Show");
        assert_eq!(season.as_deref(), Some("s1"));

        let season_item = api::Item {
            id: "s2".into(),
            kind: "Season".into(),
            series_id: Some("show1".into()),
            series_name: Some("Show".into()),
            ..Default::default()
        };
        let (target, season) = item_page_target(&season_item).unwrap();
        assert_eq!(target.id, "show1");
        assert_eq!(season.as_deref(), Some("s2"));

        let movie = api::Item {
            id: "m1".into(),
            kind: "Movie".into(),
            name: "Film".into(),
            ..Default::default()
        };
        let (target, season) = item_page_target(&movie).unwrap();
        assert_eq!(target.id, "m1");
        assert_eq!(season, None);

        let folder = api::Item {
            id: "f1".into(),
            kind: "Folder".into(),
            ..Default::default()
        };
        assert!(item_page_target(&folder).is_none());
    }
}
