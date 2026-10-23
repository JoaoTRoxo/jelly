//! The jelly.exe at the top of the Windows release's folder. The app is in
//! runtime\bin, with its DLLs (GLib, GTK and fontconfig find their data one
//! folder up from them), so that opening the folder shows jelly.exe rather
//! than a few hundred DLLs. This starts it with the same command line and
//! output, and exits with its exit code.

// No console window, like the app
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::env;
use std::process::{Command, exit};

fn main() {
    let Ok(launcher) = env::current_exe() else {
        fail("Couldn't find where jelly.exe is")
    };
    let app = launcher
        .with_file_name("runtime")
        .join("bin")
        .join(format!("jelly{}", env::consts::EXE_SUFFIX));
    #[cfg(windows)]
    windows::share_terminal();
    let mut child = match Command::new(&app).args(env::args_os().skip(1)).spawn() {
        Ok(child) => child,
        Err(e) => fail(&format!(
            "Couldn't start {} ({e}). Keep jelly.exe in its folder, next to runtime.",
            app.display()
        )),
    };
    #[cfg(windows)]
    windows::stay_with(&child);
    match child.wait() {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(e) => fail(&format!("Couldn't wait for the app ({e})")),
    }
}

/// Say what went wrong, and exit
fn fail(message: &str) -> ! {
    #[cfg(windows)]
    windows::show_error(message);
    #[cfg(not(windows))]
    eprintln!("jelly: {message}");
    exit(1)
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::ptr;

    type Handle = *mut c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> Handle;
        fn SetStdHandle(which: u32, handle: Handle) -> i32;
        fn AttachConsole(process: u32) -> i32;
        fn SetConsoleCtrlHandler(handler: *const c_void, add: i32) -> i32;
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, size: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(window: Handle, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }

    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const PARENT_PROCESS: u32 = u32::MAX;

    /// JOBOBJECT_BASIC_LIMIT_INFORMATION
    #[repr(C)]
    #[derive(Default)]
    struct BasicLimits {
        per_process_user_time: i64,
        per_job_user_time: i64,
        flags: u32,
        minimum_working_set: usize,
        maximum_working_set: usize,
        active_processes: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    /// JOBOBJECT_EXTENDED_LIMIT_INFORMATION
    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io_counters: [u64; 6],
        process_memory: usize,
        job_memory: usize,
        peak_process_memory: usize,
        peak_job_memory: usize,
    }

    const EXTENDED_LIMITS: i32 = 9;
    const SILENT_BREAKAWAY_OK: u32 = 0x1000;
    const KILL_ON_JOB_CLOSE: u32 = 0x2000;

    /// Whether this process's output goes somewhere: a pipe, a file or a
    /// terminal (the app checks the same)
    pub fn has_output() -> bool {
        let handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        !handle.is_null() && handle as isize != -1
    }

    /// Started from a terminal, which a Windows app doesn't get as its
    /// output, attach to it: the app attaches to its parent's terminal, and
    /// its parent is now this process
    pub fn share_terminal() {
        if has_output() {
            return;
        }
        let which = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
        let had = which.map(|w| unsafe { GetStdHandle(w) });
        if unsafe { AttachConsole(PARENT_PROCESS) } == 0 {
            return;
        }
        // The app gets what this process had, as if started directly, and
        // attaches by itself
        for (w, handle) in which.into_iter().zip(had) {
            unsafe { SetStdHandle(w, handle) };
        }
    }

    /// Close the app if this process is ended (from Task Manager, say), and
    /// leave Ctrl+C in the terminal to the app. What the app starts, such as
    /// a browser, stays open.
    pub fn stay_with(app: &Child) {
        // Ignoring Ctrl+C is inherited, so only now that the app has started
        unsafe { SetConsoleCtrlHandler(ptr::null(), 1) };
        let mut limits = ExtendedLimits::default();
        limits.basic.flags = KILL_ON_JOB_CLOSE | SILENT_BREAKAWAY_OK;
        unsafe {
            // Kept open until this process exits, which closes it
            let job = CreateJobObjectW(ptr::null(), ptr::null());
            if job.is_null() {
                return;
            }
            let size = size_of::<ExtendedLimits>() as u32;
            let info = (&raw const limits).cast();
            if SetInformationJobObject(job, EXTENDED_LIMITS, info, size) != 0 {
                AssignProcessToJobObject(job, app.as_raw_handle());
            }
        }
    }

    /// Say what went wrong: on the output, if there is one, or else in a
    /// message box
    pub fn show_error(message: &str) {
        if has_output() {
            eprintln!("jelly: {message}");
            return;
        }
        const ERROR_ICON: u32 = 0x10;
        let wide = |s: &str| {
            std::ffi::OsStr::new(s)
                .encode_wide()
                .chain([0])
                .collect::<Vec<u16>>()
        };
        let (text, caption) = (wide(message), wide("Jelly"));
        unsafe { MessageBoxW(ptr::null_mut(), text.as_ptr(), caption.as_ptr(), ERROR_ICON) };
    }
}
