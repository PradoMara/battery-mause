#![windows_subsystem = "windows"]
//! MouseBattery: porcentaje de bateria del mouse en la bandeja de Windows.
//! Sin red, sin administrador. Duerme entre lecturas (timer del sistema).

mod a9;
mod reg;

use mousebattery::*;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::Globalization::GetUserDefaultLocaleName;
use windows::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{GUID, PCWSTR, w};

const WM_TRAY: u32 = WM_APP + 1;
const EV_POPUP_OPEN: u32 = 0x406;
const EV_POPUP_CLOSE: u32 = 0x407;
const T_POLL: usize = 1;
const T_BLINK: usize = 2;
const T_PANEL: usize = 3;
const T_DEBOUNCE: usize = 4;

#[derive(Clone, Copy, PartialEq)]
enum Link {
    Wireless,
    Cable,
    Gone,
}

#[derive(Clone)]
struct State {
    lang: Lang,
    interval: u32,
    blink: bool,
    info: u32, // bit0 estado, bit1 nombre, bit2 conexion, bit3 ultima lectura
    percent: Option<u8>,
    link: Link,
    last_ok: Option<Instant>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State {
        lang: Lang::Es, interval: DEFAULT_INTERVAL, blink: true, info: 0b0111,
        percent: None, link: Link::Gone, last_ok: None,
    });
    static ALERTS: RefCell<AlertState> = RefCell::new(AlertState::default());
    static START: Instant = Instant::now();
    static MAIN: Cell<isize> = const { Cell::new(0) };
    static PANEL: Cell<isize> = const { Cell::new(0) };
    static HICON_CUR: Cell<isize> = const { Cell::new(0) };
    static TASKBAR_MSG: Cell<u32> = const { Cell::new(0) };
}

fn now_ms() -> u64 {
    START.with(|s| s.elapsed().as_millis() as u64)
}
fn st() -> State {
    ST.with(|s| s.borrow().clone())
}
fn hwnd_of(c: &'static std::thread::LocalKey<Cell<isize>>) -> HWND {
    HWND(c.with(|v| v.get()) as *mut c_void)
}
fn light_theme() -> bool {
    reg::get_dword(reg::THEME_KEY, "SystemUsesLightTheme") == Some(1)
}

// ------------------------------------------------------------ configuracion

fn load_settings() {
    let mut loc = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(&mut loc) } as usize;
    let sys = Lang::from_locale(&String::from_utf16_lossy(&loc[..n.saturating_sub(1)]));
    ST.with(|s| {
        let mut s = s.borrow_mut();
        s.lang = match reg::get_dword(reg::APP_KEY, "Lang") {
            Some(1) => Lang::Es,
            Some(2) => Lang::En,
            _ => sys,
        };
        if let Some(i) = reg::get_dword(reg::APP_KEY, "Interval").filter(|i| INTERVALS.contains(i)) {
            s.interval = i;
        }
        if let Some(b) = reg::get_dword(reg::APP_KEY, "Blink") {
            s.blink = b != 0;
        }
        if let Some(m) = reg::get_dword(reg::APP_KEY, "Info") {
            s.info = m & 0xF;
        }
    });
    // Primer arranque: registrar el inicio con Windows (se puede quitar en el
    // Administrador de tareas o desde el menu).
    if reg::get_dword(reg::APP_KEY, "FirstRun").is_none() {
        set_autostart(true);
        reg::set_dword(reg::APP_KEY, "FirstRun", 1);
    }
}

fn set_autostart(on: bool) {
    if on {
        if let Ok(p) = std::env::current_exe() {
            reg::set_string(reg::RUN_KEY, reg::RUN_NAME, &format!("\"{}\"", p.display()));
            reg::delete_value(reg::APPROVED_KEY, reg::RUN_NAME);
        }
    } else {
        reg::delete_value(reg::RUN_KEY, reg::RUN_NAME);
        reg::delete_value(reg::APPROVED_KEY, reg::RUN_NAME);
    }
}

// ------------------------------------------------------------ lectura

fn poll() {
    let probe = a9::read();
    let now = now_ms();
    let mut open_panel = None;
    ST.with(|s| {
        let mut s = s.borrow_mut();
        ALERTS.with(|a| {
            let mut a = a.borrow_mut();
            match probe {
                a9::Probe::Battery(p) => {
                    s.percent = Some(p);
                    s.link = Link::Wireless;
                    s.last_ok = Some(Instant::now());
                    open_panel = a.on_reading(now, p, false);
                }
                a9::Probe::Cable => {
                    s.link = Link::Cable;
                    a.on_cable();
                }
                a9::Probe::Absent => s.link = Link::Gone,
            }
        });
    });
    refresh_icon(false);
    restart_poll_timer();
    if open_panel.is_some() {
        show_panel(Some(8000));
    }
}

