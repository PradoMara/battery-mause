//! Logica pura (sin Windows): umbrales, parpadeo, icono en pixeles e idioma.
//! Todo aqui es probable con `cargo test` sin tener el mouse conectado.

/// Umbrales de aviso, de menor a mayor gravedad al bajar.
pub const THRESHOLDS: [u8; 3] = [30, 20, 10];
/// Duracion del parpadeo amarillo (30 % y 20 %).
pub const BLINK_MS: u64 = 10_000;
/// Intervalos de lectura ofrecidos en el menu (segundos).
pub const INTERVALS: [u32; 6] = [30, 60, 300, 900, 1800, 3600];
pub const DEFAULT_INTERVAL: u32 = 300;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Badge {
    None,
    Yellow,
    Red,
}

/// Devuelve el umbral mas grave que se acaba de cruzar hacia abajo.
/// Sin lectura previa solo avisa si ya esta en o bajo un umbral.
pub fn crossed(prev: Option<u8>, new: u8) -> Option<u8> {
    THRESHOLDS
        .iter()
        .rev()
        .copied()
        .find(|&t| new <= t && prev.is_none_or(|p| p > t))
}

#[derive(Default, Debug)]
pub struct AlertState {
    last: Option<u8>,
    blink_until: u64,
}

impl AlertState {
    /// Registra una lectura. Devuelve el umbral cruzado, si hay que abrir el panel.
    pub fn on_reading(&mut self, now_ms: u64, percent: u8, charging: bool) -> Option<u8> {
        if charging {
            self.last = Some(percent);
            self.blink_until = 0;
            return None;
        }
        let c = crossed(self.last, percent);
        self.last = Some(percent);
        if let Some(t) = c {
            if t > 10 {
                self.blink_until = now_ms + BLINK_MS;
            } else {
                self.blink_until = 0;
            }
        }
        c
    }

    /// El cable conectado apaga cualquier aviso.
    pub fn on_cable(&mut self) {
        self.blink_until = 0;
    }

    pub fn is_blinking(&self, now_ms: u64) -> bool {
        now_ms < self.blink_until
    }

    /// Circulo a dibujar ahora. `blink_enabled=false` deja el amarillo fijo.
    pub fn badge(&self, now_ms: u64, percent: Option<u8>, charging: bool, blink_enabled: bool) -> Badge {
        if charging {
            return Badge::None;
        }
        if percent.is_some_and(|p| p <= 10) {
            return Badge::Red;
        }
        if self.is_blinking(now_ms) {
            let on = !blink_enabled || (now_ms / 500) % 2 == 0;
            return if on { Badge::Yellow } else { Badge::None };
        }
        Badge::None
    }
}

// ---------------------------------------------------------------- icono

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    None,
    Bolt,
    Cross,
}

pub const ICON: usize = 32;
const SCALE: usize = 5;

const DIGITS: [[u8; 5]; 11] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
    [0b111, 0b001, 0b010, 0b000, 0b010], // '?'
];

const BOLT: [&str; 12] = [
    "....###",
    "...###.",
    "..###..",
    ".###...",
    "#######",
    ".....##",
    "....##.",
    "...##..",
    "..##...",
    ".##....",
    ".#.....",
    "#......",
];

pub fn color(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}
pub const YELLOW: u32 = 0xFFFF_C400;
pub const RED: u32 = 0xFFE5_2D2D;

fn put(buf: &mut [u32; ICON * ICON], x: i32, y: i32, c: u32) {
    if (0..ICON as i32).contains(&x) && (0..ICON as i32).contains(&y) {
        buf[y as usize * ICON + x as usize] = c;
    }
}

fn draw_digit(buf: &mut [u32; ICON * ICON], glyph: usize, x0: i32, y0: i32, c: u32) {
    for (row, bits) in DIGITS[glyph].iter().enumerate() {
        for col in 0..3 {
            if bits >> (2 - col) & 1 == 1 {
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        put(buf, x0 + (col * SCALE + dx) as i32, y0 + (row * SCALE + dy) as i32, c);
                    }
                }
            }
        }
    }
}

