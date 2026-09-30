// Modified by Choro contributors; see vendor/gpui-terminal/CHORO_MODIFICATIONS.md.
//! Keyboard input handling for the terminal emulator.
//!
//! This module provides [`keystroke_to_bytes`], which converts GPUI keyboard
//! events into terminal escape sequences that can be written to the PTY.
//!
//! # Key Mappings
//!
//! ## Special Keys
//!
//! | Key | Sequence | Notes |
//! |-----|----------|-------|
//! | Enter | `\r` (0x0D) | Carriage return |
//! | Escape | `\x1b` (0x1B) | ESC |
//! | Backspace | `\x7f` (0x7F) | DEL |
//! | Tab | `\t` (0x09) | Horizontal tab |
//! | Shift+Tab | `\x1b[Z` | Backtab |
//! | Space | ` ` (0x20) | Space |
//! | Ctrl+Space | `\x00` | NUL |
//!
//! ## Arrow Keys
//!
//! Arrow key sequences depend on application cursor mode:
//!
//! | Key | Normal Mode | App Cursor Mode |
//! |-----|-------------|-----------------|
//! | Up | `\x1b[A` | `\x1bOA` |
//! | Down | `\x1b[B` | `\x1bOB` |
//! | Right | `\x1b[C` | `\x1bOC` |
//! | Left | `\x1b[D` | `\x1bOD` |
//!
//! ## Navigation Keys
//!
//! | Key | Sequence |
//! |-----|----------|
//! | Home | `\x1b[H` |
//! | End | `\x1b[F` |
//! | PageUp | `\x1b[5~` |
//! | PageDown | `\x1b[6~` |
//! | Insert | `\x1b[2~` |
//! | Delete | `\x1b[3~` |
//!
//! ## Function Keys
//!
//! | Key | Sequence |
//! |-----|----------|
//! | F1-F4 | `\x1bOP` - `\x1bOS` |
//! | F5-F20 | `\x1b[15~` - `\x1b[34~` |
//!
//! Shift, Alt, and Control variants of arrow, navigation, and function keys
//! use xterm PC-style modifier sequences such as `\x1b[1;5D` for Ctrl+Left.
//!
//! ## Control Combinations
//!
//! Ctrl+A through Ctrl+Z map to ASCII control characters 0x01-0x1A:
//!
//! | Combination | Byte |
//! |-------------|------|
//! | Ctrl+A | 0x01 |
//! | Ctrl+C | 0x03 (interrupt) |
//! | Ctrl+D | 0x04 (EOF) |
//! | Ctrl+Z | 0x1A (suspend) |
//!
//! ## Alt Combinations
//!
//! Alt+key sends ESC followed by the key: `\x1b` + key
//!
//! # Terminal Mode Effects
//!
//! The [`TermMode`] flags affect key sequences:
//!
//! - **APP_CURSOR**: Changes arrow key sequences from CSI to SS3 format
//!
//! # Example
//!
//! ```
//! use gpui::Keystroke;
//! use alacritty_terminal::term::TermMode;
//! use gpui_terminal::input::keystroke_to_bytes;
//!
//! // Enter key
//! let keystroke = Keystroke::parse("enter").unwrap();
//! assert_eq!(keystroke_to_bytes(&keystroke, TermMode::empty()), Some(b"\r".to_vec()));
//!
//! // Ctrl+C (interrupt)
//! let keystroke = Keystroke::parse("ctrl-c").unwrap();
//! assert_eq!(keystroke_to_bytes(&keystroke, TermMode::empty()), Some(vec![0x03]));
//! ```

use alacritty_terminal::term::TermMode;
use gpui::Keystroke;

fn no_modifiers(keystroke: &Keystroke) -> bool {
    !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.shift
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
}

fn only_shift(keystroke: &Keystroke) -> bool {
    keystroke.modifiers.shift
        && !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
}

fn only_control(keystroke: &Keystroke) -> bool {
    keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.shift
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
}

fn only_alt(keystroke: &Keystroke) -> bool {
    keystroke.modifiers.alt
        && !keystroke.modifiers.control
        && !keystroke.modifiers.shift
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
}

