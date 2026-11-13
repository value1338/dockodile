//! Settings and packages on disk, and the pure helpers around them. No Windows calls here,
//! so `rustc --test src/store.rs` checks this file on any system.
#![allow(dead_code)]

pub const MOD_ALT: u32 = 1;
pub const MOD_CONTROL: u32 = 2;
pub const MOD_SHIFT: u32 = 4;
pub const MOD_WIN: u32 = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct PTab {
    /// Full path of the program, used to find its window again and to start it.
    pub exe: String,
    /// Window title when the package was saved; tells two windows of one program apart.
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Package {
    pub name: String,
    /// Modifiers (MOD_*) and virtual key.
    pub hotkey: Option<(u32, u32)>,
    /// Left, top, right, bottom of the group including its tab bar.
    pub frame: (i32, i32, i32, i32),
    pub tabs: Vec<PTab>,
}

#[derive(Default, Debug, PartialEq)]
pub struct Store {
    /// "de" or "en"; None follows the language of Windows.
    pub lang: Option<String>,
    pub packages: Vec<Package>,
    /// Key of the purchase, set through dockodile://activate. Lets the app download updates.
    pub token: Option<String>,
    /// The user switched automatic updates off.
    pub manual_updates: bool,
    /// Version installed without asking once; a failed install is not retried forever.
    pub tried: Option<String>,
    /// Version already announced once.
    pub offered: Option<String>,
    /// Tabs that no longer fit one row at a comfortable width continue in a second row.
    pub two_rows: bool,
}

/// One line of the file must stay one line.
pub fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

pub fn serialize(s: &Store) -> String {
    let mut out = String::new();
    if let Some(l) = &s.lang {
        out.push_str(&format!("lang={}\n", clean(l)));
    }
    if let Some(t) = s.token.as_deref().filter(|t| valid_token(t)) {
        out.push_str(&format!("token={}\n", t));
    }
    if s.manual_updates {
        out.push_str("auto=0\n");
    }
    if s.two_rows {
        out.push_str("rows=2\n");
    }
    for (key, v) in [("tried", &s.tried), ("offered", &s.offered)] {
        if let Some(v) = v.as_deref().filter(|v| valid_version(v)) {
            out.push_str(&format!("{}={}\n", key, v));
        }
    }
    for p in &s.packages {
        out.push_str("[paket]\n");
        out.push_str(&format!("name={}\n", clean(&p.name)));
        if let Some((m, k)) = p.hotkey {
            out.push_str(&format!("hotkey={},{}\n", m, k));
        }
        let f = p.frame;
        out.push_str(&format!("frame={},{},{},{}\n", f.0, f.1, f.2, f.3));
        for t in &p.tabs {
            out.push_str(&format!("tab={}\t{}\n", clean(&t.exe), clean(&t.title)));
        }
    }
    out
}

pub fn parse(text: &str) -> Store {
    let mut s = Store::default();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim() == "[paket]" {
            s.packages.push(Package::default());
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        match key {
            "lang" => {
                if value == "de" || value == "en" {
                    s.lang = Some(value.to_string());
                }
                continue;
            }
            "token" if s.packages.is_empty() => {
                s.token = Some(value.to_string()).filter(|t| valid_token(t));
                continue;
            }
            "auto" if s.packages.is_empty() => {
                s.manual_updates = value == "0";
                continue;
            }
            "rows" if s.packages.is_empty() => {
                s.two_rows = value == "2";
                continue;
            }
            "tried" | "offered" if s.packages.is_empty() => {
                let v = Some(value.to_string()).filter(|v| valid_version(v));
                if key == "tried" { s.tried = v } else { s.offered = v }
                continue;
            }
            _ => {}
        }
        let Some(p) = s.packages.last_mut() else { continue };
        match key {
            "name" => p.name = clean(value),
            "hotkey" => {
                let n: Vec<u32> = value.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                if n.len() == 2 {
                    p.hotkey = Some((n[0], n[1]));
                }
            }
            "frame" => {
                let n: Vec<i32> = value.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                if n.len() == 4 && n[2] > n[0] && n[3] > n[1] {
                    p.frame = (n[0], n[1], n[2], n[3]);
                }
            }
            "tab" => {
                let (exe, title) = value.split_once('\t').unwrap_or((value, ""));
                p.tabs.push(PTab { exe: exe.to_string(), title: title.to_string() });
            }
            _ => {}
        }
    }
    s.packages.retain(|p| !p.name.is_empty() && p.tabs.len() >= 2 && p.frame.2 > p.frame.0);
    s
}

/// The count a program puts in front of its window title, as browsers and messengers do:
/// "(3) WhatsApp". Windows gives no program access to another one's taskbar badge.
pub fn unread(title: &str) -> Option<u32> {
    let rest = title.trim_start().strip_prefix('(')?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 5 {
        return None;
    }
    let after = rest[digits.len()..].trim_start_matches('+');
    if !after.starts_with(')') {
        return None;
    }
    digits.parse().ok().filter(|n| *n > 0)
}

pub fn hotkey_name(mods: u32, vk: u32, en: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if mods & MOD_CONTROL != 0 {
        parts.push(if en { "Ctrl" } else { "Strg" }.into());
    }
    if mods & MOD_ALT != 0 {
        parts.push("Alt".into());
    }
    if mods & MOD_SHIFT != 0 {
        parts.push(if en { "Shift" } else { "Umschalt" }.into());
    }
    if mods & MOD_WIN != 0 {
        parts.push("Win".into());
    }
    parts.push(match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x20 => if en { "Space" } else { "Leertaste" }.into(),
        0x0D => "Enter".into(),
        0x09 => "Tab".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        _ => format!("{} {}", if en { "Key" } else { "Taste" }, vk),
    });
    parts.join("+")
}

