//! Translates a platform key event into the bytes a PTY-connected program
//! expects. Framework-agnostic (plain `&str`/`bool` params, not GPUI types)
//! so it's usable/testable without a GPUI dependency - see
//! `openspec/changes/terminal-rendering/specs/terminal-input/spec.md`.

use alacritty_terminal::term::TermMode;

/// Terminal input modes that affect how keys are encoded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModes {
    /// DECCKM (`APP_CURSOR`): arrow keys send SS3 (`ESC O A..D`) when unmodified.
    pub app_cursor:          bool,
    /// Kitty Keyboard Protocol flag 1 (`DISAMBIGUATE_ESC_CODES`).
    pub disambiguate_esc:    bool,
    /// Kitty Keyboard Protocol flag 8 (`REPORT_ALL_KEYS_AS_ESC`).
    pub report_all_keys:     bool,
    /// In legacy mode (when Kitty Keyboard Protocol is off), encode `Shift+Enter`
    /// as `ESC CR` (`\x1b\r`) so AI coding CLIs (Claude Code, Codex, Gemini CLI,
    /// OpenCode) insert a newline instead of submitting.
    pub shift_enter_newline: bool,
}

impl KeyModes {
    pub fn from_term_mode(mode: TermMode) -> Self {
        Self { app_cursor:          mode.contains(TermMode::APP_CURSOR),
               disambiguate_esc:    mode.contains(TermMode::DISAMBIGUATE_ESC_CODES),
               report_all_keys:     mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC),
               shift_enter_newline: true, }
    }

    fn kitty_active(self) -> bool {
        self.disambiguate_esc || self.report_all_keys
    }
}

/// One key press, named the way GPUI's `Keystroke` names keys (e.g.
/// `"a"`, `"enter"`, `"up"`, `"f1"`) - see `gpui_pre_macos::events` for the
/// canonical list this mirrors.
#[derive(Debug, Clone, Copy)]
pub struct KeyInput<'a> {
    pub key:            &'a str,
    /// The character this key would insert with no modifiers held, if any
    /// (`None` for pure control keys like a bare Cmd chord).
    pub key_char:       Option<&'a str>,
    pub control:        bool,
    pub alt:            bool,
    /// Shift held: changes Tab to back-tab, Shift+Enter to newline/CSI u, and
    /// sets xterm/Kitty modifier bits for functional and control keys.
    pub shift:          bool,
    /// Whether Option/Alt acts as Meta (ESC prefix) rather than platform
    /// character composition.
    pub option_as_meta: bool,
}

/// Returns the bytes to write to a PTY for this key press under default legacy
/// modes, or `None` if this key press shouldn't be sent at all.
pub fn key_to_bytes(input: KeyInput<'_>) -> Option<Vec<u8>> {
    key_to_bytes_with_modes(input, KeyModes::default())
}

/// Returns the bytes to write to a PTY for this key press given the active
/// terminal input [`KeyModes`].
pub fn key_to_bytes_with_modes(input: KeyInput<'_>, modes: KeyModes) -> Option<Vec<u8>> {
    let meta = input.alt && input.option_as_meta;
    let mod_param = xterm_modifier(input.shift, meta, input.control);

    if modes.kitty_active()
       && let Some(bytes) = kitty_key_bytes(input, modes, meta, mod_param)
    {
        return Some(bytes);
    }

    // Legacy Shift+Tab -> Back-tab (CBT).
    if input.key == "tab" && input.shift && !input.control && !meta {
        return Some(b"\x1b[Z".to_vec());
    }

    // Legacy Shift+Enter -> ESC CR for AI agent CLI multiline prompts.
    if input.key == "enter" && input.shift && !input.control && !meta && modes.shift_enter_newline {
        return Some(b"\x1b\r".to_vec());
    }

    if let Some(bytes) = functional_key_bytes(input.key, mod_param, meta, modes.app_cursor) {
        return Some(bytes);
    }

    if input.control {
        return control_byte(input.key).map(|byte| {
                   if meta {
                       vec![0x1b, byte]
                   }
                   else {
                       vec![byte]
                   }
               });
    }

    if input.alt {
        if input.option_as_meta {
            // Standard xterm/Meta convention: ESC followed by the base key
            // (not the macOS Option-composed glyph in `key_char`).
            let mut text = if input.key.chars().count() == 1 {
                if input.shift {
                    input.key.to_ascii_uppercase()
                }
                else {
                    input.key.to_string()
                }
            }
            else if let Some(key_char) = input.key_char {
                key_char.to_string()
            }
            else {
                return None;
            };
            if text.is_empty() {
                return None;
            }
            text.insert(0, '\u{1b}');
            return Some(text.into_bytes());
        }
        // Option as normal character composition: emit the composed `key_char`
        // directly without ESC prefix, or return `None` for dead keys so IME
        // can compose them.
        return input.key_char
                    .filter(|s| !s.is_empty())
                    .map(|s| s.as_bytes().to_vec());
    }

    // Multi-character key names with no `key_char` are modifier-only presses
    // (e.g. a bare "control"/"shift"/"cmd") or unrecognized named keys, not
    // literal text - only a single character falls back to the key name
    // itself.
    let text = match input.key_char {
        Some(key_char) => key_char.to_string(),
        None if input.key.chars().count() == 1 => input.key.to_string(),
        None => return None,
    };
    if text.is_empty() {
        None
    }
    else {
        Some(text.into_bytes())
    }
}

