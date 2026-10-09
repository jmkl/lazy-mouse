//! Windows side: a low-level mouse hook that swallows real mouse moves and
//! moves the cursor to the lazy brush position instead, plus a click-through
//! overlay window that draws the rope between the brush and the real pointer.
//!
//! Hotkeys: Ctrl+Alt+L toggles, Ctrl+Alt+Q quits.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CombineRgn,
    CreateCompatibleDC, CreateDIBSection, CreateEllipticRgn, CreateRectRgn, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, RGN_DIFF, ReleaseDC, SelectObject,
    SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetMessageW,
    GetSystemMetrics, HC_ACTION, HTTRANSPARENT, LLMHF_INJECTED, MA_NOACTIVATE, MSG, MSLLHOOKSTRUCT,
    PostThreadMessageW, RegisterClassW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SetCursorPos, SetTimer, SetWindowsHookExW,
    ShowWindow, ULW_ALPHA, UnhookWindowsHookEx, UpdateLayeredWindow, WH_MOUSE_LL, WM_APP,
    WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_NCHITTEST, WM_QUIT, WM_RBUTTONDOWN, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::w;

use crate::brush::{LazyBrush, Point};
use crate::config::{self, Settings, Update};
use crate::render::Renderer;

const ID_TOGGLE: i32 = 1;
const ID_QUIT: i32 = 2;
const VK_L: u32 = 0x4C;
const VK_Q: u32 = 0x51;
/// Set with the LAZY_DEBUG environment variable: logs button events to stderr.
static DEBUG: AtomicBool = AtomicBool::new(false);
/// Radius (px) of the hole around the cursor that the overlay never covers.
const HOLE: i32 = 6;
const FRAME_MS: u32 = 16; // overlay redraw / brush animation rate (~60 Hz); the hook runs at the mouse rate

/// Everything the hook needs. `Copy`, so a `Cell` is enough (the hook runs on
/// the thread that installed it, so no locking).
#[derive(Clone, Copy)]
struct Engine {
    enabled: bool,
    /// Something changed since the overlay was last drawn.
    dirty: bool,
    brush: LazyBrush,
    /// Virtual (unsmoothed) pointer position.
    pointer: Point,
    /// Virtual desktop bounds, inclusive.
    min: Point,
    max: Point,
    /// Time of the last brush step.
    last: Option<Instant>,
}

const OFF: Engine = Engine {
    enabled: false,
    dirty: false,
    brush: LazyBrush::new(0.0, Point::ZERO),
    pointer: Point::ZERO,
    min: Point::ZERO,
    max: Point::ZERO,
    last: None,
};

/// Written by the console thread, read by the main thread.
static PENDING: Mutex<Update> = Mutex::new(Update::NONE);

fn pending() -> MutexGuard<'static, Update> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

thread_local! {
    static SETTINGS: Cell<Settings> = const { Cell::new(Settings::DEFAULT) };
    static ENGINE: Cell<Engine> = const { Cell::new(OFF) };
    static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
}

/// Window procedure of the overlay: it must never take input. Hit tests are
/// reported as transparent and it refuses activation; everything else is default.
/// (`DefWindowProcW` has the Rust ABI in the `windows` crate, so it can't be used
/// as the window procedure directly.)
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Click-through, always-on-top, per-pixel-alpha window. Its size is fixed (the
/// rope never leaves a square of side ~2*radius around the brush); each frame it
/// is moved to the brush and its bitmap replaced.
struct Overlay {
    hwnd: HWND,
    screen: HDC,
    mem: HDC,
    bmp: HBITMAP,
    /// Bitmap that was selected into `mem` before ours; restored on drop.
    old: HGDIOBJ,
    bits: *mut u32,
    renderer: Renderer,
}

impl Overlay {
    fn new(radius: f64) -> windows::core::Result<Self> {
        let renderer = Renderer::new(radius).ok_or(windows::core::Error::from_hresult(
            windows::core::HRESULT(0x8007000E_u32 as i32), // E_OUTOFMEMORY
        ))?;
        let side = renderer.side() as i32;

        unsafe {
            let hinst = HINSTANCE(GetModuleHandleW(None)?.0);
            let class = w!("LazyMouseOverlay");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: hinst,
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc); // fails harmlessly when the class already exists (resize)

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                class,
                w!(""),
                WS_POPUP,
                0,
                0,
                side,
                side,
                None,
                None,
                Some(hinst),
                None,
            )?;

            // Cut a small hole in the window's region around the cursor hotspot (the
            // window is always centred on it). The system never hit-tests outside a
            // window's region, so a click at the cursor can't land on the overlay,
            // whatever the transparency flags do.
            let half = side / 2;
            let full = CreateRectRgn(0, 0, side, side);
            let hole = CreateEllipticRgn(half - HOLE, half - HOLE, half + HOLE, half + HOLE);
            CombineRgn(Some(full), Some(full), Some(hole), RGN_DIFF);
            let _ = DeleteObject(hole.into());
            SetWindowRgn(hwnd, Some(full), false); // the system owns `full` from here

            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: side,
                    biHeight: -side, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = null_mut();
            let bmp = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let old = SelectObject(mem, bmp.into());

            Ok(Self {
                hwnd,
                screen,
                mem,
                bmp,
                old,
                bits: bits.cast(),
                renderer,
            })
        }
    }

    fn set_visible(&self, visible: bool) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }

    fn draw(&mut self, e: &Engine) {
        let b = e.brush.brush();
        // The cursor sits on the rounded brush position, so centre on that.
        let (cx, cy) = (b.x.round(), b.y.round());
        self.renderer
            .draw(Point { x: cx, y: cy }, e.pointer, e.brush.radius());

        let side = self.renderer.side();
        // SAFETY: `bits` points to a side*side u32 DIB section that lives as long as `self`.
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, (side * side) as usize) };
        for (d, px) in dst.iter_mut().zip(self.renderer.data().chunks_exact(4)) {
            // tiny-skia: premultiplied RGBA bytes. The DIB wants premultiplied BGRA
            // (UpdateLayeredWindow, ULW_ALPHA + AC_SRC_ALPHA), i.e. swap R and B.
            let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]); // 0xAABBGGRR
            *d = (v & 0xFF00_FF00) | ((v & 0xFF) << 16) | ((v >> 16) & 0xFF);
        }

        let half = (side / 2) as i32;
        let pos = POINT {
            x: cx as i32 - half,
            y: cy as i32 - half,
        };
        let size = SIZE {
            cx: side as i32,
            cy: side as i32,
        };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd,
                Some(self.screen),
                Some(&pos),
                Some(&size),
                Some(self.mem),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            // A bitmap must not be selected into a DC when it is deleted.
            let _ = SelectObject(self.mem, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.mem);
            ReleaseDC(None, self.screen);
        }
    }
}

