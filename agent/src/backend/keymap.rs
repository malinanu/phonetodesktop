//! Key names the phone may send, mapped to `enigo` keys (macOS and Linux).
//! The accepted set is the same as the Windows table in `keys.rs`: named keys, F1-F12, a-z and 0-9.

use enigo::Key;

/// A named key, or a single a-z / 0-9 character. Anything else is rejected.
pub fn named_key(name: &str) -> Option<Key> {
    let n = name.to_ascii_lowercase();
    let k = match n.as_str() {
        "enter" => Key::Return,
        "backspace" => Key::Backspace,
        "tab" => Key::Tab,
        "esc" => Key::Escape,
        "space" => Key::Space,
        "delete" => Key::Delete,
        // Macs have no Insert key; the phone's "insert" is reported as an unknown key there.
        #[cfg(not(target_os = "macos"))]
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "left" => Key::LeftArrow,
        "up" => Key::UpArrow,
        "right" => Key::RightArrow,
        "down" => Key::DownArrow,
        "win" => Key::Meta,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => {
            let mut chars = n.chars();
            return match (chars.next(), chars.next()) {
                (Some(c @ ('a'..='z' | '0'..='9')), None) => Some(Key::Unicode(c)),
                _ => None,
            };
        }
    };
    Some(k)
}

pub fn modifier(name: &str) -> Option<Key> {
    match name.to_ascii_lowercase().as_str() {
        "ctrl" => Some(Key::Control),
        "alt" => Some(Key::Alt),
        "shift" => Some(Key::Shift),
        "win" => Some(Key::Meta),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_and_single_char_keys() {
        assert!(matches!(named_key("Enter"), Some(Key::Return)));
        assert!(matches!(named_key("left"), Some(Key::LeftArrow)));
        assert!(matches!(named_key("c"), Some(Key::Unicode('c'))));
        assert!(matches!(named_key("7"), Some(Key::Unicode('7'))));
        assert!(matches!(named_key("f12"), Some(Key::F12)));
    }

    #[test]
    fn unknown_keys_are_rejected() {
        for bad in ["", "ab", "f13", "rm -rf", "!", "é"] {
            assert!(named_key(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn modifiers() {
        assert!(matches!(modifier("Ctrl"), Some(Key::Control)));
        assert!(matches!(modifier("win"), Some(Key::Meta)));
        assert!(modifier("hyper").is_none());
    }
}