/// Computes the xterm/Kitty modifier parameter (1 = none, 2 = Shift, 3 = Alt,
/// 4 = Shift+Alt, 5 = Ctrl, 6 = Shift+Ctrl, 7 = Alt+Ctrl, 8 = Shift+Alt+Ctrl).
fn xterm_modifier(shift: bool, alt: bool, control: bool) -> u8 {
    let mut bits = 0u8;
    if shift {
        bits |= 1;
    }
    if alt {
        bits |= 2;
    }
    if control {
        bits |= 4;
    }
    bits + 1
}

fn csi_u(codepoint: u32, mod_param: u8) -> Vec<u8> {
    if mod_param == 1 {
        format!("\x1b[{codepoint}u").into_bytes()
    }
    else {
        format!("\x1b[{codepoint};{mod_param}u").into_bytes()
    }
}

/// Kitty Keyboard Protocol encoding (DISAMBIGUATE_ESC_CODES / REPORT_ALL_KEYS_AS_ESC).
fn kitty_key_bytes(input: KeyInput<'_>, modes: KeyModes, meta: bool, mod_param: u8) -> Option<Vec<u8>> {
    let has_mod = mod_param > 1;
    match input.key {
        "escape" => return Some(csi_u(27, mod_param)),
        "enter" if has_mod || modes.report_all_keys => return Some(csi_u(13, mod_param)),
        "tab" if has_mod || modes.report_all_keys => return Some(csi_u(9, mod_param)),
        "backspace" if has_mod || modes.report_all_keys => return Some(csi_u(127, mod_param)),
        "space" if input.control || meta || modes.report_all_keys => {
            return Some(csi_u(32, mod_param));
        }
        _ => {}
    }

    // Functional keys in Kitty protocol still use standard CSI / tilde sequences
    // with modifier params (except unmodified arrows respecting APP_CURSOR).
    if let Some(bytes) = functional_key_bytes(input.key, mod_param, meta, modes.app_cursor) {
        return Some(bytes);
    }

    // Single-character keys with Ctrl or Alt (or all keys under REPORT_ALL_KEYS_AS_ESC).
    if input.key.chars().count() == 1
       && (input.control || meta || modes.report_all_keys)
       && let Some(ch) = input.key.chars().next()
    {
        let base = ch.to_ascii_lowercase() as u32;
        return Some(csi_u(base, mod_param));
    }

    None
}

