//! Physical keys and the dog's mini keyboard.
//!
//! The mini keyboard is a 61-key (60%) layout. Every real key maps to one cell on it.
//! Key codes are only used to pick a cell (which paw, which key lights up) and are
//! never stored.

/// Index of a cell on the mini keyboard (0..KEY_COUNT).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(pub u8);

pub const KEY_COUNT: usize = 61;
pub const ROWS: usize = 5;
/// Every row is this many key-units wide.
pub const ROW_UNITS: f32 = 15.0;

/// Key widths per row, from the user's point of view (row 0 = number row, left to right).
const ROW_WIDTHS: [&[f32]; ROWS] = [
    // ` 1 2 3 4 5 6 7 8 9 0 - = Backspace
    &[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0],
    // Tab Q W E R T Y U I O P [ ] \
    &[1.5, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.5],
    // Caps A S D F G H J K L ; ' Enter
    &[1.75, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.25],
    // Shift Z X C V B N M , . / Shift
    &[2.25, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.75],
    // Ctrl Win Alt Space Alt Fn Menu Ctrl
    &[1.25, 1.25, 1.25, 6.25, 1.25, 1.25, 1.25, 1.25],
];

const ROW_START: [u8; ROWS] = [0, 14, 28, 41, 53];

/// A cell in user-facing layout coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyCell {
    pub row: u8,
    /// Left edge in key units (0..15) as the user sees their keyboard.
    pub x0: f32,
    pub width: f32,
}

pub fn key(row: usize, index: usize) -> KeyId {
    debug_assert!(row < ROWS && index < ROW_WIDTHS[row].len());
    KeyId(ROW_START[row] + index as u8)
}

pub fn cell(id: KeyId) -> KeyCell {
    let i = id.0 as usize;
    let mut row = ROWS - 1;
    for r in 0..ROWS {
        let start = ROW_START[r] as usize;
        if i >= start && i < start + ROW_WIDTHS[r].len() {
            row = r;
            break;
        }
    }
    let idx = i - ROW_START[row] as usize;
    let widths = ROW_WIDTHS[row];
    let x0: f32 = widths[..idx.min(widths.len())].iter().sum();
    KeyCell { row: row as u8, x0, width: widths[idx.min(widths.len() - 1)] }
}

/// Which keycap colour a key gets, like a two-tone keycap set with a few accent keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// Letters, digits and punctuation.
    Alpha,
    /// Tab, Caps, Shift, Ctrl, Win, Alt, Fn, Menu, Backspace.
    Modifier,
    /// Esc and Enter: the bright accent keys.
    Accent,
    Space,
}

pub fn key_kind(id: KeyId) -> KeyKind {
    let c = cell(id);
    let last = |row: usize| ROW_WIDTHS[row].len() - 1;
    let idx = id.0 as usize - ROW_START[c.row as usize] as usize;
    match (c.row as usize, idx) {
        (0, 0) | (2, 12) => KeyKind::Accent,
        (4, 3) => KeyKind::Space,
        (0, i) if i == last(0) => KeyKind::Modifier,
        (1, 0) | (2, 0) | (3, 0) => KeyKind::Modifier,
        (3, i) if i == last(3) => KeyKind::Modifier,
        (4, _) => KeyKind::Modifier,
        _ => KeyKind::Alpha,
    }
}

pub fn all_keys() -> impl Iterator<Item = KeyId> {
    (0..KEY_COUNT as u8).map(KeyId)
}