/// Xterm PC-style modifier code.
///
/// 2 = Shift, 3 = Alt, 4 = Shift+Alt, 5 = Control,
/// 6 = Shift+Control, 7 = Alt+Control, 8 = Shift+Alt+Control.
fn modifier_code(keystroke: &Keystroke) -> Option<u8> {
    if keystroke.modifiers.platform || keystroke.modifiers.function {
        return None;
    }

    let mut code = 1;
    if keystroke.modifiers.shift {
        code += 1;
    }
    if keystroke.modifiers.alt {
        code += 2;
    }
    if keystroke.modifiers.control {
        code += 4;
    }

    (code > 1).then_some(code)
}

fn modified_special_key_bytes(keystroke: &Keystroke) -> Option<Vec<u8>> {
    let modifier = modifier_code(keystroke)?;
    let sequence = match keystroke.key.as_str() {
        "up" => format!("\x1b[1;{modifier}A"),
        "down" => format!("\x1b[1;{modifier}B"),
        "right" => format!("\x1b[1;{modifier}C"),
        "left" => format!("\x1b[1;{modifier}D"),
        "home" => format!("\x1b[1;{modifier}H"),
        "end" => format!("\x1b[1;{modifier}F"),
        "insert" => format!("\x1b[2;{modifier}~"),
        "delete" => format!("\x1b[3;{modifier}~"),
        "pageup" => format!("\x1b[5;{modifier}~"),
        "pagedown" => format!("\x1b[6;{modifier}~"),
        "f1" => format!("\x1b[1;{modifier}P"),
        "f2" => format!("\x1b[1;{modifier}Q"),
        "f3" => format!("\x1b[1;{modifier}R"),
        "f4" => format!("\x1b[1;{modifier}S"),
        "f5" => format!("\x1b[15;{modifier}~"),
        "f6" => format!("\x1b[17;{modifier}~"),
        "f7" => format!("\x1b[18;{modifier}~"),
        "f8" => format!("\x1b[19;{modifier}~"),
        "f9" => format!("\x1b[20;{modifier}~"),
        "f10" => format!("\x1b[21;{modifier}~"),
        "f11" => format!("\x1b[23;{modifier}~"),
        "f12" => format!("\x1b[24;{modifier}~"),
        "f13" => format!("\x1b[25;{modifier}~"),
        "f14" => format!("\x1b[26;{modifier}~"),
        "f15" => format!("\x1b[28;{modifier}~"),
        "f16" => format!("\x1b[29;{modifier}~"),
        "f17" => format!("\x1b[31;{modifier}~"),
        "f18" => format!("\x1b[32;{modifier}~"),
        "f19" => format!("\x1b[33;{modifier}~"),
        "f20" => format!("\x1b[34;{modifier}~"),
        _ => return None,
    };

    Some(sequence.into_bytes())
}

fn control_character(keystroke: &Keystroke) -> Option<Vec<u8>> {
    if !keystroke.modifiers.control
        || keystroke.modifiers.alt
        || keystroke.modifiers.platform
        || keystroke.modifiers.function
    {
        return None;
    }

    let key = keystroke.key.as_str();
    if key.len() != 1 {
        return None;
    }

    let ch = key.chars().next().unwrap();
    if ch.is_ascii_alphabetic() {
        let upper = ch.to_ascii_uppercase();
        return Some(vec![(upper as u8) - b'@']);
    }

    match ch {
        '@' => Some(b"\x00".to_vec()),
        '[' => Some(b"\x1b".to_vec()),
        '\\' => Some(b"\x1c".to_vec()),
        ']' => Some(b"\x1d".to_vec()),
        '^' => Some(b"\x1e".to_vec()),
        '_' => Some(b"\x1f".to_vec()),
        '?' => Some(b"\x7f".to_vec()),
        _ => None,
    }
}

