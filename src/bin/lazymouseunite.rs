use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};

use std::sync::Mutex;
use std::time::Instant;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CombineRgn,
    CreateCompatibleDC, CreateDIBSection, CreateEllipticRgn, CreateRectRgn, DIB_RGB_COLORS,
    DeleteObject, GetDC, HDC, RGN_DIFF, SelectObject, SetWindowRgn,
};
use windows::Win32::Graphics::Gdi::{DeleteDC, HBITMAP, ReleaseDC};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, GetCursorPos, GetMessageW, GetSystemMetrics,
    HC_ACTION, HTTRANSPARENT, LLMHF_INJECTED, MA_NOACTIVATE, MSG, MSLLHOOKSTRUCT, RegisterClassW,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE,
    SW_SHOWNOACTIVATE, SetCursorPos, SetTimer, SetWindowsHookExW, ShowWindow, ULW_ALPHA,
    UnhookWindowsHookEx, UpdateLayeredWindow, WH_MOUSE_LL, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCHITTEST, WM_RBUTTONDOWN, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, PostThreadMessageW, WM_APP, WM_QUIT};
use windows::core::w;

use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Stroke,
    StrokeDash, Transform,
};

const RADIUS_MIN: f64 = 0.0;
const RADIUS_MAX: f64 = 300.0;
const FRICTION_MIN: f64 = 0.0;
const FRICTION_MAX: f64 = 1.0;
const STIFFNESS_MIN: f64 = 1.0;
const STIFFNESS_MAX: f64 = 2000.0;
const DAMPING_MIN: f64 = 0.0;
const DAMPING_MAX: f64 = 300.0;
const STRETCH_MIN: f64 = 1.0;
const STRETCH_MAX: f64 = 1.0e6;

const SUBSTEP: f64 = 0.004; // seconds

/// Thickness of the dark line, in px.
const ROPE_WIDTH: f32 = 2.0;
/// Extra thickness of the light outline under it (total outline = ROPE_WIDTH + this).
const OUTLINE_EXTRA: f32 = 1.0;
/// Dash pattern in px: visible length, then gap. Set `DASH_ON` to 0 for a solid line.
const DASH_ON: f32 = 6.0;
const DASH_OFF: f32 = 5.0;
/// Radius of the red pointer dot (the white ring is 1.5px wider).
const DOT_RADIUS: f32 = 6.0;

/// Empty border around the drawing so strokes and the dot are never cut off.
const MARGIN: u32 = 12;
const SAMPLES: usize = 32;
/// Square canvas centred on the brush, reaching `2 * radius` in every direction.
/// Without friction the pointer is never farther than `radius` from the brush;
/// with friction it can lag further, so the drawn end is clamped to the canvas.
pub struct Renderer {
    pixmap: Pixmap,
    side: u32,
    dash: Option<StrokeDash>,
}

impl Renderer {
    pub fn new(radius: f64) -> Option<Self> {
        let mut side = (radius * 4.0).ceil() as u32 + 2 * MARGIN;
        side += side & 1; // even, so the centre lands on a whole pixel
        let dash = if DASH_ON > 0.0 {
            StrokeDash::new(vec![DASH_ON, DASH_OFF], 0.0)
        } else {
            None
        };
        Some(Self {
            pixmap: Pixmap::new(side, side)?,
            side,
            dash,
        })
    }

    pub fn side(&self) -> u32 {
        self.side
    }

    /// Premultiplied RGBA pixels, row-major, `side * side` of them.
    pub fn pixels(&self) -> &[PremultipliedColorU8] {
        self.pixmap.pixels()
    }