/// Named/functional keys with optional xterm modifiers and DECCKM cursor mode.
fn functional_key_bytes(key: &str, mod_param: u8, meta: bool, app_cursor: bool) -> Option<Vec<u8>> {
    if mod_param == 1 {
        let bytes: &[u8] = match key {
            "enter" => b"\r",
            "tab" => b"\t",
            "backspace" => b"\x7f",
            "escape" => b"\x1b",
            "space" => b" ",
            "up" => if app_cursor { b"\x1bOA" } else { b"\x1b[A" },
            "down" => if app_cursor { b"\x1bOB" } else { b"\x1b[B" },
            "right" => if app_cursor { b"\x1bOC" } else { b"\x1b[C" },
            "left" => if app_cursor { b"\x1bOD" } else { b"\x1b[D" },
            "home" => if app_cursor { b"\x1bOH" } else { b"\x1b[H" },
            "end" => if app_cursor { b"\x1bOF" } else { b"\x1b[F" },
            "pageup" => b"\x1b[5~",
            "pagedown" => b"\x1b[6~",
            "delete" => b"\x1b[3~",
            "insert" => b"\x1b[2~",
            "f1" => b"\x1bOP",
            "f2" => b"\x1bOQ",
            "f3" => b"\x1bOR",
            "f4" => b"\x1bOS",
            "f5" => b"\x1b[15~",
            "f6" => b"\x1b[17~",
            "f7" => b"\x1b[18~",
            "f8" => b"\x1b[19~",
            "f9" => b"\x1b[20~",
            "f10" => b"\x1b[21~",
            "f11" => b"\x1b[23~",
            "f12" => b"\x1b[24~",
            _ => return None,
        };
        return Some(bytes.to_vec());
    }

    // Modified named keys.
    match key {
        "backspace" if meta && mod_param == 3 => Some(b"\x1b\x7f".to_vec()),
        "backspace" => Some(b"\x7f".to_vec()),
        "enter" if meta && mod_param == 3 => Some(b"\x1b\r".to_vec()),
        "enter" => Some(b"\r".to_vec()),
        "tab" => Some(b"\t".to_vec()),
        "escape" => Some(b"\x1b".to_vec()),
        "space" if mod_param == 5 => Some(vec![0x00]), // Ctrl+Space -> NUL
        "space" if meta && mod_param == 3 => Some(b"\x1b ".to_vec()),
        "space" => Some(b" ".to_vec()),
        "up" => Some(format!("\x1b[1;{mod_param}A").into_bytes()),
        "down" => Some(format!("\x1b[1;{mod_param}B").into_bytes()),
        "right" => Some(format!("\x1b[1;{mod_param}C").into_bytes()),
        "left" => Some(format!("\x1b[1;{mod_param}D").into_bytes()),
        "home" => Some(format!("\x1b[1;{mod_param}H").into_bytes()),
        "end" => Some(format!("\x1b[1;{mod_param}F").into_bytes()),
        "insert" => Some(format!("\x1b[2;{mod_param}~").into_bytes()),
        "delete" => Some(format!("\x1b[3;{mod_param}~").into_bytes()),
        "pageup" => Some(format!("\x1b[5;{mod_param}~").into_bytes()),
        "pagedown" => Some(format!("\x1b[6;{mod_param}~").into_bytes()),
        "f1" => Some(format!("\x1b[1;{mod_param}P").into_bytes()),
        "f2" => Some(format!("\x1b[1;{mod_param}Q").into_bytes()),
        "f3" => Some(format!("\x1b[1;{mod_param}R").into_bytes()),
        "f4" => Some(format!("\x1b[1;{mod_param}S").into_bytes()),
        "f5" => Some(format!("\x1b[15;{mod_param}~").into_bytes()),
        "f6" => Some(format!("\x1b[17;{mod_param}~").into_bytes()),
        "f7" => Some(format!("\x1b[18;{mod_param}~").into_bytes()),
        "f8" => Some(format!("\x1b[19;{mod_param}~").into_bytes()),
        "f9" => Some(format!("\x1b[20;{mod_param}~").into_bytes()),
        "f10" => Some(format!("\x1b[21;{mod_param}~").into_bytes()),
        "f11" => Some(format!("\x1b[23;{mod_param}~").into_bytes()),
        "f12" => Some(format!("\x1b[24;{mod_param}~").into_bytes()),
        _ => None,
    }
}