fn blinking() -> bool {
    ALERTS.with(|a| a.borrow().is_blinking(now_ms()))
}

// ------------------------------------------------------------ icono de bandeja

unsafe fn make_icon(px: &[u32; ICON * ICON]) -> HICON {
    unsafe {
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: ICON as i32,
                biHeight: -(ICON as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let hdc = GetDC(None);
        let color = CreateDIBSection(Some(hdc), &bi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u32, ICON * ICON);
        let mask = CreateBitmap(ICON as i32, ICON as i32, 1, 1, None);
        let ii = ICONINFO { fIcon: TRUE, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&ii).unwrap();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        ReleaseDC(None, hdc);
        icon
    }
}

fn tip_text(s: &State) -> String {
    let l = s.lang;
    let pct = s.percent.map_or("?".to_string(), |p| format!("{p} %"));
    match s.link {
        Link::Wireless => format!("{}: {pct}", t(l, K::Battery)),
        Link::Cable => format!("{}: {pct} · {}", t(l, K::Battery), t(l, K::Charging)),
        Link::Gone => format!("{}: {pct} · {}", t(l, K::Battery), t(l, K::Disconnected)),
    }
}

fn notify_data(hwnd: HWND, icon: HICON, tip: &str) -> NOTIFYICONDATAW {
    let mut n = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (i, c) in tip.encode_utf16().take(n.szTip.len() - 1).enumerate() {
        n.szTip[i] = c;
    }
    n
}

fn build_icon(s: &State) -> HICON {
    let text = if s.link == Link::Gone {
        color(0x80, 0x80, 0x80)
    } else if light_theme() {
        color(0x10, 0x10, 0x10)
    } else {
        color(0xFF, 0xFF, 0xFF)
    };
    let overlay = match s.link {
        Link::Cable => Overlay::Bolt,
        Link::Gone => Overlay::Cross,
        Link::Wireless => Overlay::None,
    };
    let badge = ALERTS.with(|a| a.borrow().badge(now_ms(), s.percent, s.link == Link::Cable, s.blink));
    unsafe { make_icon(&render_icon(s.percent, text, badge, overlay)) }
}

fn refresh_icon(add: bool) {
    let s = st();
    let hwnd = hwnd_of(&MAIN);
    let icon = build_icon(&s);
    let mut n = notify_data(hwnd, icon, &tip_text(&s));
    unsafe {
        if add {
            let _ = Shell_NotifyIconW(NIM_ADD, &n);
            n.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &n);
        } else {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &n);
        }
        let old = HICON_CUR.with(|c| c.replace(icon.0 as isize));
        if old != 0 {
            let _ = DestroyIcon(HICON(old as *mut c_void));
        }
        // Timer de parpadeo solo mientras haga falta.
        if blinking() {
            SetTimer(Some(hwnd), T_BLINK, 500, None);
        } else {
            let _ = KillTimer(Some(hwnd), T_BLINK);
        }
    }
}

/// Sin lectura (mouse dormido o receptor ausente) reintenta cada 30 s como
/// maximo, sin esperar el intervalo completo.
fn restart_poll_timer() {
    let s = st();
    let secs = if s.link == Link::Gone { s.interval.min(30) } else { s.interval };
    unsafe {
        SetTimer(Some(hwnd_of(&MAIN)), T_POLL, secs * 1000, None);
    }
}

// ------------------------------------------------------------ panel emergente

