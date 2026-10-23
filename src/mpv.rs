//! Minimal bindings to libmpv: the client API to control playback and the
//! render API to draw video into a GtkGLArea. Only what the player needs.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

#[repr(C)]
struct MpvHandle {
    _private: [u8; 0],
}

#[repr(C)]
struct MpvRenderContext {
    _private: [u8; 0],
}

#[repr(C)]
struct RawEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct RawEventProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct RawEventEndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: i64,
    playlist_insert_id: i64,
    playlist_insert_num_entries: c_int,
}

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

type Callback = unsafe extern "C" fn(*mut c_void);

#[link(name = "mpv")]
unsafe extern "C" {
    fn mpv_create() -> *mut MpvHandle;
    fn mpv_initialize(ctx: *mut MpvHandle) -> c_int;
    fn mpv_terminate_destroy(ctx: *mut MpvHandle);
    fn mpv_set_option_string(
        ctx: *mut MpvHandle,
        name: *const c_char,
        data: *const c_char,
    ) -> c_int;
    fn mpv_set_property_string(
        ctx: *mut MpvHandle,
        name: *const c_char,
        data: *const c_char,
    ) -> c_int;
    fn mpv_get_property_string(ctx: *mut MpvHandle, name: *const c_char) -> *mut c_char;
    fn mpv_free(data: *mut c_void);
    fn mpv_command(ctx: *mut MpvHandle, args: *mut *const c_char) -> c_int;
    fn mpv_observe_property(
        ctx: *mut MpvHandle,
        reply_userdata: u64,
        name: *const c_char,
        format: c_int,
    ) -> c_int;
    fn mpv_wait_event(ctx: *mut MpvHandle, timeout: f64) -> *mut RawEvent;
    fn mpv_set_wakeup_callback(ctx: *mut MpvHandle, cb: Option<Callback>, d: *mut c_void);
    fn mpv_error_string(error: c_int) -> *const c_char;

    fn mpv_render_context_create(
        res: *mut *mut MpvRenderContext,
        mpv: *mut MpvHandle,
        params: *mut RenderParam,
    ) -> c_int;
    fn mpv_render_context_set_update_callback(
        ctx: *mut MpvRenderContext,
        cb: Option<Callback>,
        d: *mut c_void,
    );
    fn mpv_render_context_update(ctx: *mut MpvRenderContext) -> u64;
    fn mpv_render_context_render(ctx: *mut MpvRenderContext, params: *mut RenderParam) -> c_int;
    fn mpv_render_context_report_swap(ctx: *mut MpvRenderContext);
    fn mpv_render_context_free(ctx: *mut MpvRenderContext);
}

/// An OpenGL function, looked up the way the platform's OpenGL wants: GTK
/// draws with EGL on Linux (under Wayland and X11 alike)
#[cfg(all(unix, not(target_os = "macos")))]
fn gl_function(name: &CStr) -> *mut c_void {
    #[link(name = "EGL")]
    unsafe extern "C" {
        fn eglGetProcAddress(name: *const c_char) -> *mut c_void;
    }
    unsafe { eglGetProcAddress(name.as_ptr()) }
}

/// With WGL on Windows
#[cfg(windows)]
fn gl_function(name: &CStr) -> *mut c_void {
    #[link(name = "opengl32")]
    unsafe extern "system" {
        fn wglGetProcAddress(name: *const c_char) -> *mut c_void;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleA(name: *const c_char) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }
    // WGL only has what came after OpenGL 1.1 (some drivers say "none" with
    // 1, 2, 3 or -1); opengl32.dll has the rest
    let f = unsafe { wglGetProcAddress(name.as_ptr()) };
    if !matches!(f as isize, -1..=3) {
        return f;
    }
    unsafe { GetProcAddress(GetModuleHandleA(c"opengl32.dll".as_ptr()), name.as_ptr()) }
}