pub fn run(s: Settings) -> windows::core::Result<()> {
    unsafe {
        // Without this, coordinates may be DPI-virtualized. Fails harmlessly if already set.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        SETTINGS.set(s);
        DEBUG.store(std::env::var_os("LAZY_DEBUG").is_some(), Ordering::Relaxed);

        let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
        RegisterHotKey(None, ID_TOGGLE, mods, VK_L)?;
        RegisterHotKey(None, ID_QUIT, mods, VK_Q)?;

        let hinst = HINSTANCE(GetModuleHandleW(None)?.0);
        // LAZY_NO_OVERLAY=1: no rope window at all (to tell overlay problems from hook problems)
        if std::env::var_os("LAZY_NO_OVERLAY").is_none() {
            OVERLAY.set(Some(Overlay::new(s.radius)?));
        }

        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), Some(hinst), 0)?;
        SetTimer(None, 0, FRAME_MS, None); // WM_TIMER arrives in the loop below

        println!(
            "lazy-mouse: {} | Ctrl+Alt+L toggle | Ctrl+Alt+Q quit",
            s.summary()
        );
        spawn_console(GetCurrentThreadId());
        // The message loop must keep running: low-level hooks are delivered through it.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            match msg.message {
                WM_TIMER => frame(),
                WM_HOTKEY => match msg.wParam.0 as i32 {
                    ID_TOGGLE => toggle(),
                    ID_QUIT => break,
                    _ => {}
                },
                WM_APP => apply_pending(),
                _ => {}
            }
        }

        UnhookWindowsHookEx(hook)?;
    }
    Ok(())
}

fn post(thread: u32, msg: u32) {
    unsafe {
        let _ = PostThreadMessageW(thread, msg, WPARAM(0), LPARAM(0));
    }
}

fn spawn_console(main_thread: u32) {
    config::print_help();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            let cmd = config::parse_command(&line);
            for word in &cmd.ignored {
                println!("ignored: {word}");
            }
            if cmd.quit {
                post(main_thread, WM_QUIT);
                return;
            }
            if cmd.update != Update::NONE {
                pending().merge(cmd.update);
                post(main_thread, WM_APP);
            }
        }
    });
}

/// Runs on the main thread, so it can touch ENGINE and OVERLAY.
fn apply_pending() {
    let u = std::mem::take(&mut *pending());
    let mut s = SETTINGS.get();
    let old_radius = s.radius;
    s.apply(&u);

    // The overlay size depends on the radius, so build the new one first.
    if s.radius != old_radius && OVERLAY.with_borrow(|o| o.is_some()) {
        let enabled = ENGINE.get().enabled;
        match Overlay::new(s.radius) {
            Ok(new) => {
                new.set_visible(enabled);
                OVERLAY.with_borrow_mut(|o| *o = Some(new)); // old overlay is dropped here
            }
            Err(err) => {
                eprintln!("lazy-mouse: could not resize overlay: {err}");
                return; // nothing changed
            }
        }
    }
    SETTINGS.set(s);

    // Read the engine only now, after anything that could have pumped messages.
    let mut e = ENGINE.get();
    e.brush.set_radius(s.radius);
    e.brush.set_friction(s.friction);
    e.brush.set_spring(s.spring);
    e.dirty |= e.enabled;
    ENGINE.set(e);
    println!("lazy-mouse: {}", s.summary());
}