fn panel_rows(s: &State) -> Vec<(String, String)> {
    let l = s.lang;
    let mut rows = Vec::new();
    if s.info & 1 != 0 {
        let mut v = match s.link {
            Link::Wireless => t(l, K::Connected).to_string(),
            Link::Cable => t(l, K::Charging).to_string(),
            Link::Gone => t(l, K::Disconnected).to_string(),
        };
        if s.link != Link::Wireless && s.percent.is_some() {
            v.push_str(&format!(" · {}", t(l, K::Stale)));
        }
        rows.push((t(l, K::State).to_string(), v));
    }
    if s.info & 2 != 0 {
        rows.push((t(l, K::Name).to_string(), a9::NAME.to_string()));
    }
    if s.info & 4 != 0 {
        let v = match s.link {
            Link::Wireless => "2.4 GHz",
            Link::Cable => "USB",
            Link::Gone => "—",
        };
        rows.push((t(l, K::Connection).to_string(), v.to_string()));
    }
    if s.info & 8 != 0 {
        let v = match s.last_ok {
            None => t(l, K::Unknown).to_string(),
            Some(i) => match i.elapsed().as_secs() / 60 {
                0 => t(l, K::JustNow).to_string(),
                m => format!("{m} {}", t(l, K::MinAgo)),
            },
        };
        rows.push((t(l, K::LastRead).to_string(), v));
    }
    rows
}

fn dpi_scale() -> f32 {
    unsafe {
        let hdc = GetDC(None);
        let d = GetDeviceCaps(Some(hdc), LOGPIXELSX);
        ReleaseDC(None, hdc);
        d as f32 / 96.0
    }
}

fn show_panel(auto_close_ms: Option<u32>) {
    let s = st();
    let sc = dpi_scale();
    let rows = panel_rows(&s).len() as f32;
    let (w_px, h_px) = ((280.0 * sc) as i32, ((84.0 + rows * 24.0 + 12.0) * sc) as i32);
    let main = hwnd_of(&MAIN);
    unsafe {
        let mut rect = RECT::default();
        let id = NOTIFYICONIDENTIFIER { cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32, hWnd: main, uID: 1, guidItem: GUID::zeroed() };
        if let Ok(r) = Shell_NotifyIconGetRect(&id) {
            rect = r;
        }
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut work as *mut RECT as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let cx = (rect.left + rect.right) / 2;
        let x = (cx - w_px / 2).clamp(work.left + 8, (work.right - w_px - 8).max(work.left));
        let y = if rect.top > work.bottom - 4 || rect.top == 0 { work.bottom - h_px - 12 } else { (rect.top - h_px - 12).max(work.top) };

        let mut p = hwnd_of(&PANEL);
        if p.0.is_null() {
            let hinst = GetModuleHandleW(None).unwrap();
            p = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                w!("MouseBatteryPanel"),
                w!(""),
                WS_POPUP,
                x, y, w_px, h_px,
                None, None, Some(hinst.into()), None,
            )
            .unwrap();
            PANEL.with(|c| c.set(p.0 as isize));
            let round: u32 = 2; // DWMWCP_ROUND
            let _ = DwmSetWindowAttribute(p, windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(33), &round as *const u32 as *const c_void, 4);
        }
        let _ = SetWindowPos(p, Some(HWND_TOPMOST), x, y, w_px, h_px, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let _ = InvalidateRect(Some(p), None, true);
        match auto_close_ms {
            Some(ms) => {
                SetTimer(Some(main), T_PANEL, ms, None);
            }
            None => {
                let _ = KillTimer(Some(main), T_PANEL);
            }
        }
    }
}

fn hide_panel() {
    let p = PANEL.with(|c| c.replace(0));
    unsafe {
        let _ = KillTimer(Some(hwnd_of(&MAIN)), T_PANEL);
        if p != 0 {
            let _ = DestroyWindow(HWND(p as *mut c_void));
        }
    }
}