/// Why a shortcut cannot be used, before Windows is asked.
pub fn hotkey_problem(mods: u32, en: bool) -> Option<&'static str> {
    if mods & (MOD_CONTROL | MOD_ALT) == 0 {
        return Some(if en { "The shortcut needs Ctrl or Alt." } else { "Das Kürzel braucht Strg oder Alt." });
    }
    // AltGr is Ctrl+Alt to Windows: such a shortcut would swallow { [ ] } @ on many keyboards.
    if mods & (MOD_CONTROL | MOD_ALT) == (MOD_CONTROL | MOD_ALT) && mods & (MOD_SHIFT | MOD_WIN) == 0 {
        return Some(if en {
            "Ctrl+Alt alone is AltGr and would block characters like { and @."
        } else {
            "Strg+Alt allein ist AltGr und würde Zeichen wie { und @ blockieren."
        });
    }
    None
}

/// The key of a purchase, as the shop hands it out.
pub fn valid_token(t: &str) -> bool {
    t.len() == 43 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn valid_version(v: &str) -> bool {
    !v.is_empty() && v.len() <= 20 && v.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// True if version `a` is newer than `b`, compared part by part as numbers.
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| v.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (mut x, mut y) = (parts(a), parts(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x > y
}

/// The "version" field of the answer of /api/version.
pub fn version_field(json: &str) -> Option<String> {
    let rest = &json[json.find("\"version\"")? + 9..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let v = &rest[..rest.find('"')?];
    valid_version(v).then(|| v.to_string())
}

pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 1) && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Debug, PartialEq)]
pub enum Command {
    /// dockodile://open?paket=Name
    Open(String),
    /// dockodile://tab?app=Name
    Tab(String),
    /// dockodile://activate?token=Key, from the update page.
    Activate(String),
}

pub fn parse_url(url: &str) -> Option<Command> {
    let url = url.trim().trim_matches('"');
    if url.len() < 10 || !url.is_char_boundary(10) || !url[..10].eq_ignore_ascii_case("dockodile:") {
        return None;
    }
    let rest = url[10..].trim_start_matches('/');
    let (host, query) = rest.split_once('?')?;
    let host = host.trim_matches('/').to_ascii_lowercase();
    let param = |names: &[&str]| -> Option<String> {
        query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            if names.iter().any(|n| k.eq_ignore_ascii_case(n)) {
                let v = url_decode(v).trim().to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
            None
        })
    };
    match host.as_str() {
        "open" => param(&["paket", "package", "name"]).map(Command::Open),
        "tab" => param(&["app", "name", "titel", "title"]).map(Command::Tab),
        "activate" => param(&["token"]).filter(|t| valid_token(t)).map(Command::Activate),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = Store {
            lang: Some("en".into()),
            packages: vec![Package {
                name: "Büro".into(),
                hotkey: Some((MOD_ALT | MOD_SHIFT, 0x31)),
                frame: (-1920, 40, -600, 900),
                tabs: vec![
                    PTab { exe: r"C:\Program Files\A B\a.exe".into(), title: "Eins = zwei".into() },
                    PTab { exe: r"C:\Windows\explorer.exe".into(), title: "".into() },
                ],
            }],
            ..Default::default()
        };
        assert_eq!(parse(&serialize(&s)), s);
        assert_eq!(parse(&serialize(&s).replace('\n', "\r\n")), s);
    }

    #[test]
    fn update_settings() {
        let t = "A".repeat(43);
        let s = Store {
            token: Some(t.clone()),
            manual_updates: true,
            two_rows: true,
            tried: Some("0.3.0".into()),
            offered: Some("0.3.1".into()),
            ..Default::default()
        };
        assert_eq!(parse(&serialize(&s)), s);
        let bad = Store { token: Some("kurz".into()), tried: Some("1.x".into()), ..Default::default() };
        assert_eq!(parse(&serialize(&bad)), Store::default());
        assert_eq!(parse_url(&format!("dockodile://activate?token={}", t)), Some(Command::Activate(t)));
        assert_eq!(parse_url("dockodile://activate?token=abc"), None);
    }

    #[test]
    fn versions() {
        assert!(newer("0.3.0", "0.2.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0.1", "1.0"));
        assert!(!newer("1.0", "1.0.0"));
        assert!(!newer("0.2.0", "0.3.0"));
        assert_eq!(version_field(r#"{"version":"0.3.0"}"#), Some("0.3.0".into()));
        assert_eq!(version_field(r#"{ "version" : "1.2" }"#), Some("1.2".into()));
        assert_eq!(version_field(r#"{"version":"x"}"#), None);
        assert_eq!(version_field(r#"{"error":"weg"}"#), None);
    }

    #[test]
    fn broken_input() {
        assert_eq!(parse("quatsch\n[paket]\nname=x\ntab=a\n"), Store::default());
        assert_eq!(parse("name=ohne paket\nlang=fr\n"), Store::default());
    }

    #[test]
    fn unread_counts() {
        assert_eq!(unread("(3) WhatsApp"), Some(3));
        assert_eq!(unread(" (12+) Inbox"), Some(12));
        assert_eq!(unread("(0) x"), None);
        assert_eq!(unread("Kopie (2).docx - Word"), None);
        assert_eq!(unread("(Entwurf) x"), None);
        assert_eq!(unread("(3"), None);
    }

    #[test]
    fn urls() {
        assert_eq!(parse_url("dockodile://open?paket=B%C3%BCro"), Some(Command::Open("Büro".into())));
        assert_eq!(parse_url("DOCKODILE://tab/?app=Mail%20neu"), Some(Command::Tab("Mail neu".into())));
        assert_eq!(parse_url("dockodile://open?x=1&package=A"), Some(Command::Open("A".into())));
        assert_eq!(parse_url("dockodile://open"), None);
        assert_eq!(parse_url("dockodile://weg?paket=A"), None);
        assert_eq!(parse_url("http://open?paket=A"), None);
        assert_eq!(url_decode("100%"), "100%");
        assert_eq!(url_decode("%zz%41"), "%zzA");
    }

    #[test]
    fn hotkeys() {
        assert_eq!(hotkey_name(MOD_ALT | MOD_SHIFT, 0x31, false), "Alt+Umschalt+1");
        assert_eq!(hotkey_name(MOD_CONTROL, 0x71, true), "Ctrl+F2");
        assert!(hotkey_problem(MOD_SHIFT, false).is_some());
        assert!(hotkey_problem(MOD_CONTROL | MOD_ALT, false).is_some());
        assert!(hotkey_problem(MOD_CONTROL | MOD_ALT | MOD_SHIFT, false).is_none());
        assert!(hotkey_problem(MOD_ALT, false).is_none());
    }
}