/// Maps a PC/AT "set 1" make code (what Windows Raw Input reports) to a mini-keyboard key.
/// `e0` is the extended-key prefix flag.
pub fn key_from_set1(make: u16, e0: bool) -> Option<KeyId> {
    let k = |r: usize, i: usize| Some(key(r, i));
    if e0 {
        return match make {
            0x1C => k(2, 12),           // numpad Enter
            0x1D => k(4, 7),            // right Ctrl
            0x35 => k(3, 10),           // numpad /
            0x37 => k(0, 13),           // Print Screen
            0x38 => k(4, 4),            // right Alt
            0x47 | 0x49 | 0x52 | 0x53 => k(0, 13), // Home, PgUp, Insert, Delete
            0x4F | 0x51 => k(1, 13),    // End, PgDn
            0x48 => k(3, 11),           // Up
            0x4B => k(4, 5),            // Left
            0x50 => k(4, 6),            // Down
            0x4D => k(4, 7),            // Right
            0x5B => k(4, 1),            // left Windows
            0x5C => k(4, 5),            // right Windows
            0x5D => k(4, 6),            // Menu
            _ => None,
        };
    }
    match make {
        0x01 | 0x29 => k(0, 0),                              // Esc, `
        0x02..=0x0B => k(0, (make - 0x01) as usize),          // 1..0
        0x0C => k(0, 11),
        0x0D => k(0, 12),
        0x0E => k(0, 13),                                    // Backspace
        0x0F => k(1, 0),                                     // Tab
        0x10..=0x19 => k(1, (make - 0x0F) as usize),          // Q..P
        0x1A => k(1, 11),
        0x1B => k(1, 12),
        0x2B => k(1, 13),                                    // backslash
        0x3A => k(2, 0),                                     // Caps Lock
        0x1E..=0x26 => k(2, (make - 0x1D) as usize),          // A..L
        0x27 => k(2, 10),
        0x28 => k(2, 11),
        0x1C => k(2, 12),                                    // Enter
        0x2A | 0x56 => k(3, 0),                              // left Shift, ISO key
        0x2C..=0x32 => k(3, (make - 0x2B) as usize),          // Z..M
        0x33 => k(3, 8),
        0x34 => k(3, 9),
        0x35 => k(3, 10),
        0x36 => k(3, 11),                                    // right Shift
        0x1D => k(4, 0),                                     // left Ctrl
        0x38 => k(4, 2),                                     // left Alt
        0x39 => k(4, 3),                                     // Space
        0x3B..=0x44 => k(0, (make - 0x3A) as usize),          // F1..F10
        0x57 => k(0, 11),                                    // F11
        0x58 => k(0, 12),                                    // F12
        // Numpad (NumLock on or off, without the E0 prefix)
        0x47 => k(0, 7),
        0x48 => k(0, 8),
        0x49 => k(0, 9),
        0x4A => k(0, 11),
        0x4B => k(0, 4),
        0x4C => k(0, 5),
        0x4D => k(0, 6),
        0x4E => k(0, 12),
        0x4F => k(0, 1),
        0x50 => k(0, 2),
        0x51 => k(0, 3),
        0x52 => k(0, 10),
        0x53 => k(3, 9),
        0x37 => k(0, 8),                                     // numpad *
        0x45 | 0x46 => k(0, 13),                             // Num Lock, Scroll Lock
        _ => None,
    }
}

/// What is printed on the real key with this PC/AT "set 1" make code, for the key pop-ups
/// (US layout). Letters, digits and punctuation are the character itself; other keys are a
/// short token such as `SHIFT`, `ENTER`, `LEFT` or `F5` (see `glyphs::layout`).
pub fn label_from_set1(make: u16, e0: bool) -> Option<&'static str> {
    const ROW1: [&str; 12] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "="];
    const QWERTY: [&str; 12] = ["Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P", "[", "]"];
    const HOME: [&str; 11] = ["A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'"];
    const BOTTOM: [&str; 10] = ["Z", "X", "C", "V", "B", "N", "M", ",", ".", "/"];
    const FKEYS: [&str; 10] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10"];
    if e0 {
        return Some(match make {
            0x1C => "ENTER",
            0x1D => "CTRL",
            0x35 => "/",
            0x37 => "PRT",
            0x38 => "ALT",
            0x47 => "HOME",
            0x48 => "UP",
            0x49 => "PGUP",
            0x4B => "LEFT",
            0x4D => "RIGHT",
            0x4F => "END",
            0x50 => "DOWN",
            0x51 => "PGDN",
            0x52 => "INS",
            0x53 => "DEL",
            0x5B | 0x5C => "WIN",
            0x5D => "MENU",
            _ => return None,
        });
    }
    Some(match make {
        0x01 => "ESC",
        0x02..=0x0D => ROW1[(make - 0x02) as usize],
        0x0E => "BKSP",
        0x0F => "TAB",
        0x10..=0x1B => QWERTY[(make - 0x10) as usize],
        0x1C => "ENTER",
        0x1D => "CTRL",
        0x1E..=0x28 => HOME[(make - 0x1E) as usize],
        0x29 => "`",
        0x2A | 0x36 => "SHIFT",
        0x2B | 0x56 => "\\",
        0x2C..=0x35 => BOTTOM[(make - 0x2C) as usize],
        0x37 => "*",
        0x38 => "ALT",
        0x39 => "SPACE",
        0x3A => "CAPS",
        0x3B..=0x44 => FKEYS[(make - 0x3B) as usize],
        0x45 => "NUM",
        0x46 => "SCRL",
        // Numpad without the E0 prefix: digits (NumLock on) and operators.
        0x47 => "7",
        0x48 => "8",
        0x49 => "9",
        0x4A => "-",
        0x4B => "4",
        0x4C => "5",
        0x4D => "6",
        0x4E => "+",
        0x4F => "1",
        0x50 => "2",
        0x51 => "3",
        0x52 => "0",
        0x53 => ".",
        0x57 => "F11",
        0x58 => "F12",
        _ => return None,
    })
}

