//! Jelly: a native GTK and libadwaita Jellyfin client that plays everything
//! directly with mpv, never asking the server to transcode.

// No console window behind the app on Windows (the command line attaches to
// the terminal it was started from instead)
#![cfg_attr(windows, windows_subsystem = "windows")]

mod api;
mod cli;
mod http;
mod images;
mod mpv;
mod seerr;
mod session;
#[cfg(feature = "screenshot")]
mod shot;
mod ui;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

const APP_ID: &str = "io.github.joaotroxo.Jelly";

thread_local! {
    static CTX: RefCell<Option<Rc<ui::Ctx>>> = const { RefCell::new(None) };
}

fn main() -> glib::ExitCode {
    #[cfg(windows)]
    if connect_output() && std::env::var_os("JELLY_MPV_LOG").is_none() {
        // SAFETY: no other thread is running yet to read the environment
        unsafe { std::env::set_var("JELLY_MPV_LOG", "all=warn") };
    }
    let args: Vec<String> = std::env::args().collect();
    // Commands that only print run here, without the app
    match cli::parse(&args[1..]) {
        Ok(command) => {
            if let Some(code) = cli::print(&command) {
                return code;
            }
        }
        Err(e) => {
            eprintln!("jelly: {e}\n\n{}", cli::USAGE);
            return glib::ExitCode::from(2);
        }
    }

    // The rest go to the app: this process's, or the one already running,
    // which gets them over D-Bus while this process waits to hear how it went
    // On Windows, every process is an app of its own: GLib's D-Bus there is a
    // daemon that can outlive a crash and then keep the app from starting
    let flags = if cfg!(windows) {
        gio::ApplicationFlags::HANDLES_COMMAND_LINE | gio::ApplicationFlags::NON_UNIQUE
    } else {
        gio::ApplicationFlags::HANDLES_COMMAND_LINE
    };
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();
    app.connect_startup(|_| {
        gio::spawn_blocking(images::trim_cache);
        // Dark unless the system asks for light: better around video
        let style = adw::StyleManager::default();
        style.set_color_scheme(adw::ColorScheme::PreferDark);
        // mpv refuses to run with a decimal comma (GTK sets the user's locale)
        unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) };
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let css = gtk::CssProvider::new();
        css.load_from_string(include_str!("style.css"));
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        // The dark palette, while libadwaita is dark
        let dark = gtk::CssProvider::new();
        dark.load_from_string(include_str!("style-dark.css"));
        let follow = move |style: &adw::StyleManager| {
            if style.is_dark() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &dark,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            } else {
                gtk::style_context_remove_provider_for_display(&display, &dark);
            }
        };
        follow(&style);
        style.connect_dark_notify(follow);
    });
    app.connect_activate(|app| {
        window(app, true);
    });
    app.connect_command_line(|app, line| {
        let args: Vec<String> = line
            .arguments()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let command = match cli::parse(args.get(1..).unwrap_or_default()) {
            Ok(command) => command,
            Err(e) => {
                line.printerr_literal(&format!("jelly: {e}\n"));
                return glib::ExitCode::from(2);
            }
        };
        // A local file plays alone in the window, without signing in
        let ctx = window(app, !matches!(command, cli::Command::PlayFile(_)));
        let (line, c) = (line.clone(), ctx.clone());
        cli::act(&ctx, command, move |result| {
            if let Err(e) = result {
                c.toast(&e);
                line.printerr_literal(&format!("jelly: {e}\n"));
                line.set_exit_status(1);
            }
            // The process that sent the command exits now
            line.done();
        });
        glib::ExitCode::SUCCESS
    });
    app.run_with_args(&args)
}

/// The app's window: the open one, or a new one, signed in with `sign_in`
/// (without, a local file is about to play in it)
fn window(app: &adw::Application, sign_in: bool) -> Rc<ui::Ctx> {
    if let Some(ctx) = CTX.with(|c| c.borrow().clone()) {
        ctx.window.present();
        return ctx;
    }
    let ctx = ui::Ctx::new(app);
    ctx.window.connect_close_request(|_| {
        ui::player::shutdown_current();
        glib::Propagation::Proceed
    });
    if sign_in {
        match session::load() {
            Some(saved) => ctx.sign_in(saved),
            None => ctx.nav.replace(&[ui::login::page(&ctx)]),
        }
    }
    // On logout or shutdown, close like the window's close button (saving where playback stopped)
    #[cfg(unix)]
    for signal in [libc::SIGTERM, libc::SIGINT] {
        let window = ctx.window.downgrade();
        glib::unix_signal_add_local(signal, move || {
            if let Some(window) = window.upgrade() {
                window.close();
            }
            glib::ControlFlow::Continue
        });
    }
    ctx.window.present();
    #[cfg(feature = "screenshot")]
    shot::schedule(&ctx.window);
    CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
    ctx
}

/// Give a Windows app, which has no console window of its own, somewhere to
/// print: the terminal it was started from, if any, or else jelly.log in
/// %LOCALAPPDATA%\jelly, started afresh each time. True for the log (then
/// mpv's warnings go there too). Output that already goes somewhere, a pipe
/// or a file, stays as it is.
#[cfg(windows)]
fn connect_output() -> bool {
    use std::ffi::{OsStr, c_char, c_void};
    use std::os::windows::ffi::OsStrExt;
    unsafe extern "system" {
        fn AttachConsole(process: u32) -> i32;
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
    }
    // The C runtime's stdout and stderr, which GLib, GTK and mpv print to
    unsafe extern "C" {
        fn __acrt_iob_func(index: u32) -> *mut libc::FILE;
        fn _wfreopen(
            path: *const u16,
            mode: *const u16,
            stream: *mut libc::FILE,
        ) -> *mut libc::FILE;
        fn _fileno(stream: *mut libc::FILE) -> i32;
        fn _dup2(from: i32, to: i32) -> i32;
        fn _get_osfhandle(fd: i32) -> isize;
        fn setvbuf(stream: *mut libc::FILE, buffer: *mut c_char, mode: i32, size: usize) -> i32;
    }
    const PARENT_PROCESS: u32 = u32::MAX;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const UNBUFFERED: i32 = 4;
    let wide = |s: &OsStr| s.encode_wide().chain([0]).collect::<Vec<u16>>();

    let current = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if !current.is_null() && current as isize != -1 {
        return false;
    }
    let console = unsafe { AttachConsole(PARENT_PROCESS) } != 0;
    let path = if console {
        wide("CONOUT$".as_ref())
    } else {
        let dir = session::config_dir();
        let _ = std::fs::create_dir_all(&dir);
        wide(dir.join("jelly.log").as_os_str())
    };
    let write = wide("w".as_ref());
    unsafe {
        let (output, error) = (__acrt_iob_func(1), __acrt_iob_func(2));
        if _wfreopen(path.as_ptr(), write.as_ptr(), error).is_null() {
            return false;
        }
        // stdout writes to the same file as stderr, at the same place in it
        if !_wfreopen(wide("NUL".as_ref()).as_ptr(), write.as_ptr(), output).is_null() {
            _dup2(_fileno(error), _fileno(output));
        }
        // Unbuffered, so the log has everything up to a crash; and Rust's
        // own printing goes to the same places
        for (stream, which) in [(output, STD_OUTPUT_HANDLE), (error, STD_ERROR_HANDLE)] {
            setvbuf(stream, std::ptr::null_mut(), UNBUFFERED, 0);
            SetStdHandle(which, _get_osfhandle(_fileno(stream)) as *mut c_void);
        }
    }
    !console
}