    /// `brush` is the (integer-rounded) cursor position in screen coordinates,
    /// `pointer` the real pointer. Both are mapped into canvas space.
    pub fn draw(&mut self, brush: Point, pointer: Point, length: f64) {
        self.pixmap.fill(Color::TRANSPARENT);

        let half = f64::from(self.side) / 2.0;
        let a = Point { x: half, y: half };
        // clamp the drawn pointer end to the canvas (only matters with friction)
        let (mut vx, mut vy) = (pointer.x - brush.x, pointer.y - brush.y);
        let reach = half - f64::from(MARGIN);
        let d = vx.hypot(vy);
        if d > reach {
            vx *= reach / d;
            vy *= reach / d;
        }
        let b = Point {
            x: vx + half,
            y: vy + half,
        };

        let mut pts = [Point { x: 0.0, y: 0.0 }; SAMPLES];
        sample(a, b, length, &mut pts);

        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].x as f32, pts[0].y as f32);
        for p in &pts[1..] {
            pb.line_to(p.x as f32, p.y as f32);
        }

        let mut paint = Paint::default();
        paint.anti_alias = true;
        // round caps look nice on a solid line, but would blur the gaps of a dashed one
        let cap = if self.dash.is_some() {
            LineCap::Butt
        } else {
            LineCap::Round
        };
        let mut stroke = Stroke {
            line_cap: cap,
            line_join: LineJoin::Round,
            dash: self.dash.clone(),
            ..Stroke::default()
        };

        if let Some(path) = pb.finish() {
            // light outline under a dark core, so it reads on any background
            paint.set_color_rgba8(255, 255, 255, 200);
            stroke.width = ROPE_WIDTH + OUTLINE_EXTRA;
            self.pixmap
                .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            paint.set_color_rgba8(20, 20, 20, 235);
            stroke.width = ROPE_WIDTH;
            self.pixmap
                .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }

        // pointer end
        if let Some(dot) = PathBuilder::from_circle(b.x as f32, b.y as f32, DOT_RADIUS + 1.5) {
            paint.set_color_rgba8(255, 255, 255, 220);
            self.pixmap
                .fill_path(&dot, &paint, FillRule::Winding, Transform::identity(), None);
            if let Some(core) = PathBuilder::from_circle(b.x as f32, b.y as f32, DOT_RADIUS) {
                paint.set_color_rgba8(230, 60, 60, 255);
                self.pixmap.fill_path(
                    &core,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
    }

    #[cfg(test)]
    pub fn save_png(&self, path: &str) {
        self.pixmap.save_png(path).unwrap();
    }
}

/// Fill `out` with evenly spaced (in x) points of a rope of `length` hanging
/// from `a` to `b`. If the rope is not longer than the distance between the
/// points, it is a straight line.
pub fn sample(a: Point, b: Point, length: f64, out: &mut [Point]) {
    let n = out.len();
    if n == 0 {
        return;
    }
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let dist = dx.hypot(dy);

    // Taut (or nearly): straight line.
    if n < 2 || length - dist < 1e-3 {
        for (i, p) in out.iter_mut().enumerate() {
            let t = if n > 1 {
                i as f64 / (n - 1) as f64
            } else {
                0.0
            };
            *p = Point {
                x: a.x + dx * t,
                y: a.y + dy * t,
            };
        }
        return;
    }

    // Work in local coordinates: s runs horizontally from `a` toward `b`.
    // A (near) vertical rope is widened to 0.5px so the math stays finite;
    // it then draws as a thin "V" hanging down.
    let w = dx.abs().max(0.5);
    let sgn = if dx < 0.0 { -1.0 } else { 1.0 };

    // Solve  sqrt(L^2 - dy^2) = 2a * sinh(w / 2a)  for `a`.
    // With u = w / 2a this is sinh(u) / u = r, which is increasing in u.
    let r = (length * length - dy * dy).max(0.0).sqrt() / w;
    let u = solve_sinhc(r);
    let a_par = w / (2.0 * u);

    // Horizontal position of the lowest point, then the vertical offset.
    let x0 = w / 2.0 + a_par * (dy / (2.0 * a_par * (w / (2.0 * a_par)).sinh())).asinh();
    let c = a.y + a_par * (x0 / a_par).cosh();

    for (i, p) in out.iter_mut().enumerate() {
        let s = w * i as f64 / (n - 1) as f64;
        *p = Point {
            x: a.x + sgn * s,
            y: c - a_par * ((s - x0) / a_par).cosh(),
        };
    }
}

/// Solve sinh(u)/u = r for u > 0 (r > 1) by bisection.
fn solve_sinhc(r: f64) -> f64 {
    let f = |u: f64| u.sinh() / u;
    let mut lo = 0.0_f64;
    let mut hi = 1.0_f64;
    // sinh overflows near 710, so cap the search.
    while f(hi) < r && hi < 700.0 {
        lo = hi;
        hi *= 2.0;
    }
    hi = hi.min(700.0);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if mid <= 0.0 {
            break;
        }
        if f(mid) < r {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (0.5 * (lo + hi)).max(1e-9)
}

const ID_TOGGLE: i32 = 1;
const ID_QUIT: i32 = 2;
const VK_L: u32 = 0x4C;
const VK_Q: u32 = 0x51;
/// Set with the LAZY_DEBUG environment variable: logs button events to stderr.
static DEBUG: AtomicBool = AtomicBool::new(false);
/// Radius (px) of the hole around the cursor that the overlay never covers.
const HOLE: i32 = 6;
const FRAME_MS: u32 = 16; // overlay redraw rate (~60 Hz); the hook itself runs at the mouse rate

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
    last: Option<Instant>,
}

const OFF: Engine = Engine {
    enabled: false,
    dirty: false,
    brush: LazyBrush::new(0.0, Point { x: 0.0, y: 0.0 }),
    pointer: Point { x: 0.0, y: 0.0 },
    min: Point { x: 0.0, y: 0.0 },
    max: Point { x: 0.0, y: 0.0 },
    last: None,
};

const MAX_RADIUS: f64 = 300.0;

#[derive(Clone, Copy)]
struct Settings {
    radius: f64,
    friction: f64,
    spring: Spring,
}

impl Settings {
    fn brush(&self, at: Point) -> LazyBrush {
        LazyBrush::new(self.radius, at)
            .with_friction(self.friction)
            .with_spring(self.spring)
    }
}

#[derive(Clone, Copy, PartialEq, Default)]
struct Update {
    radius: Option<f64>,
    friction: Option<f64>,
    stiffness: Option<f64>,
    damping: Option<f64>,
    stretch: Option<f64>,
}

impl Update {
    const NONE: Update = Update {
        radius: None,
        friction: None,
        stiffness: None,
        damping: None,
        stretch: None,
    };

    fn merge(&mut self, o: Update) {
        self.radius = o.radius.or(self.radius);
        self.friction = o.friction.or(self.friction);
        self.stiffness = o.stiffness.or(self.stiffness);
        self.damping = o.damping.or(self.damping);
        self.stretch = o.stretch.or(self.stretch);
    }
}

static PENDING: Mutex<Update> = Mutex::new(Update::NONE);

thread_local! {
    static SETTINGS: Cell<Settings> = const {
        Cell::new(Settings { radius: 40.0, friction: 0.0, spring: Spring::DEFAULT })
    };
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
    bmp: HBITMAP, // <- add
    bits: *mut u32,
    renderer: Renderer,
}

impl Overlay {
    unsafe fn new(radius: f64, hinst: HINSTANCE) -> windows::core::Result<Self> {
        let renderer = Renderer::new(radius).ok_or(windows::core::Error::from_hresult(
            windows::core::HRESULT(0x8007000E_u32 as i32), // E_OUTOFMEMORY
        ))?;
        let side = renderer.side() as i32;

        unsafe {
            let class = w!("LazyMouseOverlay");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: hinst,
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);

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
            SelectObject(mem, bmp.into());

            Ok(Self {
                hwnd,
                screen,
                mem,
                bmp,
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
        let pixels = self.renderer.pixels();
        // SAFETY: `bits` points to a side*side u32 DIB section that lives for the whole process.
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, (side * side) as usize) };
        for (d, p) in dst.iter_mut().zip(pixels) {
            // premultiplied BGRA, as UpdateLayeredWindow(ULW_ALPHA, AC_SRC_ALPHA) expects
            *d = u32::from(p.alpha()) << 24
                | u32::from(p.red()) << 16
                | u32::from(p.green()) << 8
                | u32::from(p.blue());
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
            let _ = DeleteDC(self.mem);
            let _ = DeleteObject(self.bmp.into());
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
            OVERLAY.set(Some(Overlay::new(s.radius, hinst)?));
        }

        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), Some(hinst), 0)?;
        SetTimer(None, 0, FRAME_MS, None); // WM_TIMER arrives in the loop below

        println!(
            "lazy-mouse: radius {}px, friction {}, stiffness {}, damping {}, stretch {} | Ctrl+Alt+L toggle | Ctrl+Alt+Q quit",
            s.radius, s.friction, s.spring.stiffness, s.spring.damping, s.spring.stretch_ref
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
fn spawn_console(main_thread: u32) {
    println!("commands (e.g. \"r 60 s 250 d 18\"):");
    println!("  radius|r    <px>  {RADIUS_MIN} .. {RADIUS_MAX}   lazy area size");
    println!("  friction|f  <n>   {FRICTION_MIN} .. {FRICTION_MAX}   1 = brush never moves");
    println!("  stiffness|s <n>   {STIFFNESS_MIN} .. {STIFFNESS_MAX}   higher = snappier pull");
    println!(
        "  damping|d   <n>   {DAMPING_MIN} .. {DAMPING_MAX}   lower = builds speed more freely"
    );
    println!("  stretch|x   <px>  {STRETCH_MIN} .. {STRETCH_MAX}   lower = more nonlinear");
    println!("  quit|q");
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            let mut it = line.split_whitespace();
            let mut u = Update::NONE;
            while let Some(word) = it.next() {
                let num = |it: &mut std::str::SplitWhitespace| {
                    it.next()
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|v| v.is_finite())
                };
                match word {
                    "radius" | "r" => {
                        u.radius = num(&mut it).map(|v| v.clamp(RADIUS_MIN, RADIUS_MAX))
                    }
                    "friction" | "f" => {
                        u.friction = num(&mut it).map(|v| v.clamp(FRICTION_MIN, FRICTION_MAX))
                    }
                    "stiffness" | "s" => {
                        u.stiffness = num(&mut it).map(|v| v.clamp(STIFFNESS_MIN, STIFFNESS_MAX))
                    }
                    "damping" | "d" => {
                        u.damping = num(&mut it).map(|v| v.clamp(DAMPING_MIN, DAMPING_MAX))
                    }
                    "stretch" | "x" => {
                        u.stretch = num(&mut it).map(|v| v.clamp(STRETCH_MIN, STRETCH_MAX))
                    }

                    "quit" | "q" => unsafe {
                        let _ = PostThreadMessageW(main_thread, WM_QUIT, WPARAM(0), LPARAM(0));
                        return;
                    },
                    _ => println!("unknown: {word}"),
                }
            }
            if u != Update::NONE {
                PENDING.lock().unwrap().merge(u);
                unsafe {
                    let _ = PostThreadMessageW(main_thread, WM_APP, WPARAM(0), LPARAM(0));
                }
            }
        }
    });
}