/// Maps a Linux evdev key code (`KEY_*`) to a mini-keyboard key.
pub fn key_from_linux(code: u16) -> Option<KeyId> {
    match code {
        // Codes 1..=88 are identical to set-1 make codes.
        1..=88 => key_from_set1(code, false),
        96 => key_from_set1(0x1C, true),  // KEY_KPENTER
        97 => key_from_set1(0x1D, true),  // KEY_RIGHTCTRL
        98 => key_from_set1(0x35, true),  // KEY_KPSLASH
        99 => key_from_set1(0x37, true),  // KEY_SYSRQ
        100 => key_from_set1(0x38, true), // KEY_RIGHTALT
        102 => key_from_set1(0x47, true), // KEY_HOME
        103 => key_from_set1(0x48, true), // KEY_UP
        104 => key_from_set1(0x49, true), // KEY_PAGEUP
        105 => key_from_set1(0x4B, true), // KEY_LEFT
        106 => key_from_set1(0x4D, true), // KEY_RIGHT
        107 => key_from_set1(0x4F, true), // KEY_END
        108 => key_from_set1(0x50, true), // KEY_DOWN
        109 => key_from_set1(0x51, true), // KEY_PAGEDOWN
        110 => key_from_set1(0x52, true), // KEY_INSERT
        111 => key_from_set1(0x53, true), // KEY_DELETE
        119 => Some(key(0, 13)),          // KEY_PAUSE
        125 => key_from_set1(0x5B, true), // KEY_LEFTMETA
        126 => key_from_set1(0x5C, true), // KEY_RIGHTMETA
        127 => key_from_set1(0x5D, true), // KEY_COMPOSE
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_full_width_and_count_matches() {
        let mut total = 0;
        for (r, widths) in ROW_WIDTHS.iter().enumerate() {
            let sum: f32 = widths.iter().sum();
            assert!((sum - ROW_UNITS).abs() < 1e-4, "row {r} is {sum} units");
            assert_eq!(ROW_START[r] as usize, total, "row {r} start");
            total += widths.len();
        }
        assert_eq!(total, KEY_COUNT);
    }

    #[test]
    fn cells_round_trip() {
        for id in all_keys() {
            let c = cell(id);
            let r = c.row as usize;
            let idx = id.0 as usize - ROW_START[r] as usize;
            assert_eq!(key(r, idx), id);
            assert!(c.x0 + c.width <= ROW_UNITS + 1e-4);
        }
    }

    #[test]
    fn letters_land_on_expected_cells() {
        assert_eq!(key_from_set1(0x10, false), Some(key(1, 1))); // Q
        assert_eq!(key_from_set1(0x19, false), Some(key(1, 10))); // P
        assert_eq!(key_from_set1(0x1E, false), Some(key(2, 1))); // A
        assert_eq!(key_from_set1(0x26, false), Some(key(2, 9))); // L
        assert_eq!(key_from_set1(0x2C, false), Some(key(3, 1))); // Z
        assert_eq!(key_from_set1(0x32, false), Some(key(3, 7))); // M
        assert_eq!(key_from_set1(0x02, false), Some(key(0, 1))); // 1
        assert_eq!(key_from_set1(0x0B, false), Some(key(0, 10))); // 0
        assert_eq!(key_from_set1(0x39, false), Some(key(4, 3))); // Space
        assert_eq!(key_from_set1(0x1D, true), Some(key(4, 7))); // right Ctrl
    }

    #[test]
    fn every_set1_mapping_is_in_range() {
        for make in 0..0x100u16 {
            for e0 in [false, true] {
                if let Some(id) = key_from_set1(make, e0) {
                    assert!((id.0 as usize) < KEY_COUNT, "{make:#x} e0={e0}");
                }
            }
        }
        for code in 0..300u16 {
            if let Some(id) = key_from_linux(code) {
                assert!((id.0 as usize) < KEY_COUNT);
            }
        }
    }
}