/// Step the brush to now. Returns where the real cursor should go if the brush moved.
/// The caller stores the engine *before* calling `move_cursor`, because
/// `SetCursorPos` can re-enter the hook.
fn advance(e: &mut Engine) -> Option<(i32, i32)> {
    let now = Instant::now();
    let dt = e
        .last
        .replace(now)
        .map_or(0.0, |t| now.duration_since(t).as_secs_f64());
    if !e.brush.step(e.pointer, dt) {
        return None;
    }
    let b = e.brush.brush();
    Some((
        b.x.round().clamp(e.min.x, e.max.x) as i32,
        b.y.round().clamp(e.min.y, e.max.y) as i32,
    ))
}

fn move_cursor((x, y): (i32, i32)) {
    let _ = unsafe { SetCursorPos(x, y) };
}

/// Animate the brush (it keeps gliding after the mouse stops) and redraw the
/// overlay if anything changed since the last frame.
fn frame() {
    let mut e = ENGINE.get();
    if !e.enabled {
        return;
    }
    let target = advance(&mut e);
    let redraw = e.dirty || target.is_some();
    // Clear before drawing, so a move that lands during the draw is not lost.
    e.dirty = false;
    ENGINE.set(e);
    if let Some(t) = target {
        move_cursor(t);
    }
    if redraw {
        OVERLAY.with_borrow_mut(|o| {
            if let Some(o) = o {
                o.draw(&e);
            }
        });
    }
}

fn toggle() {
    let s = SETTINGS.get();
    let mut e = ENGINE.get();
    if e.enabled {
        e.enabled = false;
        OVERLAY.with_borrow(|o| o.iter().for_each(|o| o.set_visible(false)));
        println!("lazy-mouse: off");
    } else {
        let mut p = POINT::default();
        if unsafe { GetCursorPos(&mut p) }.is_err() {
            return;
        }
        let at = Point {
            x: p.x as f64,
            y: p.y as f64,
        };
        let (x0, y0, w, h) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        e = Engine {
            enabled: true,
            dirty: true,
            brush: s.brush(at),
            pointer: at,
            min: Point {
                x: x0 as f64,
                y: y0 as f64,
            },
            max: Point {
                x: (x0 + w - 1) as f64,
                y: (y0 + h - 1) as f64,
            },
            last: None,
        };
        OVERLAY.with_borrow(|o| o.iter().for_each(|o| o.set_visible(true)));
        println!("lazy-mouse: on");
    }
    ENGINE.set(e);
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_MOUSE_LL, lparam points to a valid MSLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        // Injected events (our own SetCursorPos, other tools) are never touched.
        let injected = info.flags & LLMHF_INJECTED != 0;
        let msg = wparam.0 as u32;
        match msg {
            WM_MOUSEMOVE if !injected && on_move(info.pt.x, info.pt.y) => {
                return LRESULT(1); // swallow the real move
            }
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_LBUTTONUP => {
                on_button(msg, injected, info.pt.x, info.pt.y);
            }
            _ => {}
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Button events are never swallowed. On a press the virtual pointer is reset to
/// the brush (like lazy-brush's `both: true` on touch start), so a stroke always
/// starts at the cursor with the whole lazy area around it.
fn on_button(msg: u32, injected: bool, x: i32, y: i32) {
    let mut e = ENGINE.get();
    if !e.enabled {
        return;
    }
    if DEBUG.load(Ordering::Relaxed) {
        let (b, p) = (e.brush.brush(), e.pointer);
        eprintln!(
            "msg={msg:#x} injected={injected} pt=({x},{y}) brush=({:.1},{:.1}) pointer=({:.1},{:.1})",
            b.x, b.y, p.x, p.y
        );
    }
    if !injected && msg != WM_LBUTTONUP {
        e.pointer = e.brush.brush();
        e.dirty = true;
        ENGINE.set(e);
    }
}

/// Returns `true` if the event was handled and should be swallowed.
/// Kept tiny (hooks have a time limit): it only updates state and moves the cursor.
#[inline]
fn on_move(x: i32, y: i32) -> bool {
    let mut e = ENGINE.get();
    if !e.enabled {
        return false;
    }

    // `x, y` is where the OS would put the cursor. The real cursor stays where we
    // last put it (the brush), so the difference is this event's movement.
    let mut cur = POINT::default();
    if unsafe { GetCursorPos(&mut cur) }.is_err() {
        return false;
    }
    e.pointer.x = (e.pointer.x + (x - cur.x) as f64).clamp(e.min.x, e.max.x);
    e.pointer.y = (e.pointer.y + (y - cur.y) as f64).clamp(e.min.y, e.max.y);

    let target = advance(&mut e);
    e.dirty = true; // the rope changes whenever the pointer moves
    ENGINE.set(e);
    if let Some(t) = target {
        move_cursor(t);
    }
    true
}