/// Ctrl+letter/symbol -> its control byte (e.g. Ctrl+C -> 0x03), matching
/// the standard `key & 0x1f` mapping for `@`-`_` (and lowercase equivalents).
fn control_byte(key: &str) -> Option<u8> {
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let upper = ch.to_ascii_uppercase();
    if upper.is_ascii() && (0x3F..=0x5F).contains(&(upper as u8)) {
        Some(upper as u8 & 0x1F)
    }
    else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &str) -> KeyInput<'_> {
        KeyInput { key,
                   key_char: None,
                   control: false,
                   alt: false,
                   shift: false,
                   option_as_meta: true }
    }

    #[test]
    fn printable_letter_sends_its_key_char() {
        let input = KeyInput { key:      "a",
                               key_char: Some("a"),
                               ..key("a") };
        assert_eq!(key_to_bytes(input), Some(b"a".to_vec()));
    }

    #[test]
    fn shifted_letter_sends_the_shifted_key_char() {
        let input = KeyInput { key:      "a",
                               key_char: Some("A"),
                               ..key("a") };
        assert_eq!(key_to_bytes(input), Some(b"A".to_vec()));
    }

    #[test]
    fn ctrl_c_sends_end_of_text() {
        let input = KeyInput { control: true,
                               ..key("c") };
        assert_eq!(key_to_bytes(input), Some(vec![0x03]));
    }

    #[test]
    fn ctrl_d_sends_end_of_transmission() {
        let input = KeyInput { control: true,
                               ..key("d") };
        assert_eq!(key_to_bytes(input), Some(vec![0x04]));
    }

    #[test]
    fn enter_sends_carriage_return_not_key_char() {
        let input = KeyInput { key_char: Some("\n"),
                               ..key("enter") };
        assert_eq!(key_to_bytes(input), Some(b"\r".to_vec()));
    }

    #[test]
    fn arrow_keys_send_ansi_cursor_sequences() {
        assert_eq!(key_to_bytes(key("up")), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_to_bytes(key("down")), Some(b"\x1b[B".to_vec()));
        assert_eq!(key_to_bytes(key("left")), Some(b"\x1b[D".to_vec()));
        assert_eq!(key_to_bytes(key("right")), Some(b"\x1b[C".to_vec()));
    }

    #[test]
    fn function_keys_send_their_xterm_sequences() {
        assert_eq!(key_to_bytes(key("f1")), Some(b"\x1bOP".to_vec()));
        assert_eq!(key_to_bytes(key("f5")), Some(b"\x1b[15~".to_vec()));
    }

    #[test]
    fn alt_prefixes_with_escape() {
        let input = KeyInput { key_char: Some("∫"),
                               alt: true,
                               option_as_meta: true,
                               ..key("b") };
        assert_eq!(key_to_bytes(input), Some(b"\x1bb".to_vec()));
    }

    #[test]
    fn alt_shift_sends_uppercase_meta() {
        let input = KeyInput { key_char: Some("ı"),
                               alt: true,
                               shift: true,
                               option_as_meta: true,
                               ..key("b") };
        assert_eq!(key_to_bytes(input), Some(b"\x1bB".to_vec()));
    }

    #[test]
    fn alt_without_option_as_meta_sends_composed_char() {
        let input = KeyInput { key_char: Some("∫"),
                               alt: true,
                               option_as_meta: false,
                               ..key("b") };
        assert_eq!(key_to_bytes(input), Some("∫".as_bytes().to_vec()));
    }

    #[test]
    fn alt_named_keys_respect_option_as_meta() {
        let meta_bs = KeyInput { alt: true,
                                 option_as_meta: true,
                                 ..key("backspace") };
        assert_eq!(key_to_bytes(meta_bs), Some(b"\x1b\x7f".to_vec()));

        let meta_left = KeyInput { alt: true,
                                   option_as_meta: true,
                                   ..key("left") };
        assert_eq!(key_to_bytes(meta_left), Some(b"\x1b[1;3D".to_vec()));

        let plain_bs = KeyInput { alt: true,
                                  option_as_meta: false,
                                  ..key("backspace") };
        assert_eq!(key_to_bytes(plain_bs), Some(b"\x7f".to_vec()));
    }

    #[test]
    fn bare_modifier_key_produces_nothing() {
        let input = KeyInput { key_char: None,
                               ..key("control") };
        assert_eq!(key_to_bytes(input), None);
    }

    #[test]
    fn shift_tab_is_back_tab_and_tab_is_tab() {
        assert_eq!(key_to_bytes(key("tab")), Some(b"\t".to_vec()));
        let input = KeyInput { shift: true,
                               ..key("tab") };
        assert_eq!(key_to_bytes(input), Some(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn kitty_keyboard_protocol_encodes_shift_enter_and_disambiguates_escape() {
        let modes = KeyModes::from_term_mode(TermMode::DISAMBIGUATE_ESC_CODES);
        let shift_enter = KeyInput { shift: true,
                                     ..key("enter") };
        assert_eq!(key_to_bytes_with_modes(shift_enter, modes),
                   Some(b"\x1b[13;2u".to_vec()));
        assert_eq!(key_to_bytes_with_modes(key("enter"), modes),
                   Some(b"\r".to_vec()));
        assert_eq!(key_to_bytes_with_modes(key("escape"), modes),
                   Some(b"\x1b[27u".to_vec()));
        let ctrl_c = KeyInput { control: true,
                                ..key("c") };
        assert_eq!(key_to_bytes_with_modes(ctrl_c, modes),
                   Some(b"\x1b[99;5u".to_vec()));
    }

    #[test]
    fn legacy_shift_enter_sends_esc_cr_and_app_cursor_sends_ss3() {
        let modes = KeyModes::from_term_mode(TermMode::APP_CURSOR);
        let shift_enter = KeyInput { shift: true,
                                     ..key("enter") };
        assert_eq!(key_to_bytes_with_modes(shift_enter, modes),
                   Some(b"\x1b\r".to_vec()));
        assert_eq!(key_to_bytes_with_modes(key("up"), modes),
                   Some(b"\x1bOA".to_vec()));
        let ctrl_right = KeyInput { control: true,
                                    ..key("right") };
        assert_eq!(key_to_bytes_with_modes(ctrl_right, modes),
                   Some(b"\x1b[1;5C".to_vec()));
    }
}