/// Dibuja el icono 32x32 ARGB. `percent=None` dibuja "?". 100 % se muestra como "99".
pub fn render_icon(percent: Option<u8>, text: u32, badge: Badge, overlay: Overlay) -> [u32; ICON * ICON] {
    let mut buf = [0u32; ICON * ICON];
    match percent {
        Some(p) => {
            let p = p.min(99) as usize;
            if p >= 10 {
                draw_digit(&mut buf, p / 10, 0, 3, text);
                draw_digit(&mut buf, p % 10, 17, 3, text);
            } else {
                draw_digit(&mut buf, p, 8, 3, text);
            }
        }
        None => draw_digit(&mut buf, 10, 8, 3, text),
    }
    match overlay {
        Overlay::Bolt => {
            for (y, row) in BOLT.iter().enumerate() {
                for (x, ch) in row.bytes().enumerate() {
                    if ch == b'#' {
                        put(&mut buf, 22 + x as i32, 19 + y as i32 - 1, YELLOW);
                    }
                }
            }
        }
        Overlay::Cross => {
            for i in 0..10 {
                for t in 0..2 {
                    put(&mut buf, 21 + i + t, 21 + i, RED);
                    put(&mut buf, 21 + 9 - i + t, 21 + i, RED);
                }
            }
        }
        Overlay::None => {}
    }
    let dot = match badge {
        Badge::Yellow => Some(YELLOW),
        Badge::Red => Some(RED),
        Badge::None => None,
    };
    if let Some(c) = dot {
        let (cx, cy) = (25.0f32, 25.0f32);
        for y in 0..ICON {
            for x in 0..ICON {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                if d <= 6.5 {
                    buf[y * ICON + x] = if d > 5.2 { 0xFF10_1010 } else { c };
                }
            }
        }
    }
    buf
}

// ---------------------------------------------------------------- idioma

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    /// Espanol salvo que Windows este en ingles.
    pub fn from_locale(name: &str) -> Lang {
        if name.to_ascii_lowercase().starts_with("en") { Lang::En } else { Lang::Es }
    }
}

#[derive(Clone, Copy)]
pub enum K {
    Battery,
    State,
    Name,
    Connection,
    LastRead,
    Charging,
    Connected,
    Disconnected,
    Stale,
    Interval,
    Info,
    Blink,
    Autostart,
    Refresh,
    Exit,
    Language,
    JustNow,
    MinAgo,
    Unknown,
}

pub fn t(l: Lang, k: K) -> &'static str {
    use K::*;
    match (l, k) {
        (Lang::Es, Battery) => "Batería",
        (Lang::En, Battery) => "Battery",
        (Lang::Es, State) => "Estado",
        (Lang::En, State) => "Status",
        (Lang::Es, Name) => "Mouse",
        (Lang::En, Name) => "Mouse",
        (Lang::Es, Connection) => "Conexión",
        (Lang::En, Connection) => "Connection",
        (Lang::Es, LastRead) => "Última lectura",
        (Lang::En, LastRead) => "Last read",
        (Lang::Es, Charging) => "Cargando (cable)",
        (Lang::En, Charging) => "Charging (cable)",
        (Lang::Es, Connected) => "Conectado",
        (Lang::En, Connected) => "Connected",
        (Lang::Es, Disconnected) => "Desconectado",
        (Lang::En, Disconnected) => "Disconnected",
        (Lang::Es, Stale) => "dato antiguo",
        (Lang::En, Stale) => "stale data",
        (Lang::Es, Interval) => "Intervalo de lectura",
        (Lang::En, Interval) => "Read interval",
        (Lang::Es, Info) => "Información del panel",
        (Lang::En, Info) => "Panel information",
        (Lang::Es, Blink) => "Parpadeo de aviso",
        (Lang::En, Blink) => "Warning blink",
        (Lang::Es, Autostart) => "Iniciar con Windows",
        (Lang::En, Autostart) => "Start with Windows",
        (Lang::Es, Refresh) => "Actualizar ahora",
        (Lang::En, Refresh) => "Refresh now",
        (Lang::Es, Exit) => "Salir",
        (Lang::En, Exit) => "Exit",
        (Lang::Es, Language) => "Idioma",
        (Lang::En, Language) => "Language",
        (Lang::Es, JustNow) => "ahora",
        (Lang::En, JustNow) => "just now",
        (Lang::Es, MinAgo) => "min",
        (Lang::En, MinAgo) => "min ago",
        (Lang::Es, Unknown) => "sin datos",
        (Lang::En, Unknown) => "no data",
    }
}

