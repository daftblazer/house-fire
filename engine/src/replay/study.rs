
use crate::library::Library;
use crate::FileId;

/// The study switches (`REPLAY_*`, `HF_*`) as the environment had them at the start, read once:
/// the checks ask for theirs at every step, and a lookup through libc's environment is a scan.
/// Usually there are none, and a lookup is one test.
pub fn switches() -> &'static [(String, String)] {
    static SWITCHES: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    SWITCHES.get_or_init(|| {
        std::env::vars().filter(|(k, _)| k.starts_with("REPLAY_") || k.starts_with("HF_")).collect()
    })
}
pub fn env(name: &str) -> Option<String> {
    switches().iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}
pub fn has(name: &str) -> bool {
    switches().iter().any(|(k, _)| k == name)
}
/// atoi: the leading integer of a string, 0 if none.
pub fn atoi(s: &str) -> i64 {
    let t = s.trim_start();
    let (sign, rest) = match t.as_bytes().first() {
        Some(b'-') => (-1, &t[1..]),
        Some(b'+') => (1, &t[1..]),
        _ => (1, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    sign * digits.parse::<i64>().unwrap_or(0)
}
pub(super) fn file_name(lib: &Library, f: FileId) -> String {
    let p = &lib.file(f).path;
    p[p.rfind('/').map_or(0, |s| s + 1)..].to_string()
}