/// From the OpenGL framework on macOS
#[cfg(target_os = "macos")]
fn gl_function(name: &CStr) -> *mut c_void {
    let framework = c"/System/Library/Frameworks/OpenGL.framework/OpenGL";
    unsafe {
        libc::dlsym(
            libc::dlopen(framework.as_ptr(), libc::RTLD_LAZY),
            name.as_ptr(),
        )
    }
}

const FORMAT_NONE: c_int = 0;
const FORMAT_STRING: c_int = 1;
const FORMAT_FLAG: c_int = 3;
const FORMAT_INT64: c_int = 4;
const FORMAT_DOUBLE: c_int = 5;

const EVENT_NONE: c_int = 0;
const EVENT_SHUTDOWN: c_int = 1;
const EVENT_START_FILE: c_int = 6;
const EVENT_END_FILE: c_int = 7;
const EVENT_FILE_LOADED: c_int = 8;
const EVENT_PLAYBACK_RESTART: c_int = 21;
const EVENT_PROPERTY_CHANGE: c_int = 22;

const RENDER_PARAM_INVALID: c_int = 0;
const RENDER_PARAM_API_TYPE: c_int = 1;
const RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const RENDER_PARAM_OPENGL_FBO: c_int = 3;
const RENDER_PARAM_FLIP_Y: c_int = 4;
const RENDER_PARAM_X11_DISPLAY: c_int = 8;
const RENDER_PARAM_WL_DISPLAY: c_int = 9;

const RENDER_UPDATE_FRAME: u64 = 1;

fn error_string(code: c_int) -> String {
    unsafe { CStr::from_ptr(mpv_error_string(code)) }
        .to_string_lossy()
        .into_owned()
}

fn check(code: c_int, what: &str) -> Result<(), String> {
    if code < 0 {
        Err(format!("{what}: {}", error_string(code)))
    } else {
        Ok(())
    }
}

fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap()
}

#[derive(Debug, Clone, Copy)]
pub enum Format {
    /// Only get notified that it changed
    None,
    Flag,
    Double,
    Str,
}

impl Format {
    fn raw(self) -> c_int {
        match self {
            Format::None => FORMAT_NONE,
            Format::Flag => FORMAT_FLAG,
            Format::Double => FORMAT_DOUBLE,
            Format::Str => FORMAT_STRING,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Unavailable, or only a change notification
    None,
    Flag(bool),
    Int(i64),
    Double(f64),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Eof,
    Stop,
    Error,
    Other,
}

#[derive(Debug)]
pub enum Event {
    StartFile,
    FileLoaded,
    EndFile {
        reason: EndReason,
        error: Option<String>,
    },
    PlaybackRestart,
    Property {
        name: String,
        value: Value,
    },
    Shutdown,
}

/// An mpv instance. All methods are callable from the main thread; mpv itself
/// is thread-safe, but the callbacks are invoked from mpv's threads.
pub struct Mpv {
    handle: *mut MpvHandle,
    wakeup: *mut Box<dyn Fn() + Send>,
}

impl Mpv {
    /// Create and initialize mpv with the given options (set before init).
    pub fn new(options: &[(&str, &str)]) -> Result<Mpv, String> {
        let handle = unsafe { mpv_create() };
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }
        let mpv = Mpv {
            handle,
            wakeup: ptr::null_mut(),
        };
        for (name, value) in options {
            let (n, v) = (cstring(name), cstring(value));
            check(
                unsafe { mpv_set_option_string(handle, n.as_ptr(), v.as_ptr()) },
                &format!("option {name}={value}"),
            )?;
        }
        check(unsafe { mpv_initialize(handle) }, "mpv_initialize")?;
        Ok(mpv)
    }

    pub fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (cstring(name), cstring(value));
        check(
            unsafe { mpv_set_property_string(self.handle, n.as_ptr(), v.as_ptr()) },
            &format!("set {name}={value}"),
        )
    }

    pub fn get(&self, name: &str) -> Option<String> {
        let n = cstring(name);
        let raw = unsafe { mpv_get_property_string(self.handle, n.as_ptr()) };
        if raw.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        unsafe { mpv_free(raw.cast()) };
        Some(s)
    }