pub fn interval_label(l: Lang, secs: u32) -> String {
    match (secs, l) {
        (s, Lang::Es) if s < 60 => format!("{s} segundos"),
        (s, Lang::En) if s < 60 => format!("{s} seconds"),
        (60, Lang::Es) => "1 minuto".into(),
        (60, Lang::En) => "1 minute".into(),
        (3600, Lang::Es) => "1 hora".into(),
        (3600, Lang::En) => "1 hour".into(),
        (s, Lang::Es) => format!("{} minutos", s / 60),
        (s, Lang::En) => format!("{} minutes", s / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_down_picks_most_severe() {
        assert_eq!(crossed(Some(31), 30), Some(30));
        assert_eq!(crossed(Some(30), 29), None);
        assert_eq!(crossed(Some(21), 20), Some(20));
        assert_eq!(crossed(Some(11), 10), Some(10));
        assert_eq!(crossed(Some(40), 9), Some(10)); // salto grande: solo el mas grave
        assert_eq!(crossed(None, 55), None);
        assert_eq!(crossed(None, 15), Some(20));
        assert_eq!(crossed(Some(10), 9), None);
    }

    #[test]
    fn blink_30_and_20_for_ten_seconds() {
        let mut a = AlertState::default();
        assert_eq!(a.on_reading(0, 31, false), None);
        assert_eq!(a.on_reading(1_000, 30, false), Some(30));
        assert!(a.is_blinking(1_000 + BLINK_MS - 1));
        assert!(!a.is_blinking(1_000 + BLINK_MS));
        assert_eq!(a.badge(1_000, Some(30), false, true), Badge::Yellow);
        assert_eq!(a.badge(1_500, Some(30), false, true), Badge::None);
        // sin parpadeo: fijo mientras dure la ventana
        assert_eq!(a.badge(1_500, Some(30), false, false), Badge::Yellow);
        assert_eq!(a.badge(1_000 + BLINK_MS, Some(30), false, true), Badge::None);
    }

    #[test]
    fn ten_percent_is_solid_red_until_charging() {
        let mut a = AlertState::default();
        assert_eq!(a.on_reading(0, 31, false), None);
        assert_eq!(a.on_reading(0, 11, false), Some(20));
        assert_eq!(a.on_reading(1, 10, false), Some(10));
        assert_eq!(a.badge(123_456, Some(10), false, true), Badge::Red);
        assert_eq!(a.badge(123_956, Some(9), false, true), Badge::Red);
        assert_eq!(a.badge(1, Some(9), true, true), Badge::None);
        a.on_cable();
        assert_eq!(a.on_reading(5, 9, true), None);
    }

    #[test]
    fn warns_again_after_charging_recovers() {
        let mut a = AlertState::default();
        assert_eq!(a.on_reading(0, 30, false), Some(30));
        assert_eq!(a.on_reading(1, 60, true), None);
        assert_eq!(a.on_reading(2, 31, false), None);
        assert_eq!(a.on_reading(3, 30, false), Some(30));
    }

    #[test]
    fn icon_draws_pixels_and_caps_at_99() {
        let a = render_icon(Some(100), 0xFFFFFFFF, Badge::None, Overlay::None);
        let b = render_icon(Some(99), 0xFFFFFFFF, Badge::None, Overlay::None);
        assert_eq!(a, b);
        assert!(a.iter().any(|&p| p != 0));
        let with_dot = render_icon(Some(50), 0xFFFFFFFF, Badge::Red, Overlay::None);
        assert!(with_dot.contains(&RED));
        let x = render_icon(Some(50), 0xFF808080, Badge::None, Overlay::Cross);
        assert!(x.contains(&RED));
    }

    #[test]
    fn language_follows_locale() {
        assert_eq!(Lang::from_locale("es-CL"), Lang::Es);
        assert_eq!(Lang::from_locale("en-US"), Lang::En);
        assert_eq!(Lang::from_locale("fr-FR"), Lang::Es);
    }
}
