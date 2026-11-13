//! Dockodile für Windows: Tabs für alle Programme.
//!
//! Windows lets no process embed a foreign window, so a group is a stack: every member window
//! has the same rectangle, the active one lies on top, and a borderless tab bar owned by this
//! process sits directly above it. Nothing is hidden or parked off-screen, so a crash leaves
//! all windows reachable.
//!
//! Everything runs on one thread. WinEvent hooks are out-of-context and arrive through the
//! message loop, also inside menu loops. Functions therefore address groups by index or bar
//! handle and look them up again after every call that may pump messages or dissolve a group.
#![windows_subsystem = "windows"]
#![allow(clippy::missing_safety_doc)]

mod store;

use std::time::{Duration, Instant};
use std::{mem, ptr};
use store::{PTab, Package, Store};
use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Accessibility::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const VERSION: &str = "0.4.1";
const WM_TRAY: u32 = WM_APP + 1;
const WM_URL: u32 = WM_APP + 2;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// A background check for updates finished; the answer waits in UPDATE_RESULT.
const WM_UPDATE: u32 = WM_APP + 3;
/// One screen refresh has passed while a tab hangs on the cursor, see start_float.
const WM_FRAME: u32 = WM_APP + 4;
static FLOATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static FRAME_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const TIMER_UPDATE: usize = 4;
/// The user clicked the balloon of the tray icon.
const NIN_BALLOONUSERCLICK: u32 = WM_USER + 5;
const EM_SETCUEBANNER: u32 = 0x1501;
/// Hotkey id used only to ask Windows whether a combination is still free.
const HOTKEY_PROBE: i32 = 900;
const TIMER_TICK: usize = 1;
const TIMER_FLOAT: usize = 2;
const TIMER_MOVE: usize = 3;
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

static mut LOG: Option<std::fs::File> = None;

/// With `--log`, writes what Dockodile sees and does to dockodile.log next to the program.
/// Meant for bug reports; nothing is written otherwise.
macro_rules! log {
    ($($arg:tt)*) => {
        #[allow(static_mut_refs)]
        #[allow(unused_unsafe)]
        unsafe {
            if let Some(f) = LOG.as_mut() {
                use std::io::Write;
                let _ = writeln!(f, $($arg)*);
            }
        }
    };
}

struct Tab {
    hwnd: HWND,
    name: Vec<u16>,
    icon: HICON,
    /// Count from the window title, see store::unread.
    unread: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Tab(usize),
    Close(usize),
    Plus,
    Minimize,
    Maximize,
    Dissolve,
    Empty,
}

#[derive(Clone, Copy)]
enum Drag {
    None,
    /// Button went down on a tab; becomes a tab drag after a few pixels.
    Pending(usize, POINT),
    Tab(usize),
    /// Moving the whole group; the offset of the cursor from the frame's top left.
    Move(i32, i32),
    /// Resizing at the bar's top edge or one of its top corners.
    Resize { left: bool, right: bool, start: RECT, from: POINT },
}

impl Drag {
    /// The bar is moving or resizing the group; window events must not answer back.
    fn whole(&self) -> bool {
        matches!(self, Drag::Move(..) | Drag::Resize { .. })
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Lay {
    /// Bar and active window.
    Active,
    /// Bar and every member.
    All,
    /// Bar and every member except the active window, which the user is dragging.
    Others,
}

struct Group {
    bar: HWND,
    tabs: Vec<Tab>,
    active: usize,
    /// Bar plus window, in screen pixels of the visible frame.
    frame: RECT,
    /// Frame before a double click filled the screen.
    restore: Option<RECT>,
    drag: Drag,
    /// Button under the mouse, drawn lighter.
    hover: Hit,
    minimized: bool,
    /// Name of the package this group was opened from or saved as.
    package: Option<String>,
}

/// What the small window with the text field is doing.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Closed,
    /// Add a window or start a program for the group of this bar.
    Add(HWND),
    /// Save the group of this bar as a package.
    Save(HWND),
    /// Waiting for the new shortcut of this package.
    Hotkey(usize),
}

#[derive(Clone)]
enum Entry {
    Window(HWND),
    Program(String),
    SavePrompt,
    SaveNew,
    SaveOver(usize),
    Info,
}

/// A program was started from the plus; its first new window joins the group.
struct Launch {
    bar: HWND,
    known: Vec<HWND>,
    deadline: Instant,
}

struct Slot {
    exe: String,
    title: String,
    hwnd: HWND,
}

/// A package is being opened; windows join as their programs come up.
struct Opening {
    name: String,
    frame: RECT,
    slots: Vec<Slot>,
    bar: HWND,
    deadline: Instant,
}

#[derive(Clone, Copy, PartialEq)]
enum Target {
    None,
    Bar(HWND),
    Window(HWND),
}

struct App {
    inst: HINSTANCE,
    msg: HWND,
    hint: HWND,
    groups: Vec<Group>,
    /// Window the user is moving by its own title bar, and its size when the move began.
    moving: HWND,
    moving_size: (i32, i32),
    /// The mouse held the window away from its edges when the move began: a move, not a resize.
    moving_by_title: bool,
    /// When the picker opened, see its WM_ACTIVATE.
    picker_at: Option<Instant>,
    target: Target,
    /// Tab that was pulled out and now follows the cursor until the button is released.
    float: HWND,
    float_off: (i32, i32),
    pid: u32,
    taskbar_created: u32,
    store: Store,
    /// Number of package hotkeys currently registered, ids 1..=hotkeys.
    hotkeys: usize,
    mode: Mode,
    picker: HWND,
    edit: HWND,
    list: HWND,
    picker_font: HFONT,
    entries: Vec<Entry>,
    /// Start menu entries, name and path of the shortcut.
    programs: Vec<(String, String)>,
    launch: Option<Launch>,
    opening: Option<Opening>,
    url: String,
    /// Newest version on the server while it is newer than this one.
    available: Option<String>,
    /// A check or download is running.
    updating: bool,
    /// The last balloon announced an update; clicking it offers the update.
    balloon_update: bool,
}

static mut APP: *mut App = ptr::null_mut();
static mut EN: bool = false;

/// What a background thread found out about an update, handed to the UI thread.
enum UpdateResult {
    /// Newest version on the server, None if the server could not be asked.
    Version(Option<String>, bool),
    /// The new program file is written next to the old one, or the reason it is not.
    File(Result<std::path::PathBuf, String>),
}

static UPDATE_RESULT: std::sync::Mutex<Option<UpdateResult>> = std::sync::Mutex::new(None);

/// The interface text in the chosen language.
fn tr(de: &'static str, en: &'static str) -> &'static str {
    if unsafe { EN } { en } else { de }
}

fn lower(v: &[u16]) -> String {
    String::from_utf16_lossy(v).to_lowercase()
}

fn store_path() -> Option<std::path::PathBuf> {
    let dir = std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("Dockodile");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("dockodile.txt"))
}

fn save_store() {
    if let Some(p) = store_path() {
        if std::fs::write(&p, store::serialize(&app().store)).is_err() {
            log!("could not write {:?}", p);
        }
    }
}
fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

fn same(a: &RECT, b: &RECT) -> bool {
    a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
}

fn shifted(r: &RECT, dx: i32, dy: i32) -> RECT {
    RECT { left: r.left + dx, top: r.top + dy, right: r.right + dx, bottom: r.bottom + dy }
}

