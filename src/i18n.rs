//! Minimal translation layer. Every UI string goes through [`t!`].
//!
//! [`tr`] / [`t!`] return strings ready for *display*: text containing
//! right-to-left script is converted with [`crate::bidi::visual_string`]
//! so it renders correctly in egui. Use [`tr_logical`] for strings that end
//! up as data (default names) or in native OS dialogs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::bidi::visual_string;
use crate::config::Language;

mod ar;
mod en;

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Sets the active language.
pub fn set_language(lang: Language) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

/// The active language.
pub fn language() -> Language {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Language::Ar,
        _ => Language::En,
    }
}

/// Whether the active language is written right to left.
pub fn is_rtl() -> bool {
    language() == Language::Ar
}

type Table = HashMap<&'static str, &'static str>;

fn logical_table(lang: Language) -> &'static Table {
    static EN: OnceLock<Table> = OnceLock::new();
    static AR: OnceLock<Table> = OnceLock::new();
    match lang {
        Language::En => EN.get_or_init(|| en::STRINGS.iter().copied().collect()),
        Language::Ar => AR.get_or_init(|| {
            let mut t: Table = en::STRINGS.iter().copied().collect();
            t.extend(ar::STRINGS.iter().copied());
            t
        }),
    }
}

/// Display string → logical original, so wrapped paragraphs can be laid out
/// from the logical text (see `logical_of`).
fn logical_map() -> &'static Mutex<HashMap<String, String>> {
    static MAP: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember(display: &str, logical: &str) {
    if display == logical {
        return;
    }
    let mut map = logical_map().lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > 4096 {
        map.clear();
    }
    map.insert(display.to_owned(), logical.to_owned());
}

/// Converts logical text for display and remembers the pair, so that
/// wrapped paragraphs can recover the logical text with [`logical_of`].
pub fn display(logical: &str) -> String {
    let d = visual_string(logical);
    remember(&d, logical);
    d
}

/// The logical (storage-order) text behind a display string produced by
/// [`tr`] or [`tr_args`]; `None` for strings that need no conversion.
pub fn logical_of(display: &str) -> Option<String> {
    logical_map()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(display)
        .cloned()
}

fn display_table(lang: Language) -> &'static Table {
    static EN: OnceLock<Table> = OnceLock::new();
    static AR: OnceLock<Table> = OnceLock::new();
    let cell = match lang {
        Language::En => &EN,
        Language::Ar => &AR,
    };
    cell.get_or_init(|| {
        logical_table(lang)
            .iter()
            .map(|(k, v)| {
                let display: &'static str = Box::leak(visual_string(v).into_boxed_str());
                remember(display, v);
                (*k, display)
            })
            .collect()
    })
}

/// All English keys (tests check every `t!` key exists).
pub fn english_keys() -> impl Iterator<Item = &'static str> {
    en::STRINGS.iter().map(|(k, _)| *k)
}

/// All `(key, text)` pairs of a language's own table.
pub fn strings(lang: Language) -> &'static [(&'static str, &'static str)] {
    match lang {
        Language::En => en::STRINGS,
        Language::Ar => ar::STRINGS,
    }
}

/// Looks up a key for display; falls back to English, then to the key.
pub fn tr(key: &'static str) -> &'static str {
    display_table(language()).get(key).copied().unwrap_or(key)
}

/// Looks up a key in logical order (for data and native dialogs).
pub fn tr_logical(key: &'static str) -> &'static str {
    logical_table(language()).get(key).copied().unwrap_or(key)
}

/// Looks up a key, substitutes `{name}` placeholders and converts for display.
pub fn tr_args(key: &'static str, args: &[(&str, String)]) -> String {
    let mut s = tr_logical(key).to_owned();
    for (name, value) in args {
        // Each value is isolated so numbers with units, file names or titles
        // in another script keep their own direction inside the sentence.
        s = s.replace(&format!("{{{name}}}"), &crate::bidi::isolate(value));
    }
    let display = visual_string(&s);
    remember(&display, &s);
    display
}

/// Translate a UI string for display: `t!("key")` or `t!("key", n = 3)`.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::tr($key)
    };
    ($key:literal, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::tr_args($key, &[$((stringify!($name), ($val).to_string())),+])
    };
}