    pub fn command(&self, args: &[&str]) -> Result<(), String> {
        let owned: Vec<CString> = args.iter().map(|a| cstring(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        ptrs.push(ptr::null());
        check(
            unsafe { mpv_command(self.handle, ptrs.as_mut_ptr()) },
            args.first().copied().unwrap_or("command"),
        )
    }

    pub fn observe(&self, name: &str, format: Format) {
        let n = cstring(name);
        unsafe { mpv_observe_property(self.handle, 0, n.as_ptr(), format.raw()) };
    }

    /// Call `f` (from an mpv thread) whenever events are waiting; drain them
    /// with `next_event` on the main thread. `None` removes the callback.
    pub fn set_wakeup(&mut self, f: Option<Box<dyn Fn() + Send>>) {
        unsafe extern "C" fn trampoline(d: *mut c_void) {
            unsafe {
                let f = &*(d as *const Box<dyn Fn() + Send>);
                f();
            }
        }
        unsafe { mpv_set_wakeup_callback(self.handle, None, ptr::null_mut()) };
        if !self.wakeup.is_null() {
            drop(unsafe { Box::from_raw(self.wakeup) });
            self.wakeup = ptr::null_mut();
        }
        if let Some(f) = f {
            self.wakeup = Box::into_raw(Box::new(f));
            unsafe { mpv_set_wakeup_callback(self.handle, Some(trampoline), self.wakeup.cast()) };
        }
    }

    /// The next pending event, without blocking.
    pub fn next_event(&self) -> Option<Event> {
        loop {
            let ev = unsafe { &*mpv_wait_event(self.handle, 0.0) };
            match ev.event_id {
                EVENT_NONE => return None,
                EVENT_SHUTDOWN => return Some(Event::Shutdown),
                EVENT_START_FILE => return Some(Event::StartFile),
                EVENT_FILE_LOADED => return Some(Event::FileLoaded),
                EVENT_PLAYBACK_RESTART => return Some(Event::PlaybackRestart),
                EVENT_END_FILE => {
                    let end = unsafe { &*(ev.data as *const RawEventEndFile) };
                    let reason = match end.reason {
                        0 => EndReason::Eof,
                        2 => EndReason::Stop,
                        4 => EndReason::Error,
                        _ => EndReason::Other,
                    };
                    let error = (end.error < 0).then(|| error_string(end.error));
                    return Some(Event::EndFile { reason, error });
                }
                EVENT_PROPERTY_CHANGE => {
                    let prop = unsafe { &*(ev.data as *const RawEventProperty) };
                    let name = unsafe { CStr::from_ptr(prop.name) }
                        .to_string_lossy()
                        .into_owned();
                    let value = unsafe {
                        match prop.format {
                            FORMAT_FLAG => Value::Flag(*(prop.data as *const c_int) != 0),
                            FORMAT_INT64 => Value::Int(*(prop.data as *const i64)),
                            FORMAT_DOUBLE => Value::Double(*(prop.data as *const f64)),
                            FORMAT_STRING => {
                                let s = *(prop.data as *const *const c_char);
                                Value::Str(CStr::from_ptr(s).to_string_lossy().into_owned())
                            }
                            _ => Value::None,
                        }
                    };
                    return Some(Event::Property { name, value });
                }
                _ => continue,
            }
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        self.set_wakeup(None);
        unsafe { mpv_terminate_destroy(self.handle) };
    }
}

/// Draws mpv's video into the current OpenGL context (a GtkGLArea's).
/// Must be dropped with that context current, and before its `Mpv`.
pub struct Renderer {
    ctx: *mut MpvRenderContext,
    update: *mut Box<dyn Fn() + Send>,
}

unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    unsafe { gl_function(CStr::from_ptr(name)) }
}

/// The windowing system's display connection, which lets mpv hand
/// hardware-decoded frames to OpenGL without copying them
#[derive(Clone, Copy)]
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
pub enum NativeDisplay {
    Wayland(*mut c_void),
    X11(*mut c_void),
}

impl Renderer {
    pub fn new(mpv: &Mpv, display: Option<NativeDisplay>) -> Result<Renderer, String> {
        let api = cstring("opengl");
        let mut init = OpenGlInitParams {
            get_proc_address,
            get_proc_address_ctx: ptr::null_mut(),
        };
        let mut params = vec![
            RenderParam {
                kind: RENDER_PARAM_API_TYPE,
                data: api.as_ptr() as *mut c_void,
            },
            RenderParam {
                kind: RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: (&mut init as *mut OpenGlInitParams).cast(),
            },
        ];
        match display {
            Some(NativeDisplay::Wayland(d)) => params.push(RenderParam {
                kind: RENDER_PARAM_WL_DISPLAY,
                data: d,
            }),
            Some(NativeDisplay::X11(d)) => params.push(RenderParam {
                kind: RENDER_PARAM_X11_DISPLAY,
                data: d,
            }),
            None => {}
        }
        params.push(RenderParam {
            kind: RENDER_PARAM_INVALID,
            data: ptr::null_mut(),
        });
        let mut ctx = ptr::null_mut();
        check(
            unsafe { mpv_render_context_create(&mut ctx, mpv.handle, params.as_mut_ptr()) },
            "mpv render context",
        )?;
        Ok(Renderer {
            ctx,
            update: ptr::null_mut(),
        })
    }

    /// Call `f` (from an mpv thread) when `update` should be called.
    pub fn set_update_callback(&mut self, f: Box<dyn Fn() + Send>) {
        unsafe extern "C" fn trampoline(d: *mut c_void) {
            unsafe {
                let f = &*(d as *const Box<dyn Fn() + Send>);
                f();
            }
        }
        self.clear_update_callback();
        self.update = Box::into_raw(Box::new(f));
        unsafe {
            mpv_render_context_set_update_callback(self.ctx, Some(trampoline), self.update.cast())
        };
    }

    fn clear_update_callback(&mut self) {
        unsafe { mpv_render_context_set_update_callback(self.ctx, None, ptr::null_mut()) };
        if !self.update.is_null() {
            drop(unsafe { Box::from_raw(self.update) });
            self.update = ptr::null_mut();
        }
    }

    /// Whether a new frame is ready to be drawn.
    pub fn update(&self) -> bool {
        unsafe { mpv_render_context_update(self.ctx) & RENDER_UPDATE_FRAME != 0 }
    }

    /// Draw into framebuffer `fbo` of `w`×`h` pixels (the GLArea's).
    pub fn render(&self, fbo: i32, w: i32, h: i32) {
        let mut target = OpenGlFbo {
            fbo,
            w,
            h,
            internal_format: 0,
        };
        let mut flip: c_int = 1;
        let mut params = [
            RenderParam {
                kind: RENDER_PARAM_OPENGL_FBO,
                data: (&mut target as *mut OpenGlFbo).cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_FLIP_Y,
                data: (&mut flip as *mut c_int).cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        unsafe {
            mpv_render_context_render(self.ctx, params.as_mut_ptr());
            mpv_render_context_report_swap(self.ctx);
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.clear_update_callback();
        unsafe { mpv_render_context_free(self.ctx) };
    }
}

/// The framebuffer and viewport size GTK set up for the GLArea's render signal.
pub fn current_framebuffer() -> (i32, i32, i32) {
    type GetIntegerv = unsafe extern "system" fn(u32, *mut i32);
    const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
    const GL_VIEWPORT: u32 = 0x0BA2;
    let f = gl_function(c"glGetIntegerv");
    if f.is_null() {
        return (0, 0, 0);
    }
    let get: GetIntegerv = unsafe { std::mem::transmute(f) };
    let mut fbo = 0;
    let mut viewport = [0i32; 4];
    unsafe {
        get(GL_FRAMEBUFFER_BINDING, &mut fbo);
        get(GL_VIEWPORT, viewport.as_mut_ptr());
    }
    (fbo, viewport[2], viewport[3])
}