/// Convert a GPUI keystroke to terminal escape sequence bytes.
///
/// This function translates GPUI keyboard events into the appropriate byte sequences
/// expected by terminal applications. It handles special keys, control characters,
/// and application cursor mode.
///
/// # Arguments
///
/// * `keystroke` - The GPUI keystroke to convert
/// * `mode` - The current terminal mode (affects arrow key sequences)
///
/// # Returns
///
/// An optional vector of bytes representing the terminal escape sequence.
/// Returns `None` if the keystroke should not produce any output.
///
/// # Examples
///
/// ```
/// use gpui::Keystroke;
/// use alacritty_terminal::term::TermMode;
/// use gpui_terminal::input::keystroke_to_bytes;
///
/// let keystroke = Keystroke::parse("enter").unwrap();
/// let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
/// assert_eq!(bytes, Some(b"\r".to_vec()));
/// ```
pub fn keystroke_to_bytes(keystroke: &Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    match keystroke.key.as_str() {
        "space" => {
            if only_control(keystroke) {
                return Some(b"\x00".to_vec()); // Ctrl+Space = NUL
            }
            if no_modifiers(keystroke) || only_shift(keystroke) {
                return Some(b" ".to_vec());
            }
        }
        "enter" => {
            if no_modifiers(keystroke) {
                return Some(b"\r".to_vec());
            }
            if only_shift(keystroke) {
                return Some(b"\n".to_vec());
            }
            if only_alt(keystroke) {
                return Some(b"\x1b\r".to_vec());
            }
        }
        "escape" if no_modifiers(keystroke) => return Some(b"\x1b".to_vec()),
        "backspace" => {
            if no_modifiers(keystroke) || only_shift(keystroke) {
                return Some(b"\x7f".to_vec());
            }
            if only_control(keystroke) {
                return Some(b"\x08".to_vec());
            }
            if only_alt(keystroke) {
                return Some(b"\x1b\x7f".to_vec());
            }
        }
        "tab" => {
            if only_shift(keystroke) {
                return Some(b"\x1b[Z".to_vec());
            }
            if no_modifiers(keystroke) {
                return Some(b"\t".to_vec());
            }
        }

        "up" if no_modifiers(keystroke) => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOA".to_vec());
            }
            return Some(b"\x1b[A".to_vec());
        }
        "down" if no_modifiers(keystroke) => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOB".to_vec());
            }
            return Some(b"\x1b[B".to_vec());
        }
        "right" if no_modifiers(keystroke) => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOC".to_vec());
            }
            return Some(b"\x1b[C".to_vec());
        }
        "left" if no_modifiers(keystroke) => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOD".to_vec());
            }
            return Some(b"\x1b[D".to_vec());
        }

        "home" if no_modifiers(keystroke) => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOH".to_vec());
            }
            return Some(b"\x1b[H".to_vec());
        }
        "end" if no_modifiers(keystroke) => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOF".to_vec());
            }
            return Some(b"\x1b[F".to_vec());
        }
        "pageup" if no_modifiers(keystroke) => return Some(b"\x1b[5~".to_vec()),
        "pagedown" if no_modifiers(keystroke) => return Some(b"\x1b[6~".to_vec()),
        "insert" if no_modifiers(keystroke) => return Some(b"\x1b[2~".to_vec()),
        "delete" if no_modifiers(keystroke) => return Some(b"\x1b[3~".to_vec()),

        "f1" if no_modifiers(keystroke) => return Some(b"\x1bOP".to_vec()),
        "f2" if no_modifiers(keystroke) => return Some(b"\x1bOQ".to_vec()),
        "f3" if no_modifiers(keystroke) => return Some(b"\x1bOR".to_vec()),
        "f4" if no_modifiers(keystroke) => return Some(b"\x1bOS".to_vec()),
        "f5" if no_modifiers(keystroke) => return Some(b"\x1b[15~".to_vec()),
        "f6" if no_modifiers(keystroke) => return Some(b"\x1b[17~".to_vec()),
        "f7" if no_modifiers(keystroke) => return Some(b"\x1b[18~".to_vec()),
        "f8" if no_modifiers(keystroke) => return Some(b"\x1b[19~".to_vec()),
        "f9" if no_modifiers(keystroke) => return Some(b"\x1b[20~".to_vec()),
        "f10" if no_modifiers(keystroke) => return Some(b"\x1b[21~".to_vec()),
        "f11" if no_modifiers(keystroke) => return Some(b"\x1b[23~".to_vec()),
        "f12" if no_modifiers(keystroke) => return Some(b"\x1b[24~".to_vec()),
        "f13" if no_modifiers(keystroke) => return Some(b"\x1b[25~".to_vec()),
        "f14" if no_modifiers(keystroke) => return Some(b"\x1b[26~".to_vec()),
        "f15" if no_modifiers(keystroke) => return Some(b"\x1b[28~".to_vec()),
        "f16" if no_modifiers(keystroke) => return Some(b"\x1b[29~".to_vec()),
        "f17" if no_modifiers(keystroke) => return Some(b"\x1b[31~".to_vec()),
        "f18" if no_modifiers(keystroke) => return Some(b"\x1b[32~".to_vec()),
        "f19" if no_modifiers(keystroke) => return Some(b"\x1b[33~".to_vec()),
        "f20" if no_modifiers(keystroke) => return Some(b"\x1b[34~".to_vec()),

        _ => {}
    }

    if let Some(bytes) = control_character(keystroke) {
        return Some(bytes);
    }

    if let Some(bytes) = modified_special_key_bytes(keystroke) {
        return Some(bytes);
    }

    if keystroke.modifiers.alt
        && !keystroke.modifiers.control
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
    {
        let key = keystroke.key.as_str();
        if key.len() == 1 {
            let ch = if keystroke.modifiers.shift {
                key.chars().next().unwrap().to_ascii_uppercase()
            } else {
                key.chars().next().unwrap()
            };
            if ch.is_ascii() {
                let mut bytes = vec![b'\x1b'];
                bytes.push(ch as u8);
                return Some(bytes);
            }
        }
    }

    // Handle regular printable characters
    // Use key_char if available (contains the actual typed character with modifiers like Shift)
    if let Some(key_char) = &keystroke.key_char
        && !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
    {
        return Some(key_char.as_bytes().to_vec());
    }

    // Fallback to key for single characters
    let key = keystroke.key.as_str();
    if key.len() == 1 {
        let ch = key.chars().next().unwrap();
        if ch.is_ascii()
            && !keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.platform
            && !keystroke.modifiers.function
        {
            // Handle shift modifier for uppercase
            let ch = if keystroke.modifiers.shift {
                ch.to_ascii_uppercase()
            } else {
                ch
            };
            return Some(vec![ch as u8]);
        }
        // For non-ASCII characters, encode as UTF-8
        if !keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.platform
            && !keystroke.modifiers.function
        {
            return Some(key.as_bytes().to_vec());
        }
    }

    // If we get here, the keystroke doesn't produce any output
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enter_key() {
        let keystroke = Keystroke::parse("enter").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\r".to_vec()));
    }

    #[test]
    fn test_shift_enter_key() {
        let keystroke = Keystroke::parse("shift-enter").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\n".to_vec()));
    }

    #[test]
    fn test_escape_key() {
        let keystroke = Keystroke::parse("escape").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\x1b".to_vec()));
    }

    #[test]
    fn test_backspace_key() {
        let keystroke = Keystroke::parse("backspace").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\x7f".to_vec()));
    }

    #[test]
    fn test_modified_backspace_keys() {
        let mode = TermMode::empty();

        let ctrl_backspace = Keystroke::parse("ctrl-backspace").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_backspace, mode),
            Some(b"\x08".to_vec())
        );

        let alt_backspace = Keystroke::parse("alt-backspace").unwrap();
        assert_eq!(
            keystroke_to_bytes(&alt_backspace, mode),
            Some(b"\x1b\x7f".to_vec())
        );
    }

    #[test]
    fn test_tab_key() {
        let keystroke = Keystroke::parse("tab").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\t".to_vec()));
    }

    #[test]
    fn test_shift_tab() {
        let keystroke = Keystroke::parse("shift-tab").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn test_arrow_keys_normal_mode() {
        let mode = TermMode::empty();

        let up = Keystroke::parse("up").unwrap();
        assert_eq!(keystroke_to_bytes(&up, mode), Some(b"\x1b[A".to_vec()));

        let down = Keystroke::parse("down").unwrap();
        assert_eq!(keystroke_to_bytes(&down, mode), Some(b"\x1b[B".to_vec()));

        let right = Keystroke::parse("right").unwrap();
        assert_eq!(keystroke_to_bytes(&right, mode), Some(b"\x1b[C".to_vec()));

        let left = Keystroke::parse("left").unwrap();
        assert_eq!(keystroke_to_bytes(&left, mode), Some(b"\x1b[D".to_vec()));
    }

    #[test]
    fn test_arrow_keys_app_cursor_mode() {
        let mode = TermMode::APP_CURSOR;

        let up = Keystroke::parse("up").unwrap();
        assert_eq!(keystroke_to_bytes(&up, mode), Some(b"\x1bOA".to_vec()));

        let down = Keystroke::parse("down").unwrap();
        assert_eq!(keystroke_to_bytes(&down, mode), Some(b"\x1bOB".to_vec()));

        let right = Keystroke::parse("right").unwrap();
        assert_eq!(keystroke_to_bytes(&right, mode), Some(b"\x1bOC".to_vec()));

        let left = Keystroke::parse("left").unwrap();
        assert_eq!(keystroke_to_bytes(&left, mode), Some(b"\x1bOD".to_vec()));
    }

    #[test]
    fn test_modified_arrow_keys() {
        let mode = TermMode::empty();

        let shift_up = Keystroke::parse("shift-up").unwrap();
        assert_eq!(
            keystroke_to_bytes(&shift_up, mode),
            Some(b"\x1b[1;2A".to_vec())
        );

        let alt_left = Keystroke::parse("alt-left").unwrap();
        assert_eq!(
            keystroke_to_bytes(&alt_left, mode),
            Some(b"\x1b[1;3D".to_vec())
        );

        let ctrl_right = Keystroke::parse("ctrl-right").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_right, mode),
            Some(b"\x1b[1;5C".to_vec())
        );

        let ctrl_shift_left = Keystroke::parse("ctrl-shift-left").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_shift_left, mode),
            Some(b"\x1b[1;6D".to_vec())
        );
    }

    #[test]
    fn test_navigation_keys() {
        let mode = TermMode::empty();

        let home = Keystroke::parse("home").unwrap();
        assert_eq!(keystroke_to_bytes(&home, mode), Some(b"\x1b[H".to_vec()));

        let end = Keystroke::parse("end").unwrap();
        assert_eq!(keystroke_to_bytes(&end, mode), Some(b"\x1b[F".to_vec()));

        let pageup = Keystroke::parse("pageup").unwrap();
        assert_eq!(keystroke_to_bytes(&pageup, mode), Some(b"\x1b[5~".to_vec()));

        let pagedown = Keystroke::parse("pagedown").unwrap();
        assert_eq!(
            keystroke_to_bytes(&pagedown, mode),
            Some(b"\x1b[6~".to_vec())
        );

        let insert = Keystroke::parse("insert").unwrap();
        assert_eq!(keystroke_to_bytes(&insert, mode), Some(b"\x1b[2~".to_vec()));

        let delete = Keystroke::parse("delete").unwrap();
        assert_eq!(keystroke_to_bytes(&delete, mode), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn test_home_end_app_cursor_mode() {
        let mode = TermMode::APP_CURSOR;

        let home = Keystroke::parse("home").unwrap();
        assert_eq!(keystroke_to_bytes(&home, mode), Some(b"\x1bOH".to_vec()));

        let end = Keystroke::parse("end").unwrap();
        assert_eq!(keystroke_to_bytes(&end, mode), Some(b"\x1bOF".to_vec()));
    }

    #[test]
    fn test_modified_navigation_keys() {
        let mode = TermMode::empty();

        let shift_home = Keystroke::parse("shift-home").unwrap();
        assert_eq!(
            keystroke_to_bytes(&shift_home, mode),
            Some(b"\x1b[1;2H".to_vec())
        );

        let ctrl_delete = Keystroke::parse("ctrl-delete").unwrap();
        assert_eq!(
            keystroke_to_bytes(&ctrl_delete, mode),
            Some(b"\x1b[3;5~".to_vec())
        );
    }

    #[test]
    fn test_function_keys() {
        let mode = TermMode::empty();

        let f1 = Keystroke::parse("f1").unwrap();
        assert_eq!(keystroke_to_bytes(&f1, mode), Some(b"\x1bOP".to_vec()));

        let f2 = Keystroke::parse("f2").unwrap();
        assert_eq!(keystroke_to_bytes(&f2, mode), Some(b"\x1bOQ".to_vec()));

        let f5 = Keystroke::parse("f5").unwrap();
        assert_eq!(keystroke_to_bytes(&f5, mode), Some(b"\x1b[15~".to_vec()));

        let f12 = Keystroke::parse("f12").unwrap();
        assert_eq!(keystroke_to_bytes(&f12, mode), Some(b"\x1b[24~".to_vec()));

        let f13 = Keystroke::parse("f13").unwrap();
        assert_eq!(keystroke_to_bytes(&f13, mode), Some(b"\x1b[25~".to_vec()));
    }

    #[test]
    fn test_modified_function_key() {
        let mode = TermMode::empty();

        let shift_f1 = Keystroke::parse("shift-f1").unwrap();
        assert_eq!(
            keystroke_to_bytes(&shift_f1, mode),
            Some(b"\x1b[1;2P".to_vec())
        );

        let alt_shift_f5 = Keystroke::parse("alt-shift-f5").unwrap();
        assert_eq!(
            keystroke_to_bytes(&alt_shift_f5, mode),
            Some(b"\x1b[15;4~".to_vec())
        );
    }

    #[test]
    fn test_ctrl_combinations() {
        let mode = TermMode::empty();

        // Ctrl+A = 0x01
        let ctrl_a = Keystroke::parse("ctrl-a").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_a, mode), Some(vec![0x01]));

        // Ctrl+C = 0x03
        let ctrl_c = Keystroke::parse("ctrl-c").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_c, mode), Some(vec![0x03]));

        // Ctrl+Z = 0x1a
        let ctrl_z = Keystroke::parse("ctrl-z").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_z, mode), Some(vec![0x1a]));

        // Ctrl+Space = 0x00
        let ctrl_space = Keystroke::parse("ctrl-space").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_space, mode), Some(vec![0x00]));
    }

    #[test]
    fn test_alt_combinations() {
        let mode = TermMode::empty();

        // Alt+a sends ESC followed by 'a'
        let alt_a = Keystroke::parse("alt-a").unwrap();
        assert_eq!(keystroke_to_bytes(&alt_a, mode), Some(b"\x1ba".to_vec()));

        // Alt+x sends ESC followed by 'x'
        let alt_x = Keystroke::parse("alt-x").unwrap();
        assert_eq!(keystroke_to_bytes(&alt_x, mode), Some(b"\x1bx".to_vec()));
    }

    #[test]
    fn test_regular_characters() {
        let mode = TermMode::empty();

        let a = Keystroke::parse("a").unwrap();
        assert_eq!(keystroke_to_bytes(&a, mode), Some(b"a".to_vec()));

        let z = Keystroke::parse("z").unwrap();
        assert_eq!(keystroke_to_bytes(&z, mode), Some(b"z".to_vec()));

        let zero = Keystroke::parse("0").unwrap();
        assert_eq!(keystroke_to_bytes(&zero, mode), Some(b"0".to_vec()));
    }

    #[test]
    fn test_space_key() {
        let mode = TermMode::empty();

        let space = Keystroke::parse("space").unwrap();
        assert_eq!(keystroke_to_bytes(&space, mode), Some(b" ".to_vec()));
    }
}