fn apply_pending() {
    let u = std::mem::take(&mut *PENDING.lock().unwrap());
    let mut s = SETTINGS.get();
    let radius_changed = u.radius.map_or(false, |r| r != s.radius);
    if let Some(v) = u.radius {
        s.radius = v;
    }
    if let Some(v) = u.friction {
        s.friction = v;
    }
    if let Some(v) = u.stiffness {
        s.spring.stiffness = v;
    }
    if let Some(v) = u.damping {
        s.spring.damping = v;
    }
    if let Some(v) = u.stretch {
        s.spring.stretch_ref = v;
    }
    s.spring = s.spring.clamped();

    if radius_changed && OVERLAY.with_borrow(|o| o.is_some()) {
        let enabled = ENGINE.get().enabled;
        let built =
            unsafe { GetModuleHandleW(None).and_then(|h| Overlay::new(s.radius, HINSTANCE(h.0))) };
        match built {
            Ok(new) => {
                new.set_visible(enabled);
                OVERLAY.with_borrow_mut(|o| *o = Some(new));
            }
            Err(err) => {
                eprintln!("lazy-mouse: could not resize overlay: {err}");
                return;
            }
        }
    }
    SETTINGS.set(s);

    let mut e = ENGINE.get();
    e.brush.set_radius(s.radius);
    e.brush.set_friction(s.friction);
    e.brush.set_spring(s.spring);
    if e.enabled {
        e.dirty = true;
    }
    ENGINE.set(e);
    println!(
        "lazy-mouse: radius {}px, friction {}, stiffness {}, damping {}, stretch {}",
        s.radius, s.friction, s.spring.stiffness, s.spring.damping, s.spring.stretch_ref
    );
}
fn frame() {
    let mut e = ENGINE.get();
    if !e.enabled {
        return;
    }
    if advance(&mut e) {
        e.dirty = true;
    }
    if !e.dirty {
        ENGINE.set(e);
        return;
    }
    e.dirty = false;
    ENGINE.set(e);
    OVERLAY.with_borrow_mut(|o| {
        if let Some(o) = o {
            o.draw(&e);
        }
    });
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
/// Advance the brush to now and move the real cursor. Returns true if still animating.
fn advance(e: &mut Engine) -> bool {
    let now = Instant::now();
    let dt = e.last.replace(now).map_or(0.0, |t| (now - t).as_secs_f64());
    let moving = e.brush.step(e.pointer, dt);
    if moving {
        let b = e.brush.brush();
        let x = b.x.round().clamp(e.min.x, e.max.x) as i32;
        let y = b.y.round().clamp(e.min.y, e.max.y) as i32;
        let _ = unsafe { SetCursorPos(x, y) };
    }
    moving
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

    advance(&mut e);
    e.dirty = true; // the rope changes whenever the pointer moves
    ENGINE.set(e);
    true
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub stiffness: f64,   // higher = snappier pull
    pub damping: f64,     // lower = builds speed more freely
    pub stretch_ref: f64, // lower = more nonlinear (very large = linear spring)
}

impl Spring {
    pub const DEFAULT: Spring = Spring {
        stiffness: 180.0,
        damping: 22.0,
        stretch_ref: 100.0,
    };

    /// Keeps the fixed-step integration stable.
    pub fn clamped(self) -> Self {
        Spring {
            stiffness: self.stiffness.clamp(STIFFNESS_MIN, STIFFNESS_MAX),
            damping: self.damping.clamp(DAMPING_MIN, DAMPING_MAX),
            stretch_ref: self.stretch_ref.clamp(STRETCH_MIN, STRETCH_MAX),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct LazyBrush {
    radius: f64,
    /// 0 = brush closes the whole gap, 1 = brush never moves.
    friction: f64,
    brush: Point,
    vel: Point,
    spring: Spring,
}

impl LazyBrush {
    pub const fn new(radius: f64, initial: Point) -> Self {
        Self {
            radius,
            friction: 0.0,
            brush: initial,
            vel: Point { x: 0.0, y: 0.0 },
            spring: Spring::DEFAULT,
        }
    }

    /// Fraction (0..=1) of the gap the brush does NOT close per update.
    pub fn with_friction(mut self, friction: f64) -> Self {
        self.friction = if friction < 0.0 {
            0.0
        } else if friction > 1.0 {
            1.0
        } else {
            friction
        };
        self
    }
    /// Change the lazy radius on the fly (the brush position is kept).
    pub fn set_radius(&mut self, radius: f64) {
        self.radius = radius.max(0.0);
    }

    /// Change the friction on the fly (clamped to 0..=1).
    pub fn set_friction(&mut self, friction: f64) {
        *self = self.with_friction(friction);
    }
    pub fn with_spring(mut self, spring: Spring) -> Self {
        self.spring = spring.clamped();
        self
    }

    pub fn set_spring(&mut self, spring: Spring) {
        self.spring = spring.clamped();
    }

    pub const fn radius(&self) -> f64 {
        self.radius
    }

    pub const fn brush(&self) -> Point {
        self.brush
    }

    /// Feed a new pointer position. Returns `true` if the brush moved.
    #[inline]
    /// Rubber-band follow. Outside the lazy radius the pointer pulls the brush with a
    /// force that grows faster than linearly with the stretch. As soon as the pointer
    /// is inside the radius the brush stops dead. Returns `true` if the brush may have moved.
    pub fn step(&mut self, pointer: Point, dt: f64) -> bool {
        let stop = Point { x: 0.0, y: 0.0 };
        if self.friction >= 1.0 {
            self.vel = stop;
            return false;
        }
        let dt = dt.clamp(0.0, 0.05);
        let n = (dt / SUBSTEP).ceil() as u32;
        let h = if n > 0 { dt / n as f64 } else { 0.0 };
        let Spring {
            stiffness,
            damping,
            stretch_ref,
        } = self.spring;
        let stiffness = stiffness * (1.0 - self.friction);

        for _ in 0..n {
            let dx = pointer.x - self.brush.x;
            let dy = pointer.y - self.brush.y;
            let dist = dx.hypot(dy);
            if dist <= self.radius {
                self.vel = stop; // re-entered the radius: stop instantly
                return false;
            }
            let excess = dist - self.radius;
            let f = stiffness * excess * (1.0 + excess / stretch_ref) / dist;
            self.vel.x += (dx * f - damping * self.vel.x) * h;
            self.vel.y += (dy * f - damping * self.vel.y) * h;

            self.brush.x += self.vel.x * h;
            self.brush.y += self.vel.y * h;
        }

        // the last substep may have carried it into the radius
        let d = (pointer.x - self.brush.x).hypot(pointer.y - self.brush.y);
        if d <= self.radius {
            self.vel = stop;
            return false;
        }
        true
    }
}

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    // Usage: lazy-mouse [radius_px] [friction 0..1] [stiffness] [damping] [stretch_px]
    let arg = |i| {
        std::env::args()
            .nth(i)
            .and_then(|a| a.parse::<f64>().ok())
            .filter(|v| v.is_finite())
    };
    let d = Spring::DEFAULT;
    run(Settings {
        radius: arg(1).unwrap_or(40.0).clamp(RADIUS_MIN, RADIUS_MAX),
        friction: arg(2).unwrap_or(0.0).clamp(FRICTION_MIN, FRICTION_MAX),
        spring: Spring {
            stiffness: arg(3).unwrap_or(d.stiffness),
            damping: arg(4).unwrap_or(d.damping),
            stretch_ref: arg(5).unwrap_or(d.stretch_ref),
        }
        .clamped(),
    })
}
#[cfg(not(windows))]
fn main() {
    eprintln!("lazy-mouse only runs on Windows.");
}
