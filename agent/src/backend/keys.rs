//! Key names the phone may send, and their Windows virtual-key codes.
//! Kept free of Windows types so the table is validated by unit tests on any OS.

/// (name, virtual key, needs the "extended key" flag)
const KEYS: &[(&str, u16, bool)] = &[
    ("enter", 0x0D, false),
    ("backspace", 0x08, false),
    ("tab", 0x09, false),
    ("esc", 0x1B, false),
    ("space", 0x20, false),
    ("delete", 0x2E, true),
    ("insert", 0x2D, true),
    ("home", 0x24, true),
    ("end", 0x23, true),
    ("pageup", 0x21, true),
    ("pagedown", 0x22, true),
    ("left", 0x25, true),
    ("up", 0x26, true),
    ("right", 0x27, true),
    ("down", 0x28, true),
    ("win", 0x5B, true),
    ("f1", 0x70, false),
    ("f2", 0x71, false),
    ("f3", 0x72, false),
    ("f4", 0x73, false),
    ("f5", 0x74, false),
    ("f6", 0x75, false),
    ("f7", 0x76, false),
    ("f8", 0x77, false),
    ("f9", 0x78, false),
    ("f10", 0x79, false),
    ("f11", 0x7A, false),
    ("f12", 0x7B, false),
];

const MODS: &[(&str, u16)] = &[("ctrl", 0x11), ("alt", 0x12), ("shift", 0x10), ("win", 0x5B)];

/// Virtual key and extended flag for a key name: a named key, or a single a-z / 0-9 character.
pub fn key(name: &str) -> Option<(u16, bool)> {
    let n = name.to_ascii_lowercase();
    if let Some((_, vk, ext)) = KEYS.iter().find(|(k, _, _)| *k == n) {
        return Some((*vk, *ext));
    }
    let mut chars = n.chars();
    match (chars.next(), chars.next()) {
        (Some(c @ 'a'..='z'), None) => Some((c.to_ascii_uppercase() as u16, false)),
        (Some(c @ '0'..='9'), None) => Some((c as u16, false)),
        _ => None,
    }
}

pub fn modifier(name: &str) -> Option<u16> {
    let n = name.to_ascii_lowercase();
    MODS.iter().find(|(k, _)| *k == n).map(|(_, vk)| *vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_and_single_char_keys() {
        assert_eq!(key("Enter"), Some((0x0D, false)));
        assert_eq!(key("left"), Some((0x25, true)));
        assert_eq!(key("c"), Some((0x43, false)));
        assert_eq!(key("7"), Some((0x37, false)));
        assert_eq!(key("f12"), Some((0x7B, false)));
    }

    #[test]
    fn unknown_keys_are_rejected() {
        for bad in ["", "ab", "f13", "rm -rf", "!", "é"] {
            assert_eq!(key(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn modifiers() {
        assert_eq!(modifier("CTRL"), Some(0x11));
        assert_eq!(modifier("win"), Some(0x5B));
        assert_eq!(modifier("meta"), None);
    }
}