fn inside(r: &RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

// ---------------------------------------------------------------------------------------------
// Foreign windows
// ---------------------------------------------------------------------------------------------

unsafe fn dpi_of(h: HWND) -> i32 {
    let d = GetDpiForWindow(h) as i32;
    if d == 0 { 96 } else { d }
}

unsafe fn px(h: HWND, v: i32) -> i32 {
    v * dpi_of(h) / 96
}

/// Rows of tabs and tabs per row for a bar of this width. One row unless the user chose two
/// rows and the tabs would get narrower than is comfortable; then the first row stays as full
/// as it was at that point and the rest continue below.
unsafe fn rows_for(bar: HWND, width: i32, count: usize) -> (i32, i32) {
    let n = count.max(1) as i32;
    if !app().store.two_rows {
        return (1, n);
    }
    let (pad, btn, gap) = (px(bar, 6), px(bar, 30), px(bar, 4));
    let room = width - 2 * pad - 4 * btn - 3 * gap;
    let fit = (room / (px(bar, 150) + gap)).max(3);
    if n <= fit {
        (1, n)
    } else {
        (2, fit.max((n + 1) / 2))
    }
}

unsafe fn bar_h(bar: HWND) -> i32 {
    let rows = match find_bar(bar) {
        Some(gi) => {
            let g = &app().groups[gi];
            rows_for(bar, g.frame.right - g.frame.left, g.tabs.len()).0
        }
        None => 1,
    };
    px(bar, 36) + (rows - 1) * px(bar, 32)
}

/// The frame the user sees. GetWindowRect includes invisible resize borders on Windows 10/11.
unsafe fn vis(h: HWND) -> RECT {
    let mut r: RECT = mem::zeroed();
    let ok = DwmGetWindowAttribute(
        h,
        DWMWA_EXTENDED_FRAME_BOUNDS as u32,
        &mut r as *mut RECT as *mut _,
        mem::size_of::<RECT>() as u32,
    );
    if ok != 0 || r.right <= r.left {
        GetWindowRect(h, &mut r);
    }
    r
}

/// Puts the visible frame of a window at `r`, compensating the invisible borders.
unsafe fn set_vis(h: HWND, r: &RECT) {
    // Moving a window waits for its program; one that hangs would freeze every group.
    if IsHungAppWindow(h) != 0 {
        return;
    }
    if IsIconic(h) != 0 {
        ShowWindow(h, SW_SHOWNOACTIVATE);
    }
    if IsZoomed(h) != 0 {
        ShowWindow(h, SW_SHOWNOACTIVATE);
    }
    let mut wr: RECT = mem::zeroed();
    GetWindowRect(h, &mut wr);
    let vr = vis(h);
    let (l, t, rt, b) = (vr.left - wr.left, vr.top - wr.top, wr.right - vr.right, wr.bottom - vr.bottom);
    SetWindowPos(
        h,
        ptr::null_mut(),
        r.left - l,
        r.top - t,
        r.right - r.left + l + rt,
        r.bottom - r.top + t + b,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
}

unsafe fn work_area(r: &RECT) -> RECT {
    let mon = MonitorFromRect(r, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = mem::zeroed();
    mi.cbSize = mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(mon, &mut mi) != 0 { mi.rcWork } else { *r }
}

unsafe fn title_of(h: HWND) -> Vec<u16> {
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
    buf[..n].to_vec()
}

unsafe fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = GetClassNameW(h, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

unsafe fn exe_of(h: HWND) -> Vec<u16> {
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, &mut pid);
    let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if p.is_null() {
        return Vec::new();
    }
    let mut buf = [0u16; 1024];
    let mut n = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(p, 0, buf.as_mut_ptr(), &mut n);
    CloseHandle(p);
    if ok == 0 { Vec::new() } else { buf[..n as usize].to_vec() }
}

/// "FileDescription" of the program, as the taskbar shows it. Empty if the file has none.
unsafe fn description_of(exe: &[u16]) -> Vec<u16> {
    let mut path = exe.to_vec();
    path.push(0);
    let mut handle = 0u32;
    let size = GetFileVersionInfoSizeW(path.as_ptr(), &mut handle);
    if size == 0 {
        return Vec::new();
    }
    let mut data = vec![0u8; size as usize];
    if GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr() as *mut _) == 0 {
        return Vec::new();
    }
    let mut p: *mut core::ffi::c_void = ptr::null_mut();
    let mut len = 0u32;
    let q = w("\\VarFileInfo\\Translation");
    if VerQueryValueW(data.as_ptr() as *const _, q.as_ptr(), &mut p, &mut len) == 0 || len < 4 {
        return Vec::new();
    }
    let tr = p as *const u16;
    let key = w(&format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", *tr, *tr.add(1)));
    if VerQueryValueW(data.as_ptr() as *const _, key.as_ptr(), &mut p, &mut len) == 0 || len == 0 {
        return Vec::new();
    }
    let s = std::slice::from_raw_parts(p as *const u16, len as usize);
    s.iter().copied().take_while(|c| *c != 0).collect()
}

unsafe fn name_of(h: HWND) -> Vec<u16> {
    let exe = exe_of(h);
    let lower = String::from_utf16_lossy(&exe).to_lowercase();
    // These hosts show other programs' windows; their own description says nothing.
    let host = lower.ends_with("\\applicationframehost.exe") || lower.ends_with("\\explorer.exe");
    let mut name = if host || exe.is_empty() { Vec::new() } else { description_of(&exe) };
    if name.is_empty() {
        name = title_of(h);
    }
    if name.is_empty() {
        name = tr("Fenster", "Window").encode_utf16().collect();
    }
    name
}

unsafe fn icon_of(h: HWND) -> HICON {
    let mut r = 0usize;
    SendMessageTimeoutW(h, WM_GETICON, ICON_SMALL2 as usize, 0, SMTO_ABORTIFHUNG, 100, &mut r);
    if r == 0 {
        SendMessageTimeoutW(h, WM_GETICON, ICON_BIG as usize, 0, SMTO_ABORTIFHUNG, 100, &mut r);
    }
    if r == 0 {
        r = GetClassLongPtrW(h, GCLP_HICONSM);
    }
    if r == 0 {
        r = GetClassLongPtrW(h, GCLP_HICON);
    }
    r as HICON
}

/// A normal top-level program window that can become a tab.
unsafe fn eligible(h: HWND, allow_minimized: bool) -> bool {
    if IsWindowVisible(h) == 0 || (!allow_minimized && IsIconic(h) != 0) {
        return false;
    }
    if !GetWindow(h, GW_OWNER).is_null() {
        return false;
    }
    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOOLWINDOW != 0 {
        return false;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, &mut pid);
    // GetWindowTextW reads a foreign title without sending a message, so a hung program
    // cannot block this check.
    let mut probe = [0u16; 4];
    if pid == app().pid || GetWindowTextW(h, probe.as_mut_ptr(), probe.len() as i32) <= 0 {
        return false;
    }
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(h, DWMWA_CLOAKED as u32, &mut cloaked as *mut u32 as *mut _, 4);
    if cloaked != 0 {
        return false;
    }
    !matches!(
        class_of(h).as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Windows.UI.Core.CoreWindow"
    )
}

/// Brings a foreign window to the front and gives it the keyboard. Windows refuses this to
/// processes it does not consider active; attaching to the foreground thread's input lifts
/// that for the one call.
unsafe fn focus(h: HWND) {
    if GetForegroundWindow() == h {
        return;
    }
    SetForegroundWindow(h);
    if GetForegroundWindow() == h {
        return;
    }
    let me = GetCurrentThreadId();
    let other = GetWindowThreadProcessId(GetForegroundWindow(), ptr::null_mut());
    if other != 0 && other != me {
        AttachThreadInput(me, other, 1);
        BringWindowToTop(h);
        SetForegroundWindow(h);
        AttachThreadInput(me, other, 0);
    }
    log!("focus {:?} reached={}", h, GetForegroundWindow() == h);
}

fn find_tab(h: HWND) -> Option<(usize, usize)> {
    for (gi, g) in app().groups.iter().enumerate() {
        if let Some(ti) = g.tabs.iter().position(|t| t.hwnd == h) {
            return Some((gi, ti));
        }
    }
    None
}

fn find_bar(bar: HWND) -> Option<usize> {
    app().groups.iter().position(|g| g.bar == bar)
}

// ---------------------------------------------------------------------------------------------
// Groups
// ---------------------------------------------------------------------------------------------

unsafe fn content(g: &Group) -> RECT {
    RECT { top: g.frame.top + bar_h(g.bar), ..g.frame }
}

/// Moves the bar and the chosen members to the group's frame.
unsafe fn layout(gi: usize, which: Lay) {
    let g = &app().groups[gi];
    if g.minimized {
        return;
    }
    let (bar, f, active) = (g.bar, g.frame, g.active);
    let hwnds: Vec<HWND> = g.tabs.iter().map(|t| t.hwnd).collect();
    let bh = bar_h(bar);
    SetWindowPos(bar, ptr::null_mut(), f.left, f.top, f.right - f.left, bh, SWP_NOZORDER | SWP_NOACTIVATE);
    let c = RECT { top: f.top + bh, ..f };
    for (i, h) in hwnds.iter().enumerate() {
        let go = match which {
            Lay::Active => i == active,
            Lay::All => true,
            Lay::Others => i != active,
        };
        if go {
            set_vis(*h, &c);
        }
    }
    InvalidateRect(bar, ptr::null(), 0);
}

/// Keeps the bar directly above the active window in the z-order, without taking focus.
unsafe fn raise_bar(gi: usize) {
    let g = &app().groups[gi];
    if g.minimized || g.tabs.is_empty() {
        return;
    }
    let (bar, active) = (g.bar, g.tabs[g.active].hwnd);
    let prev = GetWindow(active, GW_HWNDPREV);
    if prev == bar {
        return;
    }
    // Inserting behind an always-on-top window would make the bar always-on-top as well.
    let topmost = !prev.is_null() && GetWindowLongW(prev, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0;
    let after = if topmost { HWND_TOP } else { prev };
    SetWindowPos(bar, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
}

unsafe fn select(gi: usize, idx: usize, activate: bool) {
    let g = &mut app().groups[gi];
    if idx >= g.tabs.len() {
        return;
    }
    g.active = idx;
    let (h, bar) = (g.tabs[idx].hwnd, g.bar);
    layout(gi, Lay::Active);
    SetWindowPos(h, ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    if activate {
        focus(h);
    }
    if let Some(gi) = find_bar(bar) {
        raise_bar(gi);
    }
    InvalidateRect(bar, ptr::null(), 0);
}

/// The frame a group gets when it forms around `h` or when `h` was moved from outside.
unsafe fn frame_around(h: HWND, bh: i32) -> RECT {
    let vr = vis(h);
    if IsZoomed(h) != 0 {
        return work_area(&vr);
    }
    let mut f = RECT { top: vr.top - bh, ..vr };
    let wa = work_area(&f);
    if f.top < wa.top {
        f.top = wa.top;
    }
    if f.bottom - f.top < bh + 120 {
        f.bottom = f.top + bh + 120;
    }
    f
}

/// The active window was moved or resized by the user or the system: the group follows.
unsafe fn adopt(gi: usize) {
    let g = &mut app().groups[gi];
    if g.minimized || g.tabs.is_empty() {
        return;
    }
    let h = g.tabs[g.active].hwnd;
    if IsIconic(h) != 0 {
        return;
    }
    let f = frame_around(h, bar_h(g.bar));
    if IsZoomed(h) != 0 && g.restore.is_none() {
        g.restore = Some(g.frame);
    }
    g.frame = f;
    layout(gi, Lay::All);
    raise_bar(gi);
}

unsafe fn make_tab(h: HWND) -> Tab {
    let unread = store::unread(&String::from_utf16_lossy(&title_of(h))).unwrap_or(0);
    Tab { hwnd: h, name: name_of(h), icon: icon_of(h), unread }
}

unsafe fn create_group(first: HWND, second: HWND) {
    create_group_from(&[first, second], None, 1, None);
}

/// Forms a group from at least two windows, around the first one or at `frame`.
/// Returns the new bar.
unsafe fn create_group_from(hwnds: &[HWND], frame: Option<RECT>, active: usize, package: Option<String>) -> Option<HWND> {
    if hwnds.len() < 2 {
        return None;
    }
    let first = hwnds[0];
    let a = app();
    let class = w("DockodileBar");
    let bar = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class.as_ptr(),
        w("Dockodile").as_ptr(),
        WS_POPUP,
        0,
        0,
        10,
        10,
        ptr::null_mut(),
        ptr::null_mut(),
        a.inst,
        ptr::null(),
    );
    if bar.is_null() {
        return None;
    }
    // Windows 11: small rounded corners. Fails silently on Windows 10.
    let round = 3u32;
    DwmSetWindowAttribute(bar, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &round as *const u32 as *const _, 4);
    // On the right monitor first, so that the bar height uses that monitor's scaling.
    let vr = frame.unwrap_or_else(|| vis(first));
    SetWindowPos(bar, ptr::null_mut(), vr.left, vr.top, 10, 10, SWP_NOZORDER | SWP_NOACTIVATE);
    let frame = frame.unwrap_or_else(|| frame_around(first, bar_h(bar)));
    let mut tabs = Vec::new();
    for h in hwnds {
        tabs.push(make_tab(*h));
    }
    a.groups.push(Group {
        bar,
        tabs,
        active: active.min(hwnds.len() - 1),
        frame,
        restore: None,
        drag: Drag::None,
        hover: Hit::Empty,
        minimized: false,
        package,
    });
    let gi = a.groups.len() - 1;
    log!("group created, frame {},{} {}x{}", frame.left, frame.top, frame.right - frame.left, frame.bottom - frame.top);
    layout(gi, Lay::All);
    ShowWindow(bar, SW_SHOWNOACTIVATE);
    select(gi, active.min(hwnds.len() - 1), true);
    Some(bar)
}

unsafe fn add_tab(gi: usize, h: HWND) {
    insert_tab(gi, h, None);
}

/// Adds a window to a group, at the end or at `pos`, and shows it.
unsafe fn insert_tab(gi: usize, h: HWND, pos: Option<usize>) {
    if find_tab(h).is_some() {
        return;
    }
    let tab = make_tab(h);
    let g = &mut app().groups[gi];
    let idx = pos.unwrap_or(g.tabs.len()).min(g.tabs.len());
    g.tabs.insert(idx, tab);
    let c = content(g);
    set_vis(h, &c);
    select(gi, idx, true);
}

#[derive(Clone, Copy, PartialEq)]
enum Leave {
    /// The window is gone or hidden.
    Gone,
    /// Released next to the group.
    Aside,
    /// Pulled out with the mouse; it follows the cursor.
    Float,
}

unsafe fn remove_tab(gi: usize, idx: usize, how: Leave) {
    let g = &mut app().groups[gi];
    if idx >= g.tabs.len() {
        return;
    }
    let bar = g.bar;
    let was_active = g.active == idx;
    let tab = g.tabs.remove(idx);
    if g.active > idx || g.active >= g.tabs.len() {
        g.active = g.active.saturating_sub(1);
    }
    let c = content(g);
    let step = px(bar, 40);
    let (left, active) = (g.tabs.len(), g.active);
    if how == Leave::Aside {
        set_vis(tab.hwnd, &shifted(&c, step, step));
    }
    if left <= 1 {
        dissolve(gi);
    } else {
        // A closed window hands the keyboard to its neighbour; a background tab that
        // disappears must not steal it.
        select(gi, active, how == Leave::Gone && was_active);
    }
    if how == Leave::Aside {
        focus(tab.hwnd);
    }
}

/// Ends a group. The windows stay where they are, fanned out so that each one is reachable.
unsafe fn dissolve(gi: usize) {
    let g = app().groups.remove(gi);
    log!("group dissolved, {} tabs", g.tabs.len());
    let step = px(g.bar, 32);
    DestroyWindow(g.bar);
    let mut k = 0;
    for (i, t) in g.tabs.iter().enumerate() {
        if i == g.active || IsWindow(t.hwnd) == 0 {
            continue;
        }
        k += 1;
        set_vis(t.hwnd, &shifted(&g.frame, k * step, k * step));
    }
    if let Some(t) = g.tabs.get(g.active) {
        if IsWindow(t.hwnd) != 0 {
            set_vis(t.hwnd, &g.frame);
            SetWindowPos(t.hwnd, ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

unsafe fn dissolve_all() {
    while !app().groups.is_empty() {
        dissolve(app().groups.len() - 1);
    }
}

// ---------------------------------------------------------------------------------------------
// Dropping a window onto another one
// ---------------------------------------------------------------------------------------------

/// What lies under the cursor while `moving` is dragged: a tab bar, the top strip of a free
/// window, or nothing.
/// Windows of one program: browsers, Explorer, Terminal and Notepad have tabs of their own in the
/// title bar, and dragging one of their tabs onto another of their windows must stay theirs.
/// Store apps all run inside ApplicationFrameHost, so that process says nothing.
unsafe fn same_program(a: HWND, b: HWND) -> bool {
    let (mut pa, mut pb) = (0u32, 0u32);
    GetWindowThreadProcessId(a, &mut pa);
    GetWindowThreadProcessId(b, &mut pb);
    pa != 0 && pa == pb && !lower(&exe_of(a)).ends_with("\\applicationframehost.exe")
}

/// `own_drag`: the user carries the window, so the program may be doing its own tab drag.
/// Holding Shift docks anyway; a tab bar of Dockodile always takes the drop.
unsafe fn find_target(pt: POINT, moving: HWND, own_drag: bool) -> Target {
    for g in app().groups.iter() {
        let mut r: RECT = mem::zeroed();
        GetWindowRect(g.bar, &mut r);
        if !g.minimized && inside(&r, pt) {
            return Target::Bar(g.bar);
        }
    }
    let mut h = GetTopWindow(ptr::null_mut());
    while !h.is_null() {
        if h != moving && eligible(h, false) {
            let vr = vis(h);
            if inside(&vr, pt) {
                if find_tab(h).is_some() || pt.y >= vr.top + px(h, 44) {
                    return Target::None;
                }
                if own_drag && same_program(h, moving) && GetKeyState(VK_SHIFT as i32) >= 0 {
                    return Target::None;
                }
                return Target::Window(h);
            }
        }
        h = GetWindow(h, GW_HWNDNEXT);
    }
    Target::None
}

fn describe(t: Target) -> String {
    match t {
        Target::None => "none".into(),
        Target::Bar(h) => format!("bar {:?}", h),
        Target::Window(h) => format!("window {:?} \"{}\"", h, String::from_utf16_lossy(&unsafe { title_of(h) })),
    }
}

unsafe fn show_hint(target: Target) {
    let a = app();
    if a.target == target {
        return;
    }
    a.target = target;
    log!("target {}", describe(target));
    let r = match target {
        Target::None => {
            ShowWindow(a.hint, SW_HIDE);
            return;
        }
        Target::Bar(bar) => {
            let mut r: RECT = mem::zeroed();
            GetWindowRect(bar, &mut r);
            r
        }
        Target::Window(h) => {
            let vr = vis(h);
            RECT { bottom: vr.top + px(h, 44), ..vr }
        }
    };
    SetWindowPos(
        a.hint,
        HWND_TOPMOST,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    InvalidateRect(a.hint, ptr::null(), 0);
}

unsafe fn dock(target: Target, h: HWND) {
    if IsWindow(h) == 0 || find_tab(h).is_some() {
        return;
    }
    match target {
        Target::None => {}
        Target::Bar(bar) => {
            if let Some(gi) = find_bar(bar) {
                add_tab(gi, h);
            }
        }
        Target::Window(first) => {
            if IsWindow(first) != 0 && first != h && find_tab(first).is_none() {
                create_group(first, h);
            }
        }
    }
}

/// A tab was pulled out of its bar. It follows the cursor until the mouse button is released.
/// Not the bar drives this, so it survives the bar disappearing with its group, but a thread that
/// waits for each screen refresh: the window then moves once per frame. A timer alone ticks only
/// every 16 or 31 ms and out of step with the screen, which looks like 30 frames a second. The
/// timer stays as a slow net in case the frames stop coming.
unsafe fn start_float(h: HWND) {
    use std::sync::atomic::Ordering::SeqCst;
    let a = app();
    let vr = vis(h);
    a.float = h;
    a.float_off = ((vr.right - vr.left) / 2, px(h, 14));
    SetTimer(a.msg, TIMER_FLOAT, 100, None);
    FRAME_PENDING.store(false, SeqCst);
    if !FLOATING.swap(true, SeqCst) {
        let msg = a.msg as isize;
        std::thread::spawn(move || {
            while FLOATING.load(SeqCst) {
                let before = Instant::now();
                let ok = unsafe { DwmFlush() } >= 0;
                // Without a compositor to wait for, keep a steady pace instead of spinning.
                if !ok || before.elapsed() < Duration::from_millis(2) {
                    std::thread::sleep(Duration::from_millis(7));
                }
                if !FRAME_PENDING.swap(true, SeqCst) {
                    unsafe { PostMessageW(msg as HWND, WM_FRAME, 0, 0) };
                }
            }
        });
    }
    float_tick();
}

unsafe fn float_tick() {
    let a = app();
    let h = a.float;
    if h.is_null() {
        KillTimer(a.msg, TIMER_FLOAT);
        FLOATING.store(false, std::sync::atomic::Ordering::SeqCst);
        return;
    }
    if IsWindow(h) == 0 || GetAsyncKeyState(VK_LBUTTON as i32) >= 0 {
        KillTimer(a.msg, TIMER_FLOAT);
        FLOATING.store(false, std::sync::atomic::Ordering::SeqCst);
        let target = a.target;
        a.float = ptr::null_mut();
        show_hint(Target::None);
        dock(target, h);
        return;
    }
    let mut pt: POINT = mem::zeroed();
    GetCursorPos(&mut pt);
    let vr = vis(h);
    let (x, y) = (pt.x - a.float_off.0, pt.y - a.float_off.1);
    if x != vr.left || y != vr.top {
        set_vis(h, &RECT { left: x, top: y, right: x + vr.right - vr.left, bottom: y + vr.bottom - vr.top });
    }
    show_hint(find_target(pt, h, false));
}

// ---------------------------------------------------------------------------------------------
// System events
// ---------------------------------------------------------------------------------------------

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if APP.is_null() || hwnd.is_null() || id_object != OBJID_WINDOW || id_child != 0 {
        return;
    }
    let a = app();
    match event {
        EVENT_SYSTEM_FOREGROUND => {
            if let Some((gi, ti)) = find_tab(hwnd) {
                if a.groups[gi].minimized {
                    return;
                }
                if a.groups[gi].active != ti {
                    select(gi, ti, false);
                } else {
                    raise_bar(gi);
                }
            }
        }
        EVENT_SYSTEM_MOVESIZESTART => begin_move(hwnd, false),
        EVENT_SYSTEM_MOVESIZEEND => {
            if a.moving == hwnd {
                end_move();
            }
        }
        EVENT_OBJECT_LOCATIONCHANGE => {
            // Programs that drag their own title bar never announce a move. A window that
            // moves while the mouse button holds its top edge is treated the same way.
            if a.moving.is_null() && a.float.is_null() && GetAsyncKeyState(VK_LBUTTON as i32) < 0 {
                let mut pt: POINT = mem::zeroed();
                GetCursorPos(&mut pt);
                let vr = vis(hwnd);
                let grip = RECT { bottom: vr.top + px(hwnd, 60), ..vr };
                let dragged_by_bar = find_tab(hwnd).is_some_and(|(gi, _)| a.groups[gi].drag.whole());
                if inside(&grip, pt) && !dragged_by_bar && GetForegroundWindow() == hwnd && eligible(hwnd, false) {
                    begin_move(hwnd, true);
                }
            }
            if let Some((gi, ti)) = find_tab(hwnd) {
                let g = &mut a.groups[gi];
                if g.active != ti || g.minimized || IsIconic(hwnd) != 0 || g.drag.whole() {
                    return;
                }
                let vr = vis(hwnd);
                if same(&vr, &content(g)) {
                    return;
                }
                if a.moving == hwnd {
                    // Live drag: the bar and the windows behind follow at once, so that
                    // nothing of the group stays behind on the screen.
                    g.frame = RECT { top: vr.top - bar_h(g.bar), ..vr };
                    layout(gi, Lay::Others);
                } else {
                    adopt(gi);
                }
            } else if a.moving == hwnd && a.float.is_null() {
                let mut pt: POINT = mem::zeroed();
                GetCursorPos(&mut pt);
                let target = if eligible(hwnd, false) { find_target(pt, hwnd, true) } else { Target::None };
                show_hint(target);
            }
        }
        EVENT_SYSTEM_MINIMIZESTART => {
            if let Some((gi, ti)) = find_tab(hwnd) {
                let g = &mut a.groups[gi];
                if g.active != ti || g.minimized {
                    return;
                }
                g.minimized = true;
                let bar = g.bar;
                let others: Vec<HWND> = g.tabs.iter().map(|t| t.hwnd).filter(|h| *h != hwnd).collect();
                ShowWindow(bar, SW_HIDE);
                for h in others {
                    ShowWindow(h, SW_SHOWMINNOACTIVE);
                }
            }
        }
        EVENT_SYSTEM_MINIMIZEEND => {
            if let Some((gi, ti)) = find_tab(hwnd) {
                let g = &mut a.groups[gi];
                if !g.minimized {
                    return;
                }
                g.minimized = false;
                g.active = ti;
                let bar = g.bar;
                let others: Vec<HWND> = g.tabs.iter().map(|t| t.hwnd).filter(|h| *h != hwnd).collect();
                for h in others {
                    ShowWindow(h, SW_SHOWNOACTIVATE);
                }
                ShowWindow(bar, SW_SHOWNOACTIVATE);
                if let Some(gi) = find_bar(bar) {
                    layout(gi, Lay::All);
                    select(gi, ti, false);
                }
            }
        }
        EVENT_OBJECT_DESTROY | EVENT_OBJECT_HIDE => {
            if a.moving == hwnd {
                a.moving = ptr::null_mut();
                show_hint(Target::None);
            }
            if let Some((gi, ti)) = find_tab(hwnd) {
                remove_tab(gi, ti, Leave::Gone);
            }
        }
        _ => {}
    }
}

unsafe fn begin_move(hwnd: HWND, synthetic: bool) {
    let a = app();
    let vr = vis(hwnd);
    log!(
        "move start {:?} \"{}\" synthetic={} eligible={}",
        hwnd,
        String::from_utf16_lossy(&title_of(hwnd)),
        synthetic,
        eligible(hwnd, false)
    );
    a.moving = hwnd;
    a.moving_size = (vr.right - vr.left, vr.bottom - vr.top);
    let mut pt: POINT = mem::zeroed();
    GetCursorPos(&mut pt);
    let edge = px(hwnd, 8);
    a.moving_by_title = IsZoomed(hwnd) != 0
        || (pt.x >= vr.left + edge && pt.x < vr.right - edge && pt.y >= vr.top + edge && pt.y < vr.bottom - edge);
    if synthetic {
        SetTimer(a.msg, TIMER_MOVE, 30, None);
    }
}

unsafe fn end_move() {
    let a = app();
    let hwnd = a.moving;
    KillTimer(a.msg, TIMER_MOVE);
    if hwnd.is_null() {
        return;
    }
    a.moving = ptr::null_mut();
    let target = a.target;
    log!("move end {:?} target={}", hwnd, describe(target));
    show_hint(Target::None);
    if IsWindow(hwnd) == 0 {
        return;
    }
    if let Some((gi, ti)) = find_tab(hwnd) {
        if a.groups[gi].active == ti {
            adopt(gi);
        }
        return;
    }
    let vr = vis(hwnd);
    // Only a move docks, not a resize. A move can change the size as well: Windows restores a
    // maximized or snapped window while it is dragged, and rescales one that crosses to a monitor
    // with another scaling. So the size alone does not decide, where the mouse took hold does too.
    let same_size = (vr.right - vr.left, vr.bottom - vr.top) == a.moving_size;
    if (same_size || a.moving_by_title) && eligible(hwnd, false) {
        dock(target, hwnd);
    }
}

/// Twice a second: drop members that vanished without an event, repair the z-order, refresh
/// the unread counts and let programs that are still starting join their group.
unsafe fn tick() {
    opening_step();
    launch_step();
    for gi in 0..app().groups.len() {
        let mut changed = false;
        for t in app().groups[gi].tabs.iter_mut() {
            let n = store::unread(&String::from_utf16_lossy(&title_of(t.hwnd))).unwrap_or(0);
            if n != t.unread {
                t.unread = n;
                changed = true;
            }
        }
        if changed {
            InvalidateRect(app().groups[gi].bar, ptr::null(), 0);
        }
    }
    let mut gi = 0;
    while gi < app().groups.len() {
        let bar = app().groups[gi].bar;
        let dead = app().groups[gi].tabs.iter().position(|t| IsWindow(t.hwnd) == 0);
        if let Some(ti) = dead {
            remove_tab(gi, ti, Leave::Gone);
            continue;
        }
        if find_bar(bar) == Some(gi) {
            let g = &app().groups[gi];
            if !g.minimized && GetForegroundWindow() == g.tabs[g.active].hwnd {
                raise_bar(gi);
            }
        }
        gi += 1;
    }
}

// ---------------------------------------------------------------------------------------------
// The tab bar
// ---------------------------------------------------------------------------------------------

struct Metrics {
    tabs: Vec<RECT>,
    plus: RECT,
    minimize: RECT,
    maximize: RECT,
    dissolve: RECT,
    close_w: i32,
}

unsafe fn metrics(g: &Group) -> Metrics {
    let bar = g.bar;
    let mut c: RECT = mem::zeroed();
    GetClientRect(bar, &mut c);
    let (pad, btn, gap) = (px(bar, 6), px(bar, 30), px(bar, 4));
    let (rows, n) = rows_for(bar, g.frame.right - g.frame.left, g.tabs.len());
    let avail = c.right - 2 * pad - 4 * btn - gap * (n + 3);
    let tw = (avail / n).min(px(bar, 210)).max(px(bar, 36));
    // The buttons stay in the first row. A single row fills the bar as before.
    let top = px(bar, 4);
    let bottom = if rows == 1 { c.bottom - px(bar, 4) } else { top + px(bar, 28) };
    let step = px(bar, 32);
    let tabs: Vec<RECT> = (0..g.tabs.len() as i32)
        .map(|i| {
            let left = pad + (i % n) * (tw + gap);
            let y = (i / n) * step;
            RECT { left, top: top + y, right: left + tw, bottom: bottom + y }
        })
        .collect();
    // The plus follows the last tab, in whichever row that is.
    let last = (g.tabs.len() as i32 - (rows - 1) * n).max(1);
    let plus_left = pad + last * (tw + gap);
    let plus_y = (rows - 1) * step;
    Metrics {
        tabs,
        plus: RECT { left: plus_left, top: top + plus_y, right: plus_left + btn, bottom: bottom + plus_y },
        minimize: RECT { left: c.right - pad - 3 * btn - 2 * gap, top, right: c.right - pad - 2 * btn - 2 * gap, bottom },
        maximize: RECT { left: c.right - pad - 2 * btn - gap, top, right: c.right - pad - btn - gap, bottom },
        dissolve: RECT { left: c.right - pad - btn, top, right: c.right - pad, bottom },
        close_w: if tw >= px(bar, 80) { px(bar, 24) } else { 0 },
    }
}

unsafe fn hit_test(g: &Group, p: POINT) -> Hit {
    let m = metrics(g);
    for (i, r) in m.tabs.iter().enumerate() {
        if inside(r, p) {
            return if m.close_w > 0 && p.x >= r.right - m.close_w { Hit::Close(i) } else { Hit::Tab(i) };
        }
    }
    if inside(&m.dissolve, p) {
        Hit::Dissolve
    } else if inside(&m.minimize, p) {
        Hit::Minimize
    } else if inside(&m.maximize, p) {
        Hit::Maximize
    } else if inside(&m.plus, p) {
        Hit::Plus
    } else {
        Hit::Empty
    }
}

unsafe fn fill_round(dc: HDC, r: &RECT, radius: i32, color: u32) {
    let brush = CreateSolidBrush(color);
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
    RoundRect(dc, r.left, r.top, r.right + 1, r.bottom + 1, radius, radius);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(brush);
}

unsafe fn make_font(h: HWND, size: i32, weight: i32) -> HFONT {
    CreateFontW(
        -px(h, size),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        0,
        0,
        CLEARTYPE_QUALITY as u32,
        0,
        w("Segoe UI").as_ptr(),
    )
}

unsafe fn text(dc: HDC, s: &[u16], r: &RECT, color: u32, flags: u32) {
    let mut r = *r;
    SetTextColor(dc, color);
    DrawTextW(dc, s.as_ptr(), s.len() as i32, &mut r, flags | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
}

unsafe fn paint_bar(bar: HWND) {
    let mut ps: PAINTSTRUCT = mem::zeroed();
    let screen = BeginPaint(bar, &mut ps);
    let mut c: RECT = mem::zeroed();
    GetClientRect(bar, &mut c);
    let dc = CreateCompatibleDC(screen);
    let bmp = CreateCompatibleBitmap(screen, c.right.max(1), c.bottom.max(1));
    let old_bmp = SelectObject(dc, bmp);
    let bg = CreateSolidBrush(rgb(0x16, 0x29, 0x3A));
    FillRect(dc, &c, bg);
    DeleteObject(bg);
    SetBkMode(dc, TRANSPARENT as i32);

    if let Some(gi) = find_bar(bar) {
        let g = &app().groups[gi];
        let m = metrics(g);
        let font = make_font(bar, 13, 500);
        let bold = make_font(bar, 13, 700);
        let glyph = make_font(bar, 17, 400);
        let small = make_font(bar, 11, 700);
        let radius = px(bar, 12);
        let ico = px(bar, 16);
        for (i, r) in m.tabs.iter().enumerate() {
            let on = i == g.active;
            if on {
                fill_round(dc, r, radius, rgb(0x3A, 0x57, 0x6C));
            } else {
                fill_round(dc, r, radius, rgb(0x1E, 0x3A, 0x50));
            }
            let t = &g.tabs[i];
            let mut left = r.left + px(bar, 9);
            if !t.icon.is_null() && r.right - r.left > px(bar, 50) {
                DrawIconEx(dc, left, (r.top + r.bottom - ico) / 2, t.icon, ico, ico, 0, ptr::null_mut(), DI_NORMAL);
                left += ico + px(bar, 7);
            }
            let old = SelectObject(dc, if on { bold } else { font });
            let mut right = r.right - m.close_w - px(bar, 4);
            if t.unread > 0 && r.right - r.left > px(bar, 50) {
                let n: Vec<u16> = if t.unread > 99 { "99+".into() } else { t.unread.to_string() }.encode_utf16().collect();
                let bw = px(bar, 12) + px(bar, 7) * n.len() as i32;
                let mid = (r.top + r.bottom) / 2;
                let badge = RECT { left: right - bw, top: mid - px(bar, 9), right, bottom: mid + px(bar, 9) };
                fill_round(dc, &badge, px(bar, 18), rgb(0xE5, 0x3E, 0x3E));
                SelectObject(dc, small);
                text(dc, &n, &badge, rgb(0xFF, 0xFF, 0xFF), DT_CENTER);
                SelectObject(dc, if on { bold } else { font });
                right = badge.left - px(bar, 4);
            }
            let label = RECT { left, top: r.top, right, bottom: r.bottom };
            let color = if on { rgb(0xFF, 0xFF, 0xFF) } else { rgb(0xB8, 0xC6, 0xCD) };
            text(dc, &t.name, &label, color, DT_LEFT | DT_END_ELLIPSIS);
            if m.close_w > 0 {
                SelectObject(dc, glyph);
                let x = RECT { left: r.right - m.close_w, top: r.top, right: r.right - px(bar, 4), bottom: r.bottom };
                text(dc, &[0x00D7], &x, rgb(0x9F, 0xB2, 0xBA), DT_CENTER);
            }
            SelectObject(dc, old);
        }
        let old = SelectObject(dc, glyph);
        fill_round(dc, &m.plus, radius, rgb(0x1E, 0x3A, 0x50));
        text(dc, &[0x002B], &m.plus, rgb(0xFF, 0xFF, 0xFF), DT_CENTER);
        for (hit, r) in [(Hit::Minimize, &m.minimize), (Hit::Maximize, &m.maximize), (Hit::Dissolve, &m.dissolve)] {
            if g.hover == hit {
                fill_round(dc, r, radius, if hit == Hit::Dissolve { rgb(0xC4, 0x2B, 0x1C) } else { rgb(0x2C, 0x48, 0x5E) });
            }
        }
        let light = if g.hover == Hit::Dissolve { rgb(0xFF, 0xFF, 0xFF) } else { rgb(0xB8, 0xC6, 0xCD) };
        text(dc, &[0x00D7], &m.dissolve, light, DT_CENTER);
        // Drawn with lines like the window buttons of Windows, sharp at every scaling.
        let pen = CreatePen(PS_SOLID as i32, px(bar, 1).max(1), rgb(0xD7, 0xE1, 0xE5));
        let old_pen = SelectObject(dc, pen);
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        let k = px(bar, 5);
        let (cx, cy) = ((m.minimize.left + m.minimize.right) / 2, (m.minimize.top + m.minimize.bottom) / 2);
        MoveToEx(dc, cx - k, cy, ptr::null_mut());
        LineTo(dc, cx + k + 1, cy);
        let (cx, cy) = ((m.maximize.left + m.maximize.right) / 2, (m.maximize.top + m.maximize.bottom) / 2);
        if g.restore.is_some() {
            // Two overlapping frames: back to the old size.
            let d = px(bar, 2);
            Rectangle(dc, cx - k, cy - k + d, cx + k - d + 1, cy + k + 1);
            MoveToEx(dc, cx - k + d, cy - k + d, ptr::null_mut());
            LineTo(dc, cx - k + d, cy - k);
            LineTo(dc, cx + k, cy - k);
            LineTo(dc, cx + k, cy + k - d);
            LineTo(dc, cx + k - d, cy + k - d);
        } else {
            Rectangle(dc, cx - k, cy - k, cx + k + 1, cy + k + 1);
        }
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(pen);
        SelectObject(dc, old);
        DeleteObject(font);
        DeleteObject(bold);
        DeleteObject(glyph);
        DeleteObject(small);
    }

    BitBlt(screen, 0, 0, c.right, c.bottom, dc, 0, 0, SRCCOPY);
    SelectObject(dc, old_bmp);
    DeleteObject(bmp);
    DeleteDC(dc);
    EndPaint(bar, &ps);
}

/// Shows a menu at the cursor and returns the chosen command, 0 if dismissed.
unsafe fn popup(items: &[(u32, u32, Vec<u16>)]) -> u32 {
    let menu = CreatePopupMenu();
    for (flags, id, label) in items {
        AppendMenuW(menu, *flags, *id as usize, label.as_ptr());
    }
    track(menu)
}

/// Runs a menu at the cursor and destroys it, submenus included.
unsafe fn track(menu: HMENU) -> u32 {
    let mut pt: POINT = mem::zeroed();
    GetCursorPos(&mut pt);
    let owner = app().msg;
    // The hidden owner moves under the cursor first. Left at its corner of the main screen, a menu
    // opened on another monitor with a different scaling can land on the wrong screen and miss clicks.
    SetWindowPos(owner, ptr::null_mut(), pt.x, pt.y, 0, 0, SWP_NOZORDER | SWP_NOACTIVATE);
    // Without this the menu does not close when the user clicks elsewhere.
    SetForegroundWindow(owner);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, owner, ptr::null());
    PostMessageW(owner, WM_NULL, 0, 0);
    DestroyMenu(menu);
    cmd as u32
}

/// Gives the keyboard back to the group's active window after a menu.
unsafe fn refocus(bar: HWND) {
    if let Some(gi) = find_bar(bar) {
        let g = &app().groups[gi];
        if let Some(t) = g.tabs.get(g.active) {
            focus(t.hwnd);
        }
    }
}

unsafe fn dissolve_menu(bar: HWND) {
    let cmd = popup(&[
        (MF_STRING, 1, w(tr("Gruppe auflösen", "Dissolve group"))),
        (MF_STRING, 2, w(tr("Alle Programme der Gruppe schließen", "Close all programs of the group"))),
        (MF_SEPARATOR, 0, w("")),
        (MF_STRING, 3, w(tr("Diese Gruppe als Paket sichern …", "Save this group as a package …"))),
        (MF_SEPARATOR, 0, w("")),
        (MF_STRING | rows_check(), 4, w(tr("Zweireihige Tabs", "Two rows of tabs"))),
    ]);
    let Some(gi) = find_bar(bar) else { return };
    match cmd {
        4 => toggle_rows(),
        3 => open_picker(Mode::Save(bar)),
        1 => {
            let h = app().groups[gi].tabs.get(app().groups[gi].active).map(|t| t.hwnd);
            dissolve(gi);
            if let Some(h) = h {
                focus(h);
            }
        }
        2 => {
            let hwnds: Vec<HWND> = app().groups[gi].tabs.iter().map(|t| t.hwnd).collect();
            for h in hwnds {
                PostMessageW(h, WM_CLOSE, 0, 0);
            }
            refocus(bar);
        }
        _ => refocus(bar),
    }
}

unsafe fn rows_check() -> u32 {
    if app().store.two_rows { MF_CHECKED } else { 0 }
}

/// Switches the second row of tabs on or off, for every group: the bars change their height and
/// the windows below make room.
unsafe fn toggle_rows() {
    let a = app();
    a.store.two_rows = !a.store.two_rows;
    save_store();
    for gi in 0..a.groups.len() {
        layout(gi, Lay::Active);
    }
}

unsafe fn tab_menu(bar: HWND, idx: usize) {
    let cmd = popup(&[
        (MF_STRING, 1, w(tr("Tab aus der Gruppe lösen", "Detach tab from the group"))),
        (MF_STRING, 2, w(tr("Gruppe auflösen", "Dissolve group"))),
        (MF_SEPARATOR, 0, w("")),
        (MF_STRING, 3, w(tr("Diese Gruppe als Paket sichern …", "Save this group as a package …"))),
        (MF_SEPARATOR, 0, w("")),
        (MF_STRING | rows_check(), 4, w(tr("Zweireihige Tabs", "Two rows of tabs"))),
    ]);
    let Some(gi) = find_bar(bar) else { return };
    match cmd {
        4 => toggle_rows(),
        3 => open_picker(Mode::Save(bar)),
        1 => remove_tab(gi, idx, Leave::Aside),
        2 => dissolve(gi),
        _ => refocus(bar),
    }
}

/// The top edge and the two top corners of the bar resize the group; the window's own top
/// border lies under the bar and cannot be reached.
/// Puts the active window of the group on top and gives it the keyboard; the bar follows.
/// Windows allows this here because the click on the bar was the last input.
/// Puts the group on top of the other windows without moving the keyboard focus.
unsafe fn raise_group(bar: HWND) {
    let Some(gi) = find_bar(bar) else { return };
    let g = &app().groups[gi];
    if g.minimized || g.tabs.is_empty() {
        return;
    }
    SetWindowPos(g.tabs[g.active].hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    if let Some(gi) = find_bar(bar) {
        raise_bar(gi);
    }
}

unsafe fn bring_front(bar: HWND) {
    let Some(gi) = find_bar(bar) else { return };
    let g = &app().groups[gi];
    if g.minimized || g.tabs.is_empty() {
        return;
    }
    let h = g.tabs[g.active].hwnd;
    focus(h);
    SetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    if let Some(gi) = find_bar(bar) {
        raise_bar(gi);
    }
}

/// Fills the work area of the monitor, or returns to the size before.
unsafe fn toggle_fill(gi: usize) {
    let g = &mut app().groups[gi];
    match g.restore.take() {
        Some(old) => g.frame = old,
        None => {
            g.restore = Some(g.frame);
            g.frame = work_area(&g.frame);
        }
    }
    layout(gi, Lay::All);
    if let Some(gi) = find_bar(app().groups.get(gi).map_or(ptr::null_mut(), |g| g.bar)) {
        InvalidateRect(app().groups[gi].bar, ptr::null(), 0);
    }
}

unsafe fn resize_zone(bar: HWND, p: POINT) -> Option<(bool, bool)> {
    let mut c: RECT = mem::zeroed();
    GetClientRect(bar, &mut c);
    let k = px(bar, 10);
    if p.y < 0 || p.y >= c.bottom || p.x < 0 || p.x >= c.right {
        None
    } else if p.y < k && p.x < k {
        Some((true, false))
    } else if p.y < k && p.x >= c.right - k {
        Some((false, true))
    } else if p.y < px(bar, 3) {
        Some((false, false))
    } else {
        None
    }
}

fn point(lp: LPARAM) -> POINT {
    POINT { x: (lp & 0xFFFF) as i16 as i32, y: ((lp >> 16) & 0xFFFF) as i16 as i32 }
}

unsafe extern "system" fn bar_proc(bar: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(bar, msg, wp, lp);
    }
    match msg {
        WM_PAINT => {
            paint_bar(bar);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_SETCURSOR => {
            let mut pt: POINT = mem::zeroed();
            GetCursorPos(&mut pt);
            ScreenToClient(bar, &mut pt);
            if let Some((left, right)) = resize_zone(bar, pt) {
                let shape = if left { IDC_SIZENWSE } else if right { IDC_SIZENESW } else { IDC_SIZENS };
                SetCursor(LoadCursorW(ptr::null_mut(), shape));
                return 1;
            }
        }
        WM_LBUTTONDOWN => {
            let Some(gi) = find_bar(bar) else { return 0 };
            let p = point(lp);
            // A click anywhere on the bar brings the whole group to the front, not only a click
            // on a tab. Minimizing and closing a tab do not need that.
            match hit_test(&app().groups[gi], p) {
                Hit::Minimize | Hit::Close(_) | Hit::Tab(_) => {}
                // The plus and the X open a window or a menu of their own that needs the keyboard.
                // Giving it to the group's window first would let that window take it back a
                // moment later and close what just opened. The group only comes on top.
                Hit::Plus | Hit::Dissolve => raise_group(bar),
                _ => bring_front(bar),
            }
            let Some(gi) = find_bar(bar) else { return 0 };
            if let Some((left, right)) = resize_zone(bar, p) {
                let mut from: POINT = mem::zeroed();
                GetCursorPos(&mut from);
                let g = &mut app().groups[gi];
                g.drag = Drag::Resize { left, right, start: g.frame, from };
                SetCapture(bar);
                return 0;
            }
            match hit_test(&app().groups[gi], p) {
                Hit::Close(i) => remove_tab(gi, i, Leave::Aside),
                Hit::Tab(i) => {
                    select(gi, i, true);
                    if let Some(gi) = find_bar(bar) {
                        app().groups[gi].drag = Drag::Pending(i, p);
                        SetCapture(bar);
                    }
                }
                Hit::Plus => open_picker(Mode::Add(bar)),
                Hit::Minimize => {
                    let g = &app().groups[gi];
                    let h = g.tabs[g.active].hwnd;
                    // The minimize event takes the bar and the other members along.
                    ShowWindow(h, SW_MINIMIZE);
                }
                Hit::Maximize => toggle_fill(gi),
                Hit::Dissolve => dissolve_menu(bar),
                Hit::Empty => {
                    let mut pt: POINT = mem::zeroed();
                    GetCursorPos(&mut pt);
                    let g = &mut app().groups[gi];
                    g.drag = Drag::Move(pt.x - g.frame.left, pt.y - g.frame.top);
                    SetCapture(bar);
                }
            }
            return 0;
        }
        WM_MOUSEMOVE => {
            let Some(gi) = find_bar(bar) else { return 0 };
            let p = point(lp);
            let g = &mut app().groups[gi];
            match g.drag {
                Drag::None => {
                    let hit = hit_test(g, p);
                    let hover = if matches!(hit, Hit::Minimize | Hit::Maximize | Hit::Dissolve) { hit } else { Hit::Empty };
                    if hover != g.hover {
                        g.hover = hover;
                        InvalidateRect(bar, ptr::null(), 0);
                        let mut t: TRACKMOUSEEVENT = mem::zeroed();
                        t.cbSize = mem::size_of::<TRACKMOUSEEVENT>() as u32;
                        t.dwFlags = TME_LEAVE;
                        t.hwndTrack = bar;
                        TrackMouseEvent(&mut t);
                    }
                }
                Drag::Pending(i, start) => {
                    if (p.x - start.x).abs() > px(bar, 5) || (p.y - start.y).abs() > px(bar, 5) {
                        g.drag = Drag::Tab(i);
                    }
                }
                Drag::Tab(i) => {
                    let slack = px(bar, 26);
                    if p.y < -slack || p.y > bar_h(bar) + slack {
                        // Pulled out: the tab becomes a window that follows the cursor.
                        g.drag = Drag::None;
                        let Some(h) = g.tabs.get(i).map(|t| t.hwnd) else { return 0 };
                        ReleaseCapture();
                        if let Some(gi) = find_bar(bar) {
                            remove_tab(gi, i, Leave::Float);
                        }
                        SetWindowPos(h, ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                        focus(h);
                        start_float(h);
                        return 0;
                    }
                    let m = metrics(g);
                    // The tab in this column whose row is nearest: with two rows the mouse picks the row.
                    let over = m
                        .tabs
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| p.x >= r.left && p.x < r.right)
                        .min_by_key(|(_, r)| ((r.top + r.bottom) / 2 - p.y).abs())
                        .map(|(i, _)| i);
                    if let Some(to) = over {
                        if to != i && i < g.tabs.len() {
                            let tab = g.tabs.remove(i);
                            g.tabs.insert(to, tab);
                            g.active = to;
                            g.drag = Drag::Tab(to);
                            InvalidateRect(bar, ptr::null(), 0);
                        }
                    }
                }
                Drag::Move(dx, dy) => {
                    let mut pt: POINT = mem::zeroed();
                    GetCursorPos(&mut pt);
                    let f = g.frame;
                    let moved = shifted(&f, pt.x - dx - f.left, pt.y - dy - f.top);
                    if !same(&moved, &f) {
                        g.frame = moved;
                        g.restore = None;
                        // Every member moves with the bar, not only the window on top.
                        layout(gi, Lay::All);
                    }
                }
                Drag::Resize { left, right, start, from } => {
                    let mut pt: POINT = mem::zeroed();
                    GetCursorPos(&mut pt);
                    let (dx, dy) = (pt.x - from.x, pt.y - from.y);
                    let (min_w, min_h) = (px(bar, 320), bar_h(bar) + px(bar, 160));
                    let mut f = start;
                    f.top = (start.top + dy).min(start.bottom - min_h);
                    if left {
                        f.left = (start.left + dx).min(start.right - min_w);
                    }
                    if right {
                        f.right = (start.right + dx).max(start.left + min_w);
                    }
                    if !same(&f, &g.frame) {
                        g.frame = f;
                        g.restore = None;
                        layout(gi, Lay::Active);
                    }
                }
            }
            return 0;
        }
        WM_LBUTTONUP => {
            if let Some(gi) = find_bar(bar) {
                let was = mem::replace(&mut app().groups[gi].drag, Drag::None);
                ReleaseCapture();
                if was.whole() {
                    if let Some(gi) = find_bar(bar) {
                        layout(gi, Lay::All);
                    }
                }
            }
            return 0;
        }
        WM_CAPTURECHANGED => {
            if let Some(gi) = find_bar(bar) {
                let was = mem::replace(&mut app().groups[gi].drag, Drag::None);
                if was.whole() {
                    layout(gi, Lay::All);
                }
            }
            return 0;
        }
        WM_LBUTTONDBLCLK => {
            let Some(gi) = find_bar(bar) else { return 0 };
            let g = &mut app().groups[gi];
            if hit_test(g, point(lp)) == Hit::Empty {
                g.drag = Drag::None;
                ReleaseCapture();
                if let Some(gi) = find_bar(bar) {
                    toggle_fill(gi);
                }
            }
            return 0;
        }
        WM_RBUTTONUP => {
            bring_front(bar);
            let Some(gi) = find_bar(bar) else { return 0 };
            match hit_test(&app().groups[gi], point(lp)) {
                Hit::Tab(i) | Hit::Close(i) => tab_menu(bar, i),
                _ => dissolve_menu(bar),
            }
            return 0;
        }
        WM_MOUSELEAVE => {
            if let Some(gi) = find_bar(bar) {
                if app().groups[gi].hover != Hit::Empty {
                    app().groups[gi].hover = Hit::Empty;
                    InvalidateRect(bar, ptr::null(), 0);
                }
            }
            return 0;
        }
        WM_DPICHANGED => {
            if let Some(gi) = find_bar(bar) {
                layout(gi, Lay::All);
            }
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(bar, msg, wp, lp)
}

unsafe extern "system" fn hint_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let mut ps: PAINTSTRUCT = mem::zeroed();
        let dc = BeginPaint(h, &mut ps);
        let mut c: RECT = mem::zeroed();
        GetClientRect(h, &mut c);
        let bg = CreateSolidBrush(rgb(0xF7, 0xB9, 0x21));
        FillRect(dc, &c, bg);
        DeleteObject(bg);
        SetBkMode(dc, TRANSPARENT as i32);
        let font = make_font(h, 14, 700);
        let old = SelectObject(dc, font);
        let label: Vec<u16> = tr("Loslassen zum Tabben", "Release to tab").encode_utf16().collect();
        text(dc, &label, &c, rgb(0x2A, 0x1E, 0x00), DT_CENTER);
        SelectObject(dc, old);
        DeleteObject(font);
        EndPaint(h, &ps);
        return 0;
    }
    DefWindowProcW(h, msg, wp, lp)
}

// ---------------------------------------------------------------------------------------------
// The picker: add a window, start a program, name a package, set a shortcut
// ---------------------------------------------------------------------------------------------

/// Programs whose path says nothing about the window they show; only the title identifies it.
fn is_host(exe: &str) -> bool {
    exe.is_empty() || exe.to_lowercase().ends_with("\\applicationframehost.exe")
}

unsafe fn window_label(h: HWND) -> String {
    let name = String::from_utf16_lossy(&name_of(h));
    let title = String::from_utf16_lossy(&title_of(h));
    if title == name || title.is_empty() {
        name
    } else {
        format!("{} \u{2013} {}", name, title.chars().take(60).collect::<String>())
    }
}

unsafe fn top_windows(allow_minimized: bool) -> Vec<HWND> {
    let mut out = Vec::new();
    let mut h = GetTopWindow(ptr::null_mut());
    while !h.is_null() {
        if eligible(h, allow_minimized) {
            out.push(h);
        }
        h = GetWindow(h, GW_HWNDNEXT);
    }
    out
}

fn walk_programs(dir: &std::path::Path, depth: u32, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if depth < 4 {
                walk_programs(&path, depth + 1, out);
            }
        } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let low = name.to_lowercase();
            if !name.is_empty() && !low.contains("uninstall") && !low.contains("deinstall") {
                out.push((name, path.to_string_lossy().into_owned()));
            }
        }
    }
}

/// The programs of the start menu, for all users and for this one.
fn scan_programs() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for var in ["ProgramData", "APPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            let dir = std::path::PathBuf::from(base).join("Microsoft\\Windows\\Start Menu\\Programs");
            walk_programs(&dir, 0, &mut out);
        }
    }
    out.sort_by_key(|(n, _)| n.to_lowercase());
    out.dedup_by_key(|(n, _)| n.to_lowercase());
    out
}

unsafe fn edit_text() -> String {
    store::clean(&String::from_utf16_lossy(&title_of(app().edit)))
}

unsafe fn set_rows(rows: Vec<(Entry, String)>) {
    let a = app();
    SendMessageW(a.list, LB_RESETCONTENT, 0, 0);
    for (_, label) in &rows {
        SendMessageW(a.list, LB_ADDSTRING, 0, w(label).as_ptr() as LPARAM);
    }
    a.entries = rows.into_iter().map(|r| r.0).collect();
    SendMessageW(a.list, LB_SETCURSEL, 0, 0);
}

unsafe fn fill_picker() {
    let a = app();
    let raw = edit_text();
    let q = raw.to_lowercase();
    let mut rows: Vec<(Entry, String)> = Vec::new();
    match a.mode {
        Mode::Closed => {}
        Mode::Add(_) => {
            for h in top_windows(true) {
                if find_tab(h).is_some() {
                    continue;
                }
                let label = window_label(h);
                if q.is_empty() || label.to_lowercase().contains(&q) {
                    rows.push((Entry::Window(h), label));
                }
            }
            if q.chars().count() >= 2 {
                for (name, path) in a.programs.iter().filter(|(n, _)| n.to_lowercase().contains(&q)).take(40) {
                    rows.push((Entry::Program(path.clone()), format!("{} {}", tr("Starten:", "Start:"), name)));
                }
            }
            if q.is_empty() {
                let save = tr("Diese Gruppe als Paket sichern …", "Save this group as a package …");
                rows.push((Entry::SavePrompt, save.to_string()));
                let hint = tr(
                    "Tippen sucht Fenster und installierte Programme.",
                    "Type to search windows and installed programs.",
                );
                rows.push((Entry::Info, hint.to_string()));
            } else if rows.is_empty() {
                rows.push((Entry::Info, tr("Nichts gefunden", "Nothing found").to_string()));
            }
        }
        Mode::Save(bar) => {
            let origin = find_bar(bar).and_then(|gi| a.groups[gi].package.clone());
            if !raw.is_empty() {
                rows.push((Entry::SaveNew, format!("{} „{}“", tr("Sichern als", "Save as"), raw)));
            } else if origin.is_none() {
                // First row, so that a bare Enter never overwrites a package by accident.
                let hint = tr("Namen für ein neues Paket eintippen, dann Enter.", "Type a name for a new package, then Enter.");
                rows.push((Entry::Info, hint.to_string()));
            }
            let mut order: Vec<usize> = (0..a.store.packages.len()).collect();
            order.sort_by_key(|i| Some(&a.store.packages[*i].name) != origin.as_ref());
            for i in order {
                let label = format!("{} „{}“", tr("Aktualisieren:", "Update:"), a.store.packages[i].name);
                rows.push((Entry::SaveOver(i), label));
            }
        }
        Mode::Hotkey(i) => {
            let name = a.store.packages.get(i).map(|p| p.name.clone()).unwrap_or_default();
            let ask = if EN {
                format!("Press the new shortcut for “{}”, with Ctrl or Alt.", name)
            } else {
                format!("Neues Kürzel für „{}“ drücken, mit Strg oder Alt.", name)
            };
            rows.push((Entry::Info, ask));
            rows.push((Entry::Info, tr("Esc bricht ab.", "Esc cancels.").to_string()));
        }
    }
    set_rows(rows);
}

unsafe fn open_picker(mode: Mode) {
    let a = app();
    a.mode = mode;
    let p = a.picker;
    let mut pt: POINT = mem::zeroed();
    GetCursorPos(&mut pt);
    // On the right monitor first, so that the sizes use that monitor's scaling.
    SetWindowPos(p, ptr::null_mut(), pt.x, pt.y, 10, 10, SWP_NOZORDER | SWP_NOACTIVATE);
    let hotkey = matches!(mode, Mode::Hotkey(_));
    let (wd, ht) = (px(p, 400), if hotkey { px(p, 120) } else { px(p, 360) });
    let wa = work_area(&RECT { left: pt.x, top: pt.y, right: pt.x + 1, bottom: pt.y + 1 });
    let x = pt.x.min(wa.right - wd).max(wa.left);
    let y = pt.y.min(wa.bottom - ht).max(wa.top);
    if !a.picker_font.is_null() {
        DeleteObject(a.picker_font);
    }
    a.picker_font = make_font(p, 14, 400);
    let (pad, eh) = (px(p, 8), px(p, 26));
    SetWindowPos(a.edit, ptr::null_mut(), pad, pad, wd - 2 * pad - 2, eh, SWP_NOZORDER);
    SetWindowPos(a.list, ptr::null_mut(), pad, 2 * pad + eh, wd - 2 * pad - 2, ht - eh - 3 * pad - 2, SWP_NOZORDER);
    SendMessageW(a.edit, WM_SETFONT, a.picker_font as WPARAM, 1);
    SendMessageW(a.list, WM_SETFONT, a.picker_font as WPARAM, 1);
    let cue = match mode {
        Mode::Add(_) => tr("Fenster oder Programm suchen", "Search window or program"),
        Mode::Save(_) => tr("Name des Pakets", "Name of the package"),
        _ => tr("Kürzel drücken", "Press shortcut"),
    };
    SendMessageW(a.edit, EM_SETCUEBANNER, 1, w(cue).as_ptr() as LPARAM);
    if matches!(mode, Mode::Add(_)) {
        a.programs = scan_programs();
    }
    SetWindowTextW(a.edit, w("").as_ptr());
    fill_picker();
    SetWindowPos(p, HWND_TOPMOST, x, y, wd, ht, SWP_SHOWWINDOW);
    a.picker_at = Some(Instant::now());
    focus(p);
    SetFocus(a.edit);
}

unsafe fn close_picker() {
    let a = app();
    if a.mode == Mode::Closed {
        return;
    }
    a.mode = Mode::Closed;
    ReleaseCapture();
    ShowWindow(a.picker, SW_HIDE);
}

/// Acts on the selected row.
unsafe fn choose() {
    let a = app();
    let idx = SendMessageW(a.list, LB_GETCURSEL, 0, 0);
    if idx < 0 {
        return;
    }
    let Some(entry) = a.entries.get(idx as usize).cloned() else { return };
    let name = edit_text();
    match (a.mode, entry) {
        (Mode::Add(bar), Entry::Window(h)) => {
            close_picker();
            if let Some(gi) = find_bar(bar) {
                if IsWindow(h) != 0 {
                    insert_tab(gi, h, None);
                }
            }
        }
        (Mode::Add(bar), Entry::Program(path)) => {
            close_picker();
            launch_into(bar, &path);
        }
        (Mode::Add(bar), Entry::SavePrompt) => open_picker(Mode::Save(bar)),
        (Mode::Save(bar), Entry::SaveNew) => {
            close_picker();
            save_package(bar, Some(name), None);
            refocus(bar);
        }
        (Mode::Save(bar), Entry::SaveOver(i)) => {
            close_picker();
            save_package(bar, None, Some(i));
            refocus(bar);
        }
        _ => {}
    }
}

/// Keys for the picker, taken out of the message loop before the text field sees them.
unsafe fn picker_key(m: &MSG) -> bool {
    let a = app();
    if a.mode == Mode::Closed || !(m.hwnd == a.edit || m.hwnd == a.list || m.hwnd == a.picker) {
        return false;
    }
    let down = m.message == WM_KEYDOWN || m.message == WM_SYSKEYDOWN;
    if let Mode::Hotkey(i) = a.mode {
        if down {
            hotkey_pressed(i, m.wParam as u32);
            return true;
        }
        return matches!(m.message, WM_KEYUP | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR);
    }
    if m.message != WM_KEYDOWN {
        return false;
    }
    match m.wParam as u16 {
        VK_ESCAPE => {
            let bar = match a.mode {
                Mode::Add(b) | Mode::Save(b) => b,
                _ => ptr::null_mut(),
            };
            close_picker();
            refocus(bar);
        }
        VK_RETURN => choose(),
        VK_DOWN | VK_UP => {
            let n = SendMessageW(a.list, LB_GETCOUNT, 0, 0);
            let cur = SendMessageW(a.list, LB_GETCURSEL, 0, 0);
            let next = if m.wParam as u16 == VK_DOWN { cur + 1 } else { cur - 1 };
            if n > 0 {
                SendMessageW(a.list, LB_SETCURSEL, next.clamp(0, n - 1) as WPARAM, 0);
            }
        }
        _ => return false,
    }
    true
}

unsafe extern "system" fn picker_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(h, msg, wp, lp);
    }
    match msg {
        WM_COMMAND => {
            let (id, code) = ((wp & 0xFFFF) as u32, ((wp >> 16) & 0xFFFF) as u32);
            if id == 1 && code == EN_CHANGE {
                fill_picker();
            } else if id == 2 && code == LBN_SELCHANGE {
                choose();
            }
            return 0;
        }
        WM_ACTIVATE => {
            // Clicked somewhere else: the picker goes away like a menu.
            if (wp & 0xFFFF) as u32 == WA_INACTIVE {
                // Losing the keyboard right after opening is not the user clicking elsewhere but
                // another window answering the same click late. Take it back once.
                let fresh = app().picker_at.take().is_some_and(|t| t.elapsed() < Duration::from_millis(400));
                if fresh && app().mode != Mode::Closed {
                    focus(h);
                    SetFocus(app().edit);
                } else {
                    close_picker();
                }
            }
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(h, msg, wp, lp)
}

// ---------------------------------------------------------------------------------------------
// Packages and their shortcuts
// ---------------------------------------------------------------------------------------------

const NOREPEAT: u32 = 0x4000;

unsafe fn notify(title: &str, body: &str) {
    let a = app();
    a.balloon_update = false;
    let mut d: NOTIFYICONDATAW = mem::zeroed();
    d.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    d.hWnd = a.msg;
    d.uID = 1;
    d.uFlags = NIF_INFO | NIF_ICON;
    d.hIcon = LoadIconW(a.inst, 1 as *const u16);
    put(&mut d.szInfoTitle, title);
    put(&mut d.szInfo, body);
    d.dwInfoFlags = NIIF_USER;
    Shell_NotifyIconW(NIM_MODIFY, &d);
}

/// The first of Alt+Shift+1 to 9 that neither a package nor another program uses.
/// Ctrl+Alt is avoided: Windows treats it as AltGr.
unsafe fn free_hotkey() -> Option<(u32, u32)> {
    let a = app();
    let mods = store::MOD_ALT | store::MOD_SHIFT;
    for vk in 0x31..=0x39u32 {
        if a.store.packages.iter().any(|p| p.hotkey == Some((mods, vk))) {
            continue;
        }
        if RegisterHotKey(a.msg, HOTKEY_PROBE, mods | NOREPEAT, vk) != 0 {
            UnregisterHotKey(a.msg, HOTKEY_PROBE);
            return Some((mods, vk));
        }
    }
    None
}

/// Registers the shortcuts of all packages anew; the hotkey id is the package index plus one.
unsafe fn rebind_hotkeys() {
    let a = app();
    for i in 0..a.hotkeys {
        UnregisterHotKey(a.msg, 1 + i as i32);
    }
    a.hotkeys = a.store.packages.len();
    for (i, p) in a.store.packages.iter().enumerate() {
        if let Some((mods, vk)) = p.hotkey {
            let ok = RegisterHotKey(a.msg, 1 + i as i32, mods | NOREPEAT, vk) != 0;
            log!("hotkey {} for \"{}\" registered={}", store::hotkey_name(mods, vk, true), p.name, ok);
        }
    }
}

unsafe fn hotkey_pressed(i: usize, vk: u32) {
    let a = app();
    if vk == VK_ESCAPE as u32 {
        close_picker();
        return;
    }
    // Modifier keys alone are not a shortcut yet.
    if matches!(vk, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5) {
        return;
    }
    let held = |k: u16| GetKeyState(k as i32) < 0;
    let mut mods = 0u32;
    if held(VK_CONTROL) {
        mods |= store::MOD_CONTROL;
    }
    if held(VK_MENU) {
        mods |= store::MOD_ALT;
    }
    if held(VK_SHIFT) {
        mods |= store::MOD_SHIFT;
    }
    if held(VK_LWIN) || held(VK_RWIN) {
        mods |= store::MOD_WIN;
    }
    let name = store::hotkey_name(mods, vk, EN);
    let other = a.store.packages.iter().enumerate().find(|(k, p)| *k != i && p.hotkey == Some((mods, vk)));
    let problem: Option<String> = if let Some(text) = store::hotkey_problem(mods, EN) {
        Some(text.to_string())
    } else if let Some((_, p)) = other {
        Some(format!("{} „{}“.", tr("Gehört schon zu Paket", "Already belongs to package"), p.name))
    } else {
        if i < a.hotkeys {
            UnregisterHotKey(a.msg, 1 + i as i32);
        }
        if RegisterHotKey(a.msg, HOTKEY_PROBE, mods | NOREPEAT, vk) != 0 {
            UnregisterHotKey(a.msg, HOTKEY_PROBE);
            None
        } else {
            let taken = tr(
                "Windows oder ein anderes Programm benutzt dieses Kürzel schon.",
                "Windows or another program already uses this shortcut.",
            );
            Some(taken.to_string())
        }
    };
    match problem {
        Some(text) => {
            rebind_hotkeys();
            set_rows(vec![
                (Entry::Info, format!("{}: {}", name, text)),
                (Entry::Info, tr("Anderes Kürzel drücken oder Esc.", "Press another shortcut or Esc.").to_string()),
            ]);
        }
        None => {
            let mut package = String::new();
            if let Some(p) = a.store.packages.get_mut(i) {
                p.hotkey = Some((mods, vk));
                package = p.name.clone();
            }
            save_store();
            rebind_hotkeys();
            close_picker();
            notify(&package, &format!("{} {}", tr("Kürzel:", "Shortcut:"), name));
        }
    }
}

/// Saves the group of `bar` as a new package called `name`, or over package `over`.
unsafe fn save_package(bar: HWND, name: Option<String>, over: Option<usize>) {
    let a = app();
    let Some(gi) = find_bar(bar) else { return };
    let mut tabs = Vec::new();
    for t in a.groups[gi].tabs.iter() {
        tabs.push(PTab {
            exe: String::from_utf16_lossy(&exe_of(t.hwnd)),
            title: store::clean(&String::from_utf16_lossy(&title_of(t.hwnd))),
        });
    }
    let f = a.groups[gi].frame;
    let frame = (f.left, f.top, f.right, f.bottom);
    let name = store::clean(&name.unwrap_or_default());
    // A new package with the name of an existing one updates that one.
    let over = over.or_else(|| a.store.packages.iter().position(|p| p.name.to_lowercase() == name.to_lowercase()));
    let idx = match over {
        Some(i) if i < a.store.packages.len() => {
            a.store.packages[i].frame = frame;
            a.store.packages[i].tabs = tabs;
            i
        }
        _ => {
            if name.is_empty() {
                return;
            }
            let hotkey = free_hotkey();
            a.store.packages.push(Package { name, hotkey, frame, tabs });
            a.store.packages.len() - 1
        }
    };
    let p = &a.store.packages[idx];
    a.groups[gi].package = Some(p.name.clone());
    let body = match p.hotkey {
        Some((m, k)) => format!("{} {}", tr("Paket gesichert. Kürzel:", "Package saved. Shortcut:"), store::hotkey_name(m, k, EN)),
        None => tr("Paket gesichert.", "Package saved.").to_string(),
    };
    let title = p.name.clone();
    log!("package \"{}\" saved with {} tabs", title, p.tabs.len());
    save_store();
    rebind_hotkeys();
    notify(&title, &body);
}

/// A free window of the program `exe`, preferably the one titled `title`.
unsafe fn find_free(exe: &str, title: &str, claimed: &[HWND]) -> HWND {
    let want = exe.to_lowercase();
    let mut fallback: HWND = ptr::null_mut();
    for h in top_windows(true) {
        if find_tab(h).is_some() || claimed.contains(&h) {
            continue;
        }
        let same_title = !title.is_empty() && String::from_utf16_lossy(&title_of(h)) == title;
        if is_host(exe) {
            if same_title {
                return h;
            }
            continue;
        }
        if lower(&exe_of(h)) != want {
            continue;
        }
        if same_title {
            return h;
        }
        if fallback.is_null() {
            fallback = h;
        }
    }
    fallback
}

/// Opens a package: brings its group to the front if it is open, otherwise collects the
/// windows of its programs, starts the missing ones and puts the group back in its place.
unsafe fn open_package(i: usize) {
    let a = app();
    let Some(p) = a.store.packages.get(i) else { return };
    let name = p.name.clone();
    if let Some(gi) = a.groups.iter().position(|g| g.package.as_deref() == Some(name.as_str())) {
        let g = &a.groups[gi];
        if let Some(t) = g.tabs.get(g.active) {
            let h = t.hwnd;
            if IsIconic(h) != 0 {
                ShowWindow(h, SW_RESTORE);
            }
            focus(h);
        }
        return;
    }
    let mut frame = RECT { left: p.frame.0, top: p.frame.1, right: p.frame.2, bottom: p.frame.3 };
    // The monitor of that day may be gone: then the group opens on the main screen.
    if MonitorFromRect(&frame, MONITOR_DEFAULTTONULL).is_null() {
        let wa = work_area(&RECT { left: 0, top: 0, right: 1, bottom: 1 });
        let wd = (frame.right - frame.left).min(wa.right - wa.left);
        let ht = (frame.bottom - frame.top).min(wa.bottom - wa.top);
        let (x, y) = (wa.left + (wa.right - wa.left - wd) / 2, wa.top + (wa.bottom - wa.top - ht) / 2);
        frame = RECT { left: x, top: y, right: x + wd, bottom: y + ht };
    }
    let mut slots: Vec<Slot> = Vec::new();
    for t in p.tabs.iter() {
        let claimed: Vec<HWND> = slots.iter().map(|s| s.hwnd).filter(|h| !h.is_null()).collect();
        let hwnd = find_free(&t.exe, &t.title, &claimed);
        slots.push(Slot { exe: t.exe.clone(), title: t.title.clone(), hwnd });
    }
    for s in slots.iter().filter(|s| s.hwnd.is_null() && !is_host(&s.exe)) {
        log!("package \"{}\": starting {}", name, s.exe);
        ShellExecuteW(ptr::null_mut(), w("open").as_ptr(), w(&s.exe).as_ptr(), ptr::null(), ptr::null(), SW_SHOWNORMAL);
    }
    log!("package \"{}\" opening, {} of {} windows found", name, slots.iter().filter(|s| !s.hwnd.is_null()).count(), slots.len());
    a.opening = Some(Opening { name, frame, slots, bar: ptr::null_mut(), deadline: Instant::now() + Duration::from_secs(30) });
    opening_step();
}

/// Lets the windows of a package that is being opened join, in their saved order.
unsafe fn opening_step() {
    let Some(mut o) = app().opening.take() else { return };
    if !o.bar.is_null() && find_bar(o.bar).is_none() {
        return;
    }
    // A start-up screen that closed again frees its place for the real window.
    for s in o.slots.iter_mut() {
        if !s.hwnd.is_null() && IsWindow(s.hwnd) == 0 {
            s.hwnd = ptr::null_mut();
        }
    }
    let mut fresh: Vec<usize> = Vec::new();
    for k in 0..o.slots.len() {
        if !o.slots[k].hwnd.is_null() {
            continue;
        }
        let claimed: Vec<HWND> = o.slots.iter().map(|s| s.hwnd).filter(|h| !h.is_null()).collect();
        let h = find_free(&o.slots[k].exe, &o.slots[k].title, &claimed);
        if !h.is_null() {
            o.slots[k].hwnd = h;
            fresh.push(k);
        }
    }
    if o.bar.is_null() {
        let ready: Vec<HWND> = o.slots.iter().map(|s| s.hwnd).filter(|h| !h.is_null()).collect();
        if ready.len() >= 2 {
            match create_group_from(&ready, Some(o.frame), 0, Some(o.name.clone())) {
                Some(bar) => o.bar = bar,
                None => return,
            }
        }
    } else {
        for k in fresh {
            let Some(gi) = find_bar(o.bar) else { return };
            let pos = o.slots[..k].iter().filter(|s| find_tab(s.hwnd).is_some_and(|(g, _)| g == gi)).count();
            insert_tab(gi, o.slots[k].hwnd, Some(pos));
        }
    }
    let complete = !o.bar.is_null() && o.slots.iter().all(|s| !s.hwnd.is_null());
    if !complete && Instant::now() < o.deadline {
        app().opening = Some(o);
    } else {
        log!("package \"{}\" opening finished, complete={}", o.name, complete);
    }
}

/// Starts a program from the start menu; its first new window joins the group of `bar`.
unsafe fn launch_into(bar: HWND, path: &str) {
    let known = top_windows(true);
    log!("starting {}", path);
    ShellExecuteW(ptr::null_mut(), w("open").as_ptr(), w(path).as_ptr(), ptr::null(), ptr::null(), SW_SHOWNORMAL);
    app().launch = Some(Launch { bar, known, deadline: Instant::now() + Duration::from_secs(20) });
}

unsafe fn launch_step() {
    let Some(l) = app().launch.take() else { return };
    let Some(gi) = find_bar(l.bar) else { return };
    for h in top_windows(false) {
        if !l.known.contains(&h) && find_tab(h).is_none() {
            insert_tab(gi, h, None);
            return;
        }
    }
    if Instant::now() < l.deadline {
        app().launch = Some(l);
    }
}

// ---------------------------------------------------------------------------------------------
// Updates: version check, download with the key of the purchase, swapping the program file
// ---------------------------------------------------------------------------------------------

const SERVER: &str = "dockodile.de";
const UPDATE_PAGE: &str = "https://dockodile.de/update.html";

/// One HTTPS request to dockodile.de. Windows checks the certificate. Returns status and body.
fn http(method: &str, path: &str, form: Option<&str>, limit: usize) -> Result<(u32, Vec<u8>), String> {
    use windows_sys::Win32::Networking::WinHttp::*;
    unsafe {
        let agent = w(&format!("Dockodile/{VERSION} (Windows)"));
        let mut session = WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, ptr::null(), ptr::null(), 0);
        if session.is_null() {
            // Before Windows 8.1 and in Wine.
            session = WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, ptr::null(), ptr::null(), 0);
        }
        if session.is_null() {
            return Err("WinHttpOpen".into());
        }
        WinHttpSetTimeouts(session, 15_000, 15_000, 30_000, 120_000);
        let (host, port, secure) = (SERVER.to_string(), 443u16, WINHTTP_FLAG_SECURE);
        // Development only: a local test server without TLS.
        #[cfg(feature = "selftest")]
        let (host, port, secure) = match std::env::var("DOCKODILE_TEST_PORT").ok().and_then(|p| p.parse::<u16>().ok()) {
            Some(p) => ("127.0.0.1".to_string(), p, 0),
            None => (host, port, secure),
        };
        let conn = WinHttpConnect(session, w(&host).as_ptr(), port, 0);
        let req = if conn.is_null() {
            ptr::null_mut()
        } else {
            WinHttpOpenRequest(conn, w(method).as_ptr(), w(path).as_ptr(), ptr::null(), ptr::null(), ptr::null(), secure)
        };
        let close = || {
            if !req.is_null() {
                WinHttpCloseHandle(req);
            }
            if !conn.is_null() {
                WinHttpCloseHandle(conn);
            }
            WinHttpCloseHandle(session);
        };
        if req.is_null() {
            close();
            return Err("WinHttpOpenRequest".into());
        }
        let headers = w("Content-Type: application/x-www-form-urlencoded\r\n");
        let body = form.unwrap_or("").as_bytes();
        let sent = WinHttpSendRequest(
            req,
            if form.is_some() { headers.as_ptr() } else { ptr::null() },
            if form.is_some() { u32::MAX } else { 0 },
            if body.is_empty() { ptr::null() } else { body.as_ptr() as *const _ },
            body.len() as u32,
            body.len() as u32,
            0,
        );
        if sent == 0 || WinHttpReceiveResponse(req, ptr::null_mut()) == 0 {
            let e = GetLastError();
            close();
            return Err(format!("connection failed ({e})"));
        }
        let mut status = 0u32;
        let mut size = 4u32;
        WinHttpQueryHeaders(
            req,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut status as *mut u32 as *mut _,
            &mut size,
            ptr::null_mut(),
        );
        let mut out: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            if WinHttpReadData(req, buf.as_mut_ptr() as *mut _, buf.len() as u32, &mut read) == 0 {
                close();
                return Err("download interrupted".into());
            }
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
            if out.len() > limit {
                close();
                return Err("answer too large".into());
            }
        }
        close();
        Ok((status, out))
    }
}

fn hand_over(result: UpdateResult, msg: isize) {
    if let Ok(mut slot) = UPDATE_RESULT.lock() {
        *slot = Some(result);
    }
    unsafe {
        PostMessageW(msg as HWND, WM_UPDATE, 0, 0);
    }
}

fn own_dir() -> std::path::PathBuf {
    let exe = std::path::PathBuf::from(String::from_utf16_lossy(&unsafe { own_path() }));
    exe.parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

/// Updates install only with the key of a purchase and where the program file may be replaced.
/// In "Programme" (Program Files) that needs administrator rights: then the update page helps.
fn can_install() -> bool {
    if cfg!(feature = "microsoft-store") { return false; }
    if app().store.token.is_none() {
        return false;
    }
    let probe = own_dir().join("dockodile.write-test");
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Starts the daily check, or stops it when automatic updates are off.
unsafe fn schedule_updates() {
    let a = app();
    KillTimer(a.msg, TIMER_UPDATE);
    if cfg!(feature = "microsoft-store") { return; }
    if a.store.manual_updates {
        return;
    }
    check_update(false);
    SetTimer(a.msg, TIMER_UPDATE, 24 * 60 * 60 * 1000, None);
}

unsafe fn check_update(manual: bool) {
    if cfg!(feature = "microsoft-store") {
        if manual { open_url("ms-windows-store://downloadsandupdates"); }
        return;
    }
    let a = app();
    if a.updating {
        return;
    }
    a.updating = true;
    let msg = a.msg as isize;
    std::thread::spawn(move || {
        let version = match http("GET", "/api/version?platform=windows", None, 4096) {
            Ok((200, body)) => store::version_field(&String::from_utf8_lossy(&body)),
            // The log file belongs to the UI thread; a failed check only shows up when asked.
            _ => None,
        };
        hand_over(UpdateResult::Version(version, manual), msg);
    });
}

unsafe fn update_found(version: Option<String>, manual: bool) {
    let a = app();
    let Some(version) = version else {
        if manual {
            let text = tr(
                "Die Suche nach Updates hat nicht geklappt. Prüfe die Internetverbindung und versuch es später noch einmal.",
                "Checking for updates failed. Check your internet connection and try again later.",
            );
            message(text, MB_ICONWARNING);
        }
        return;
    };
    log!("update check: server has {}, this is {}", version, VERSION);
    if !store::newer(&version, VERSION) {
        a.available = None;
        if manual {
            let text = if EN {
                format!("Dockodile is up to date. Version {VERSION} is the newest.")
            } else {
                format!("Dockodile ist aktuell. Version {VERSION} ist die neueste.")
            };
            message(&text, MB_ICONINFORMATION);
        }
        return;
    }
    a.available = Some(version.clone());
    if !manual {
        // Without asking only while no group is open that a restart would take apart, and only
        // once per version: if the swap fails, the next time asks instead of trying forever.
        if can_install() && a.groups.is_empty() && a.store.tried.as_deref() != Some(version.as_str()) {
            a.store.tried = Some(version);
            save_store();
            install_update();
            return;
        }
        // Announced once per version. After that it waits in the menu.
        if a.store.offered.as_deref() == Some(version.as_str()) {
            return;
        }
        a.store.offered = Some(version.clone());
        save_store();
        let title = if EN { format!("Dockodile {version} is available") } else { format!("Dockodile {version} ist da") };
        notify(&title, tr("Hier klicken zum Aktualisieren.", "Click here to update."));
        a.balloon_update = true;
        return;
    }
    offer_update();
}

unsafe fn message(text: &str, icon: u32) -> i32 {
    MessageBoxW(app().msg, w(text).as_ptr(), w("Dockodile").as_ptr(), icon | MB_SETFOREGROUND)
}

/// The update question. Also behind the menu item.
unsafe fn offer_update() {
    let Some(version) = app().available.clone() else { return };
    let direct = can_install();
    let text = match (direct, EN) {
        (true, false) => format!("Dockodile {version} ist da. Du nutzt Version {VERSION}.\n\nDockodile lädt das Update, ersetzt sich selbst und startet neu. Offene Gruppen werden dabei aufgelöst.\n\nJetzt installieren?"),
        (true, true) => format!("Dockodile {version} is available. You are using version {VERSION}.\n\nDockodile downloads the update, replaces itself and starts again. Open groups are dissolved.\n\nInstall now?"),
        (false, false) => format!("Dockodile {version} ist da. Du nutzt Version {VERSION}.\n\nAuf der Update-Seite bekommst du die neue Version mit der E-Mail-Adresse deines Kaufs. Dort schaltest du auch die Installation direkt in der App frei.\n\nUpdate-Seite öffnen?"),
        (false, true) => format!("Dockodile {version} is available. You are using version {VERSION}.\n\nThe update page sends you the new version to the email address of your purchase. It also unlocks installing straight from the app.\n\nOpen the update page?"),
    };
    if message(&text, MB_YESNO | MB_ICONQUESTION) != IDYES {
        return;
    }
    if direct {
        install_update();
    } else {
        open_url(UPDATE_PAGE);
    }
}

unsafe fn open_url(url: &str) {
    ShellExecuteW(ptr::null_mut(), w("open").as_ptr(), w(url).as_ptr(), ptr::null(), ptr::null(), SW_SHOWNORMAL);
}

unsafe fn install_update() {
    if cfg!(feature = "microsoft-store") { return; }
    let a = app();
    let Some(token) = a.store.token.clone() else { return };
    if a.updating {
        return;
    }
    a.updating = true;
    let msg = a.msg as isize;
    let target = own_dir().join("dockodile.new.exe");
    log!("update: downloading");
    std::thread::spawn(move || {
        let result = match http("POST", "/api/app/update", Some(&format!("token={token}&platform=windows")), 64 << 20) {
            // An unsigned program can only be checked for its shape; HTTPS vouches for the origin.
            Ok((200, body)) if body.len() > 100_000 && body.starts_with(b"MZ") => {
                std::fs::write(&target, &body).map(|_| target).map_err(|e| e.to_string())
            }
            Ok((403, _)) => Err(tr(
                "Zu diesem Schlüssel gibt es keinen gültigen Kauf. Hol dir auf der Update-Seite einen neuen Link.",
                "There is no valid purchase for this key. Get a new link on the update page.",
            )
            .to_string()),
            Ok((code, _)) => Err(format!("{} ({code})", tr("Der Server hat das Update nicht geliefert", "The server did not deliver the update"))),
            Err(e) => Err(e),
        };
        hand_over(UpdateResult::File(result), msg);
    });
}

/// Swaps the program file and starts the new version. Windows lets a running program file be
/// renamed, not overwritten; the old one is deleted at the next start.
unsafe fn update_downloaded(result: Result<std::path::PathBuf, String>) {
    let new = match result {
        Ok(p) => p,
        Err(e) => {
            log!("update: {}", e);
            let text = format!("{}\n\n{}", tr("Das Update hat nicht geklappt.", "The update did not work."), e);
            message(&text, MB_ICONWARNING);
            return;
        }
    };
    let exe = std::path::PathBuf::from(String::from_utf16_lossy(&own_path()));
    let old = own_dir().join("dockodile.old.exe");
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&exe, &old) {
        log!("update: cannot move the running program: {}", e);
        let _ = std::fs::remove_file(&new);
        message(tr("Das Update konnte die Programmdatei nicht ersetzen.", "The update could not replace the program file."), MB_ICONWARNING);
        return;
    }
    if let Err(e) = std::fs::rename(&new, &exe) {
        log!("update: cannot put the new program in place: {}", e);
        let _ = std::fs::rename(&old, &exe);
        message(tr("Das Update konnte die Programmdatei nicht ersetzen.", "The update could not replace the program file."), MB_ICONWARNING);
        return;
    }
    log!("update: installed, restarting");
    let path = w(&exe.to_string_lossy());
    ShellExecuteW(ptr::null_mut(), w("open").as_ptr(), path.as_ptr(), w("--updated").as_ptr(), ptr::null(), SW_SHOWNORMAL);
    DestroyWindow(app().msg);
}

unsafe fn activate(token: String) {
    if cfg!(feature = "microsoft-store") { return; }
    let a = app();
    if let Some(old) = &a.store.token {
        if *old == token {
            notify(tr("Updates sind freigeschaltet.", "Updates are unlocked."), "");
            return;
        }
        // Any web page can open this link, so an existing key is never replaced silently.
        let ask = tr(
            "Dockodile ist schon für Updates freigeschaltet. Ersetze den Schlüssel nur, wenn du den Link selbst gerade angefordert hast.\n\nErsetzen?",
            "Dockodile is already unlocked for updates. Replace the key only if you just asked for this link yourself.\n\nReplace?",
        );
        if message(ask, MB_YESNO | MB_ICONQUESTION) != IDYES {
            return;
        }
    }
    a.store.token = Some(token);
    a.store.tried = None;
    save_store();
    log!("update: unlocked");
    notify(
        tr("Updates sind freigeschaltet.", "Updates are unlocked."),
        tr("Dockodile lädt neue Versionen ab jetzt selbst.", "From now on Dockodile downloads new versions itself."),
    );
    check_update(false);
}

// ---------------------------------------------------------------------------------------------
// Control by link: dockodile://open?paket=Name and dockodile://tab?app=Name
// ---------------------------------------------------------------------------------------------

unsafe fn handle_url(url: &str) {
    let a = app();
    log!("url {}", url);
    match store::parse_url(url) {
        Some(store::Command::Open(name)) => {
            let want = name.to_lowercase();
            if let Some(i) = a.store.packages.iter().position(|p| p.name.to_lowercase() == want) {
                open_package(i);
                return;
            }
        }
        Some(store::Command::Activate(token)) => {
            activate(token);
            return;
        }
        Some(store::Command::Tab(q)) => {
            let q = q.to_lowercase();
            for gi in 0..a.groups.len() {
                for ti in 0..a.groups[gi].tabs.len() {
                    let t = &a.groups[gi].tabs[ti];
                    if lower(&t.name).contains(&q) || lower(&title_of(t.hwnd)).contains(&q) {
                        let h = t.hwnd;
                        if a.groups[gi].minimized {
                            ShowWindow(h, SW_RESTORE);
                        }
                        select(gi, ti, true);
                        return;
                    }
                }
            }
        }
        None => {}
    }
    MessageBeep(MB_ICONWARNING);
}

unsafe fn reg_set(sub: &str, name: Option<&str>, value: &str) {
    let v = w(value);
    let n = name.map(w);
    RegSetKeyValueW(
        HKEY_CURRENT_USER,
        w(sub).as_ptr(),
        n.as_ref().map_or(ptr::null(), |n| n.as_ptr()),
        REG_SZ,
        v.as_ptr() as *const _,
        (v.len() * 2) as u32,
    );
}

/// Makes Windows hand dockodile:// links to this program, for the current user only.
unsafe fn register_protocol() {
    let exe = String::from_utf16_lossy(&own_path());
    reg_set("Software\\Classes\\dockodile", None, "URL:Dockodile");
    reg_set("Software\\Classes\\dockodile", Some("URL Protocol"), "");
    reg_set("Software\\Classes\\dockodile\\shell\\open\\command", None, &format!("\"{}\" \"%1\"", exe));
}

/// WM_COPYDATA payload, as Windows defines it.
#[repr(C)]
struct CopyData {
    kind: usize,
    size: u32,
    data: *const core::ffi::c_void,
}

// ---------------------------------------------------------------------------------------------
// Tray icon, autostart, start-up
// ---------------------------------------------------------------------------------------------

fn put(dst: &mut [u16], s: &str) {
    let v: Vec<u16> = s.encode_utf16().collect();
    let n = v.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&v[..n]);
    dst[n] = 0;
}

unsafe fn tray(add: bool, balloon: bool) {
    let a = app();
    let mut d: NOTIFYICONDATAW = mem::zeroed();
    d.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    d.hWnd = a.msg;
    d.uID = 1;
    if !add {
        Shell_NotifyIconW(NIM_DELETE, &d);
        return;
    }
    d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    d.uCallbackMessage = WM_TRAY;
    d.hIcon = LoadIconW(a.inst, 1 as *const u16);
    if d.hIcon.is_null() {
        d.hIcon = LoadIconW(ptr::null_mut(), IDI_APPLICATION);
    }
    put(&mut d.szTip, "Dockodile");
    if balloon {
        d.uFlags |= NIF_INFO;
        put(&mut d.szInfoTitle, tr("Dockodile läuft", "Dockodile is running"));
        put(
            &mut d.szInfo,
            tr(
                "Fenster an der Titelleiste auf den oberen Rand eines anderen ziehen.",
                "Drag a window by its title bar onto the top edge of another one.",
            ),
        );
        d.dwInfoFlags = NIIF_USER;
    }
    Shell_NotifyIconW(NIM_ADD, &d);
}

unsafe fn own_path() -> Vec<u16> {
    let mut buf = [0u16; 1024];
    let n = windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW(
        ptr::null_mut(),
        buf.as_mut_ptr(),
        buf.len() as u32,
    ) as usize;
    buf[..n].to_vec()
}

unsafe fn autostart() -> bool {
    let mut size = 0u32;
    RegGetValueW(
        HKEY_CURRENT_USER,
        w(RUN_KEY).as_ptr(),
        w("Dockodile").as_ptr(),
        RRF_RT_REG_SZ,
        ptr::null_mut(),
        ptr::null_mut(),
        &mut size,
    ) == ERROR_SUCCESS
}

unsafe fn set_autostart(on: bool) {
    let mut key: HKEY = ptr::null_mut();
    if RegOpenKeyExW(HKEY_CURRENT_USER, w(RUN_KEY).as_ptr(), 0, KEY_SET_VALUE, &mut key) != ERROR_SUCCESS {
        return;
    }
    if on {
        let mut value: Vec<u16> = vec![b'"' as u16];
        value.extend(own_path());
        value.push(b'"' as u16);
        value.push(0);
        RegSetValueExW(
            key,
            w("Dockodile").as_ptr(),
            0,
            REG_SZ,
            value.as_ptr() as *const u8,
            (value.len() * 2) as u32,
        );
    } else {
        RegDeleteValueW(key, w("Dockodile").as_ptr());
    }
    RegCloseKey(key);
}

unsafe fn tray_menu() {
    let a = app();
    let auto = autostart();
    let add = |m: HMENU, flags: u32, id: usize, label: &str| {
        AppendMenuW(m, flags, id, w(label).as_ptr());
    };
    let menu = CreatePopupMenu();
    let about = if EN { format!("Dockodile {VERSION} for Windows") } else { format!("Dockodile {VERSION} für Windows") };
    add(menu, MF_STRING | MF_GRAYED, 0, &about);
    add(menu, MF_SEPARATOR, 0, "");
    if a.store.packages.is_empty() {
        add(menu, MF_STRING | MF_GRAYED, 0, tr("Noch kein Paket gesichert", "No package saved yet"));
    }
    for (i, p) in a.store.packages.iter().enumerate().take(80) {
        let sub = CreatePopupMenu();
        let base = 100 + i * 10;
        add(sub, MF_STRING, base, tr("Öffnen", "Open"));
        add(sub, MF_STRING, base + 1, tr("Kürzel ändern …", "Change shortcut …"));
        if p.hotkey.is_some() {
            add(sub, MF_STRING, base + 2, tr("Kürzel entfernen", "Remove shortcut"));
        }
        add(sub, MF_STRING, base + 3, tr("Paket löschen", "Delete package"));
        let mut label = p.name.replace('&', "&&");
        if let Some((m, k)) = p.hotkey {
            label = format!("{}\t{}", label, store::hotkey_name(m, k, EN));
        }
        add(menu, MF_POPUP, sub as usize, &label);
    }
    add(menu, MF_SEPARATOR, 0, "");
    add(menu, MF_STRING, 1, tr("Alle Gruppen auflösen", "Dissolve all groups"));
    add(menu, MF_STRING | if auto { MF_CHECKED } else { 0 }, 2, tr("Mit Windows starten", "Start with Windows"));
    let lang = CreatePopupMenu();
    add(lang, MF_STRING | if EN { 0 } else { MF_CHECKED }, 4, "Deutsch");
    add(lang, MF_STRING | if EN { MF_CHECKED } else { 0 }, 5, "English");
    add(menu, MF_POPUP, lang as usize, tr("Sprache", "Language"));
    if cfg!(feature = "microsoft-store") {
        add(menu, MF_STRING, 7, tr("Im Microsoft Store aktualisieren …", "Update in Microsoft Store …"));
    } else {
    let updates = CreatePopupMenu();
    if let Some(v) = &a.available {
        let label = if EN { format!("Install version {v} …") } else { format!("Version {v} installieren …") };
        add(updates, MF_STRING, 6, &label);
    }
    add(updates, MF_STRING | if a.updating { MF_GRAYED } else { 0 }, 7, tr("Nach Updates suchen", "Check for updates"));
    add(updates, MF_STRING | if a.store.manual_updates { 0 } else { MF_CHECKED }, 8, tr("Automatisch aktualisieren", "Update automatically"));
    if a.store.token.is_none() {
        add(updates, MF_STRING, 10, tr("Updates freischalten …", "Unlock updates …"));
    }
    let updates_label = match &a.available {
        Some(v) if EN => format!("Updates (version {v} is available)"),
        Some(v) => format!("Updates (Version {v} ist da)"),
        None => "Updates".to_string(),
    };
    add(menu, MF_POPUP, updates as usize, &updates_label);
    }
    add(menu, MF_STRING, 3, tr("dockodile.de öffnen", "Open dockodile.de"));
    add(menu, MF_SEPARATOR, 0, "");
    add(menu, MF_STRING, 9, tr("Beenden", "Quit"));
    let cmd = track(menu) as usize;
    match cmd {
        1 => dissolve_all(),
        2 => set_autostart(!auto),
        3 => {
            ShellExecuteW(
                ptr::null_mut(),
                w("open").as_ptr(),
                w("https://dockodile.de").as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            );
        }
        4 | 5 => {
            EN = cmd == 5;
            a.store.lang = Some(if EN { "en" } else { "de" }.to_string());
            save_store();
        }
        6 => offer_update(),
        7 => check_update(true),
        8 => {
            a.store.manual_updates = !a.store.manual_updates;
            save_store();
            schedule_updates();
        }
        10 => open_url(UPDATE_PAGE),
        9 => {
            DestroyWindow(app().msg);
        }
        100.. => {
            let (i, action) = ((cmd - 100) / 10, (cmd - 100) % 10);
            if i >= a.store.packages.len() {
                return;
            }
            match action {
                0 => open_package(i),
                1 => open_picker(Mode::Hotkey(i)),
                2 => {
                    a.store.packages[i].hotkey = None;
                    save_store();
                    rebind_hotkeys();
                }
                3 => {
                    let name = a.store.packages[i].name.clone();
                    let ask = if EN { format!("Delete package “{}”?", name) } else { format!("Paket „{}“ löschen?", name) };
                    if MessageBoxW(a.msg, w(&ask).as_ptr(), w("Dockodile").as_ptr(), MB_YESNO | MB_ICONQUESTION) == IDYES {
                        if let Some(k) = a.store.packages.iter().position(|p| p.name == name) {
                            a.store.packages.remove(k);
                        }
                        for g in a.groups.iter_mut() {
                            if g.package.as_deref() == Some(name.as_str()) {
                                g.package = None;
                            }
                        }
                        save_store();
                        rebind_hotkeys();
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

unsafe extern "system" fn msg_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(h, msg, wp, lp);
    }
    match msg {
        WM_TRAY => {
            let event = (lp & 0xFFFF) as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                tray_menu();
            } else if event == NIN_BALLOONUSERCLICK && app().balloon_update {
                app().balloon_update = false;
                offer_update();
            }
            return 0;
        }
        WM_HOTKEY => {
            if wp >= 1 && wp <= app().hotkeys {
                open_package(wp - 1);
            }
            return 0;
        }
        WM_COPYDATA => {
            // A second start with a dockodile:// link hands it over and exits. The link is
            // handled after this call returns, so the sender does not wait for windows.
            let cd = lp as *const CopyData;
            if !cd.is_null() && (*cd).kind == 1 && !(*cd).data.is_null() && (*cd).size < 8192 {
                let text = std::slice::from_raw_parts((*cd).data as *const u16, (*cd).size as usize / 2);
                app().url = String::from_utf16_lossy(text).trim_end_matches('\0').to_string();
                PostMessageW(h, WM_URL, 0, 0);
            }
            return 1;
        }
        WM_FRAME => {
            FRAME_PENDING.store(false, std::sync::atomic::Ordering::SeqCst);
            if !app().float.is_null() {
                float_tick();
            }
            return 0;
        }
        WM_UPDATE => {
            let result = UPDATE_RESULT.lock().ok().and_then(|mut slot| slot.take());
            app().updating = false;
            match result {
                Some(UpdateResult::Version(v, manual)) => update_found(v, manual),
                Some(UpdateResult::File(f)) => update_downloaded(f),
                None => {}
            }
            return 0;
        }
        WM_URL => {
            let url = mem::take(&mut app().url);
            handle_url(&url);
            return 0;
        }
        WM_TIMER => {
            if wp == TIMER_TICK {
                tick();
            } else if wp == TIMER_UPDATE {
                check_update(false);
            } else if wp == TIMER_FLOAT {
                float_tick();
            } else if wp == TIMER_MOVE && GetAsyncKeyState(VK_LBUTTON as i32) >= 0 {
                end_move();
            }
            return 0;
        }
        WM_DESTROY => {
            dissolve_all();
            tray(false, false);
            PostQuitMessage(0);
            return 0;
        }
        _ => {
            if msg == app().taskbar_created && msg != 0 {
                // Explorer restarted: the icon has to be added again.
                tray(true, false);
                return 0;
            }
        }
    }
    DefWindowProcW(h, msg, wp, lp)
}

unsafe fn register(inst: HINSTANCE, name: &str, proc_: WNDPROC, style: u32) {
    let class = w(name);
    let mut wc: WNDCLASSEXW = mem::zeroed();
    wc.cbSize = mem::size_of::<WNDCLASSEXW>() as u32;
    wc.style = style;
    wc.lpfnWndProc = proc_;
    wc.hInstance = inst;
    wc.hCursor = LoadCursorW(ptr::null_mut(), IDC_ARROW);
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = class.as_ptr();
    RegisterClassExW(&wc);
}

fn main() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let url = std::env::args().skip(1).find(|a| a.to_lowercase().starts_with("dockodile:"));
        let updated = std::env::args().any(|a| a == "--updated");
        let mut mutex = CreateMutexW(ptr::null(), 0, w("Dockodile.Windows.Single").as_ptr());
        // Started by an update: the old version is still closing.
        let mut waited = 0;
        while updated && GetLastError() == ERROR_ALREADY_EXISTS && waited < 50 {
            CloseHandle(mutex);
            Sleep(200);
            waited += 1;
            mutex = CreateMutexW(ptr::null(), 0, w("Dockodile.Windows.Single").as_ptr());
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Already running: pass a link on to that instance.
            if let Some(u) = url {
                let running = FindWindowW(w("DockodileMsg").as_ptr(), ptr::null());
                if !running.is_null() {
                    let text: Vec<u16> = u.encode_utf16().collect();
                    let cd = CopyData { kind: 1, size: (text.len() * 2) as u32, data: text.as_ptr() as *const _ };
                    let mut r = 0usize;
                    SendMessageTimeoutW(running, WM_COPYDATA, 0, &cd as *const CopyData as LPARAM, SMTO_ABORTIFHUNG, 3000, &mut r);
                }
            }
            return;
        }
        let inst = GetModuleHandleW(ptr::null());
        if std::env::args().any(|a| a == "--log") {
            let mut path = String::from_utf16_lossy(&own_path());
            path = path.trim_end_matches(".exe").to_string() + ".log";
            LOG = std::fs::File::create(path).ok();
        }
        register(inst, "DockodileBar", Some(bar_proc), CS_DBLCLKS);
        register(inst, "DockodileHint", Some(hint_proc), 0);
        register(inst, "DockodileMsg", Some(msg_proc), 0);
        register(inst, "DockodilePicker", Some(picker_proc), 0);
        let store = store_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| store::parse(&t))
            .unwrap_or_default();
        EN = match store.lang.as_deref() {
            Some(l) => l == "en",
            // Primary language 0x07 is German.
            None => GetUserDefaultUILanguage() & 0x3FF != 0x07,
        };

        // A hidden ordinary window: it owns the tray icon, the timers and the menus.
        let msg = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w("DockodileMsg").as_ptr(),
            w("Dockodile").as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            inst,
            ptr::null(),
        );
        let hint = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST,
            w("DockodileHint").as_ptr(),
            w("").as_ptr(),
            WS_POPUP,
            0,
            0,
            10,
            10,
            ptr::null_mut(),
            ptr::null_mut(),
            inst,
            ptr::null(),
        );
        let picker = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w("DockodilePicker").as_ptr(),
            w("Dockodile").as_ptr(),
            WS_POPUP | WS_BORDER,
            0,
            0,
            10,
            10,
            ptr::null_mut(),
            ptr::null_mut(),
            inst,
            ptr::null(),
        );
        let child = |class: &str, style: u32, id: usize| {
            CreateWindowExW(0, w(class).as_ptr(), w("").as_ptr(), WS_CHILD | WS_VISIBLE | style, 0, 0, 10, 10, picker, id as HMENU, inst, ptr::null())
        };
        let edit = child("EDIT", ES_AUTOHSCROLL as u32, 1);
        let list = child("LISTBOX", WS_VSCROLL | (LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32, 2);
        if msg.is_null() || hint.is_null() || picker.is_null() || edit.is_null() || list.is_null() {
            return;
        }
        SetLayeredWindowAttributes(hint, 0, 225, LWA_ALPHA);

        APP = Box::into_raw(Box::new(App {
            inst,
            msg,
            hint,
            groups: Vec::new(),
            moving: ptr::null_mut(),
            moving_size: (0, 0),
            moving_by_title: false,
            picker_at: None,
            target: Target::None,
            float: ptr::null_mut(),
            float_off: (0, 0),
            pid: GetCurrentProcessId(),
            taskbar_created: RegisterWindowMessageW(w("TaskbarCreated").as_ptr()),
            store,
            hotkeys: 0,
            mode: Mode::Closed,
            picker,
            edit,
            list,
            picker_font: ptr::null_mut(),
            entries: Vec::new(),
            programs: Vec::new(),
            launch: None,
            opening: None,
            url: String::new(),
            available: None,
            updating: false,
            balloon_update: false,
        }));
        rebind_hotkeys();
        register_protocol();
        let _ = std::fs::remove_file(own_dir().join("dockodile.old.exe"));
        let _ = std::fs::remove_file(own_dir().join("dockodile.new.exe"));
        schedule_updates();

        tray(true, !updated);
        if updated {
            let title = if EN { format!("Dockodile {VERSION} is installed") } else { format!("Dockodile {VERSION} ist installiert") };
            notify(&title, tr("Das Update hat geklappt.", "The update worked."));
        }
        SetTimer(msg, TIMER_TICK, 500, None);
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let system = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_MINIMIZEEND,
            ptr::null_mut(),
            Some(on_event),
            0,
            0,
            flags,
        );
        let object = SetWinEventHook(
            EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_LOCATIONCHANGE,
            ptr::null_mut(),
            Some(on_event),
            0,
            0,
            flags,
        );

        // Development only: group the two topmost windows at once. Wine sends no move
        // events, so this is the only way to exercise the bar there.
        #[cfg(feature = "selftest")]
        {
            let mut found: Vec<HWND> = Vec::new();
            let mut h = GetTopWindow(ptr::null_mut());
            while !h.is_null() {
                log!(
                    "window {:?} \"{}\" class={} visible={} eligible={}",
                    h,
                    String::from_utf16_lossy(&title_of(h)),
                    class_of(h),
                    IsWindowVisible(h),
                    eligible(h, false)
                );
                if eligible(h, false) {
                    found.push(h);
                }
                h = GetWindow(h, GW_HWNDNEXT);
            }
            if found.len() >= 2 {
                create_group(found[1], found[0]);
            }
        }
        log!("Dockodile {} started, hooks {} {}", VERSION, !system.is_null(), !object.is_null());
        if let Some(u) = url {
            handle_url(&u);
        }
        let mut m: MSG = mem::zeroed();
        while GetMessageW(&mut m, ptr::null_mut(), 0, 0) > 0 {
            if picker_key(&m) {
                continue;
            }
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
        UnhookWinEvent(system);
        UnhookWinEvent(object);
    }
}