fn rgb(c: u32) -> COLORREF {
    COLORREF(((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF))
}

unsafe fn draw_text(hdc: HDC, text: &str, rect: &mut RECT, flags: DRAW_TEXT_FORMAT) {
    unsafe {
        let mut v: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(hdc, &mut v, rect, flags | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
    }
}

unsafe fn font(h: i32, weight: i32) -> HFONT {
    unsafe {
        CreateFontW(-h, 0, 0, 0, weight, 0, 0, 0, FONT_CHARSET(1), FONT_OUTPUT_PRECISION(0), FONT_CLIP_PRECISION(0), FONT_QUALITY(5), 0, w!("Segoe UI"))
    }
}

unsafe fn paint_panel(hwnd: HWND) {
    unsafe {
        let s = st();
        let sc = dpi_scale();
        let u = |v: f32| (v * sc) as i32;
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let light = light_theme();
        let (bg, fg, dim, track) = if light { (0xF3F3F3, 0x1A1A1A, 0x6B6B6B, 0xD0D0D0) } else { (0x202020, 0xF5F5F5, 0x9A9A9A, 0x3A3A3A) };
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let br = CreateSolidBrush(rgb(bg));
        FillRect(hdc, &rc, br);
        let _ = DeleteObject(br.into());
        SetBkMode(hdc, TRANSPARENT);

        // Porcentaje grande
        let big = font(u(34.0), 600);
        let old = SelectObject(hdc, big.into());
        SetTextColor(hdc, rgb(fg));
        let txt = s.percent.map_or("—".to_string(), |p| format!("{p} %"));
        let mut r = RECT { left: u(16.0), top: u(10.0), right: rc.right - u(16.0), bottom: u(58.0) };
        draw_text(hdc, &txt, &mut r, DT_LEFT);
        let small = font(u(14.0), 400);
        SelectObject(hdc, small.into());
        SetTextColor(hdc, rgb(dim));
        draw_text(hdc, t(s.lang, K::Battery), &mut r, DT_RIGHT);

        // Barra
        let bar = RECT { left: u(16.0), top: u(62.0), right: rc.right - u(16.0), bottom: u(68.0) };
        let tb = CreateSolidBrush(rgb(track));
        FillRect(hdc, &bar, tb);
        let _ = DeleteObject(tb.into());
        if let Some(p) = s.percent {
            let col = if p <= 10 { 0xE52D2D } else if p <= 30 { 0xFFC400 } else { 0x3CB371 };
            let fill = RECT { right: bar.left + (bar.right - bar.left) * p as i32 / 100, ..bar };
            let fb = CreateSolidBrush(rgb(col));
            FillRect(hdc, &fill, fb);
            let _ = DeleteObject(fb.into());
        }

        // Filas
        let mut y = 76.0;
        for (label, value) in panel_rows(&s) {
            let mut lr = RECT { left: u(16.0), top: u(y), right: rc.right - u(16.0), bottom: u(y + 24.0) };
            SetTextColor(hdc, rgb(dim));
            draw_text(hdc, &label, &mut lr, DT_LEFT);
            SetTextColor(hdc, rgb(fg));
            draw_text(hdc, &value, &mut lr, DT_RIGHT);
            y += 24.0;
        }
        SelectObject(hdc, old);
        let _ = DeleteObject(big.into());
        let _ = DeleteObject(small.into());
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                paint_panel(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

// ------------------------------------------------------------ menu

const ID_INTERVAL: usize = 100; // +indice
const ID_INFO: usize = 200; // +bit
const ID_BLINK: usize = 210;
const ID_AUTOSTART: usize = 211;
const ID_REFRESH: usize = 212;
const ID_EXIT: usize = 213;
const ID_LANG_ES: usize = 220;
const ID_LANG_EN: usize = 221;

fn text16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn add(menu: HMENU, checked: bool, id: usize, text: &str) {
    unsafe {
        let f = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, f, id, PCWSTR(text16(text).as_ptr()));
    }
}

unsafe fn add_sub(menu: HMENU, sub: HMENU, text: &str) {
    unsafe {
        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(text16(text).as_ptr()));
    }
}

fn show_menu(hwnd: HWND) {
    let s = st();
    let l = s.lang;
    unsafe {
        let m = CreatePopupMenu().unwrap();
        let iv = CreatePopupMenu().unwrap();
        for (i, secs) in INTERVALS.iter().enumerate() {
            add(iv, *secs == s.interval, ID_INTERVAL + i, &interval_label(l, *secs));
        }
        add_sub(m, iv, t(l, K::Interval));
        let info = CreatePopupMenu().unwrap();
        let names = [K::State, K::Name, K::Connection, K::LastRead];
        for (bit, k) in names.iter().enumerate() {
            add(info, s.info >> bit & 1 == 1, ID_INFO + bit, t(l, *k));
        }
        add_sub(m, info, t(l, K::Info));
        add(m, s.blink, ID_BLINK, t(l, K::Blink));
        add(m, reg::autostart_enabled(), ID_AUTOSTART, t(l, K::Autostart));
        let lg = CreatePopupMenu().unwrap();
        add(lg, l == Lang::Es, ID_LANG_ES, "Español");
        add(lg, l == Lang::En, ID_LANG_EN, "English");
        add_sub(m, lg, t(l, K::Language));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        add(m, false, ID_REFRESH, t(l, K::Refresh));
        add(m, false, ID_EXIT, t(l, K::Exit));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTALIGN | TPM_BOTTOMALIGN, pt.x, pt.y, None, hwnd, None).0 as usize;
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
        if cmd != 0 {
            handle_command(hwnd, cmd);
        }
    }
}

fn handle_command(hwnd: HWND, cmd: usize) {
    match cmd {
        c if (ID_INTERVAL..ID_INTERVAL + INTERVALS.len()).contains(&c) => {
            let secs = INTERVALS[c - ID_INTERVAL];
            ST.with(|s| s.borrow_mut().interval = secs);
            reg::set_dword(reg::APP_KEY, "Interval", secs);
            restart_poll_timer();
        }
        c if (ID_INFO..ID_INFO + 4).contains(&c) => {
            let m = ST.with(|s| {
                let mut s = s.borrow_mut();
                s.info ^= 1 << (c - ID_INFO);
                s.info
            });
            reg::set_dword(reg::APP_KEY, "Info", m);
        }
        ID_BLINK => {
            let b = ST.with(|s| {
                let mut s = s.borrow_mut();
                s.blink = !s.blink;
                s.blink
            });
            reg::set_dword(reg::APP_KEY, "Blink", b as u32);
            refresh_icon(false);
        }
        ID_AUTOSTART => set_autostart(!reg::autostart_enabled()),
        ID_LANG_ES | ID_LANG_EN => {
            let (lang, v) = if cmd == ID_LANG_ES { (Lang::Es, 1) } else { (Lang::En, 2) };
            ST.with(|s| s.borrow_mut().lang = lang);
            reg::set_dword(reg::APP_KEY, "Lang", v);
            refresh_icon(false);
        }
        ID_REFRESH => poll(),
        ID_EXIT => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        _ => {}
    }
}

// ------------------------------------------------------------ ventana principal

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TRAY => {
                match (lp.0 as u32) & 0xFFFF {
                    EV_POPUP_OPEN => show_panel(None),
                    EV_POPUP_CLOSE => hide_panel(),
                    WM_CONTEXTMENU | WM_RBUTTONUP => {
                        hide_panel();
                        show_menu(hwnd);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_TIMER => {
                match wp.0 {
                    T_POLL => poll(),
                    T_BLINK => refresh_icon(false),
                    T_PANEL => hide_panel(),
                    T_DEBOUNCE => {
                        let _ = KillTimer(Some(hwnd), T_DEBOUNCE);
                        poll();
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            // Cambio de dispositivos (receptor/cable) o reanudar de suspension: leer enseguida.
            WM_DEVICECHANGE if wp.0 == 0x0007 => {
                SetTimer(Some(hwnd), T_DEBOUNCE, 2500, None);
                LRESULT(0)
            }
            WM_POWERBROADCAST if wp.0 == 0x12 || wp.0 == 0x7 => {
                SetTimer(Some(hwnd), T_DEBOUNCE, 4000, None);
                LRESULT(1)
            }
            WM_SETTINGCHANGE => {
                refresh_icon(false);
                LRESULT(0)
            }
            WM_DESTROY => {
                let n = notify_data(hwnd, HICON::default(), "");
                let _ = Shell_NotifyIconW(NIM_DELETE, &n);
                PostQuitMessage(0);
                LRESULT(0)
            }
            m if m == TASKBAR_MSG.with(|c| c.get()) && m != 0 => {
                refresh_icon(true);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn main() {
    unsafe {
        let _ = SetProcessDPIAware();
        // Una sola instancia.
        let _m = CreateMutexW(None, false, w!("Local\\MouseBatterySingleton"));
        if windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        now_ms(); // fija el origen del reloj
        load_settings();
        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap().into();

        let wc = WNDCLASSW { lpfnWndProc: Some(main_proc), hInstance: hinst, lpszClassName: w!("MouseBatteryMain"), ..Default::default() };
        RegisterClassW(&wc);
        let pc = WNDCLASSW {
            lpfnWndProc: Some(panel_proc),
            hInstance: hinst,
            lpszClassName: w!("MouseBatteryPanel"),
            style: CS_DROPSHADOW,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            ..Default::default()
        };
        RegisterClassW(&pc);

        // Ventana oculta de nivel superior (recibe broadcasts de energia/dispositivos).
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW, w!("MouseBatteryMain"), w!("MouseBattery"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), None).unwrap();
        MAIN.with(|c| c.set(hwnd.0 as isize));
        TASKBAR_MSG.with(|c| c.set(RegisterWindowMessageW(w!("TaskbarCreated"))));

        poll_initial();
        restart_poll_timer();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn poll_initial() {
    refresh_icon(true);
    poll();
}
