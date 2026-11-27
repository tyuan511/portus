//! Streaming sideband OSC parser for modern AI-agent CLI protocols:
//! - OSC 7 (current working directory)
//! - OSC 9 (iTerm2/ConEmu notifications) and OSC 9;4 (task progress reporting)
//! - OSC 99 (Kitty desktop notifications, including multi-part payloads)
//! - OSC 777 (rxvt/VTE desktop notifications)
//! - OSC 133 (FinalTerm / shell integration prompt & command markers)
//! - tmux DCS passthrough (`ESC P tmux; ... ESC \`) unwrapping

use std::collections::HashMap;

const MAX_OSC_BYTES: usize = 64 * 1024;
const MAX_DCS_BYTES: usize = 64 * 1024;
const MAX_KITTY_PENDING: usize = 16;

/// Progress bar state reported via `OSC 9;4;state;progress`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TerminalProgress {
    #[default]
    Hidden,
    Normal(u8),
    Error(Option<u8>),
    Indeterminate,
    Paused(Option<u8>),
}

impl TerminalProgress {
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Hidden)
    }
}

/// Shell integration markers reported via `OSC 133;A|B|C|D`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMark {
    /// `OSC 133;A` - start of shell prompt.
    PromptStart,
    /// `OSC 133;B` - end of prompt / start of user input.
    PromptEnd,
    /// `OSC 133;C` - start of command execution / output.
    CommandStart,
    /// `OSC 133;D[;exit_code]` - end of command execution.
    CommandFinished(Option<i32>),
}

/// Parsed sideband OSC event emitted during `Grid::feed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscEvent {
    Cwd(String),
    Progress(TerminalProgress),
    Notification {
        title: Option<String>,
        body:  String,
    },
    ShellMark(ShellMark),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    Ground,
    Escape,
    Osc,
    OscEscape,
    Dcs,
    DcsEscape,
}

#[derive(Debug, Default)]
struct KittyNotifAcc {
    title: String,
    body:  String,
}

/// Stateful, chunk-boundary-safe OSC scanner.
#[derive(Debug)]
pub struct OscScanner {
    state:         ScanState,
    osc_buf:       Vec<u8>,
    dcs_buf:       Vec<u8>,
    kitty_pending: HashMap<String, KittyNotifAcc>,
}

impl Default for OscScanner {
    fn default() -> Self {
        Self { state:         ScanState::Ground,
               osc_buf:       Vec::new(),
               dcs_buf:       Vec::new(),
               kitty_pending: HashMap::new(), }
    }
}

impl OscScanner {
    pub fn advance(&mut self, bytes: &[u8], out: &mut Vec<OscEvent>) {
        self.advance_inner(bytes, out, true);
    }

    fn advance_inner(&mut self, bytes: &[u8], out: &mut Vec<OscEvent>, allow_dcs: bool) {
        for &byte in bytes {
            match self.state {
                ScanState::Ground => {
                    if byte == 0x1b {
                        self.state = ScanState::Escape;
                    }
                    else if byte == 0x9d {
                        self.osc_buf.clear();
                        self.state = ScanState::Osc;
                    }
                    else if allow_dcs && byte == 0x90 {
                        self.dcs_buf.clear();
                        self.state = ScanState::Dcs;
                    }
                }
                ScanState::Escape => {
                    if byte == b']' {
                        self.osc_buf.clear();
                        self.state = ScanState::Osc;
                    }
                    else if allow_dcs && byte == b'P' {
                        self.dcs_buf.clear();
                        self.state = ScanState::Dcs;
                    }
                    else if byte == 0x1b {
                        self.state = ScanState::Escape;
                    }
                    else {
                        self.state = ScanState::Ground;
                    }
                }
                ScanState::Osc => {
                    if byte == 0x07 || byte == 0x9c {
                        self.dispatch_osc(out);
                        self.state = ScanState::Ground;
                    }
                    else if byte == 0x1b {
                        self.state = ScanState::OscEscape;
                    }
                    else if self.osc_buf.len() < MAX_OSC_BYTES {
                        self.osc_buf.push(byte);
                    }
                }
                ScanState::OscEscape => {
                    if byte == b'\\' {
                        self.dispatch_osc(out);
                        self.state = ScanState::Ground;
                    }
                    else if byte == b']' {
                        // Abandoned previous OSC; start a new one.
                        self.osc_buf.clear();
                        self.state = ScanState::Osc;
                    }
                    else if byte == 0x1b {
                        self.state = ScanState::Escape;
                    }
                    else {
                        self.osc_buf.clear();
                        self.state = ScanState::Ground;
                    }
                }
                ScanState::Dcs => {
                    if byte == 0x9c {
                        self.dispatch_dcs(out);
                        self.state = ScanState::Ground;
                    }
                    else if byte == 0x1b {
                        self.state = ScanState::DcsEscape;
                    }
                    else if self.dcs_buf.len() < MAX_DCS_BYTES {
                        self.dcs_buf.push(byte);
                    }
                }
                ScanState::DcsEscape => {
                    if byte == b'\\' {
                        self.dispatch_dcs(out);
                        self.state = ScanState::Ground;
                    }
                    else if byte == 0x1b {
                        // Inside tmux DCS passthrough, ESC is doubled as ESC ESC.
                        if self.dcs_buf.len() + 2 <= MAX_DCS_BYTES {
                            self.dcs_buf.push(0x1b);
                            self.dcs_buf.push(0x1b);
                        }
                        self.state = ScanState::Dcs;
                    }
                    else {
                        if self.dcs_buf.len() + 2 <= MAX_DCS_BYTES {
                            self.dcs_buf.push(0x1b);
                            self.dcs_buf.push(byte);
                        }
                        self.state = ScanState::Dcs;
                    }
                }
            }
        }
    }

    fn dispatch_dcs(&mut self, out: &mut Vec<OscEvent>) {
        let payload = std::mem::take(&mut self.dcs_buf);
        let Some(rest) = payload.strip_prefix(b"tmux;")
        else {
            return;
        };
        // Unescape doubled ESC inside tmux passthrough.
        let mut unwrapped = Vec::with_capacity(rest.len());
        let mut i = 0;
        while i < rest.len() {
            if rest[i] == 0x1b && i + 1 < rest.len() && rest[i + 1] == 0x1b {
                unwrapped.push(0x1b);
                i += 2;
            }
            else {
                unwrapped.push(rest[i]);
                i += 1;
            }
        }
        let mut inner = OscScanner { state:         ScanState::Ground,
                                     osc_buf:       Vec::new(),
                                     dcs_buf:       Vec::new(),
                                     kitty_pending: std::mem::take(&mut self.kitty_pending), };
        inner.advance_inner(&unwrapped, out, false);
        self.kitty_pending = inner.kitty_pending;
    }

    fn dispatch_osc(&mut self, out: &mut Vec<OscEvent>) {
        let raw = std::mem::take(&mut self.osc_buf);
        let Ok(text) = std::str::from_utf8(&raw)
        else {
            return;
        };
        let Some((code, rest)) = text.split_once(';')
        else {
            return;
        };
        match code {
            "7" => {
                if let Some(cwd) = parse_osc7_cwd(rest) {
                    out.push(OscEvent::Cwd(cwd));
                }
            }
            "9" => {
                if let Some(prog_str) = rest.strip_prefix("4;") {
                    if let Some(progress) = parse_osc9_4_progress(prog_str) {
                        out.push(OscEvent::Progress(progress));
                    }
                }
                else {
                    let body = rest.trim();
                    if !body.is_empty() {
                        out.push(OscEvent::Notification { title: None,
                                                          body:  body.to_owned(), });
                    }
                }
            }
            "99" => {
                if let Some(event) = self.parse_osc99_kitty(rest) {
                    out.push(event);
                }
            }
            "777" => {
                if let Some(event) = parse_osc777(rest) {
                    out.push(event);
                }
            }
            "133" => {
                if let Some(mark) = parse_osc133(rest) {
                    out.push(OscEvent::ShellMark(mark));
                }
            }
            _ => {}
        }
    }

    fn parse_osc99_kitty(&mut self, rest: &str) -> Option<OscEvent> {
        let (meta, payload) = rest.split_once(';')?;
        let mut id = String::new();
        let mut done = true;
        let mut part = "title";
        let mut encoded_b64 = false;

        for pair in meta.split(':') {
            if pair.is_empty() {
                continue;
            }
            let Some((k, v)) = pair.split_once('=')
            else {
                continue;
            };
            match k {
                "i" => id = v.to_owned(),
                "d" => done = v != "0",
                "p" => part = v,
                "e" => encoded_b64 = v == "1",
                _ => {}
            }
        }

        let decoded = if encoded_b64 {
            decode_base64_utf8(payload)?
        }
        else {
            payload.to_owned()
        };

        if self.kitty_pending.len() >= MAX_KITTY_PENDING && !self.kitty_pending.contains_key(&id) {
            self.kitty_pending.clear();
        }
        let acc = self.kitty_pending.entry(id.clone()).or_default();
        match part {
            "body" => acc.body.push_str(&decoded),
            "title" => acc.title.push_str(&decoded),
            _ => {}
        }

        if !done {
            return None;
        }
        let finished = self.kitty_pending.remove(&id).unwrap_or_default();
        let title = finished.title.trim().to_owned();
        let body = finished.body.trim().to_owned();
        if title.is_empty() && body.is_empty() {
            return None;
        }
        if body.is_empty() {
            Some(OscEvent::Notification { title: None,
                                          body:  title, })
        }
        else {
            Some(OscEvent::Notification { title: (!title.is_empty()).then_some(title),
                                          body })
        }
    }
}

fn parse_osc7_cwd(uri: &str) -> Option<String> {
    let stripped = uri.strip_prefix("file://")?;
    let slash_idx = stripped.find('/')?;
    let path_encoded = &stripped[slash_idx..];
    let decoded = percent_decode(path_encoded);
    if decoded.is_empty() {
        None
    }
    else {
        Some(decoded)
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
           && i + 2 < bytes.len()
           && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_osc9_4_progress(rest: &str) -> Option<TerminalProgress> {
    let mut parts = rest.split(';');
    let state = parts.next()?.trim();
    let pct = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<u8>().ok())
        .map(|v| v.min(100));
    match state {
        "0" => Some(TerminalProgress::Hidden),
        "1" => Some(TerminalProgress::Normal(pct.unwrap_or(0))),
        "2" => Some(TerminalProgress::Error(pct)),
        "3" => Some(TerminalProgress::Indeterminate),
        "4" => Some(TerminalProgress::Paused(pct)),
        _ => None,
    }
}

fn parse_osc777(rest: &str) -> Option<OscEvent> {
    let after = rest.strip_prefix("notify;")?;
    let (title, body) = after.split_once(';')?;
    let title = title.trim();
    let body = body.trim();
    if title.is_empty() && body.is_empty() {
        return None;
    }
    if body.is_empty() {
        Some(OscEvent::Notification { title: None,
                                      body:  title.to_owned(), })
    }
    else {
        Some(OscEvent::Notification { title: (!title.is_empty()).then_some(title.to_owned()),
                                      body:  body.to_owned(), })
    }
}

fn parse_osc133(rest: &str) -> Option<ShellMark> {
    let mut parts = rest.split(';');
    let kind = parts.next()?.trim();
    match kind {
        "A" => Some(ShellMark::PromptStart),
        "B" => Some(ShellMark::PromptEnd),
        "C" => Some(ShellMark::CommandStart),
        "D" => {
            let code = parts
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse::<i32>().ok());
            Some(ShellMark::CommandFinished(code))
        }
        _ => None,
    }
}

fn decode_base64_utf8(input: &str) -> Option<String> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u8 = 0;
    for byte in input.bytes() {
        let val = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_chunks(chunks: &[&[u8]]) -> Vec<OscEvent> {
        let mut scanner = OscScanner::default();
        let mut events = Vec::new();
        for chunk in chunks {
            scanner.advance(chunk, &mut events);
        }
        events
    }

    #[test]
    fn parses_osc7_cwd_with_percent_encoding_across_chunks() {
        let events = scan_chunks(&[b"\x1b]7;file://macbook.local/Users/dev/my",
                                   b"%20repo/portus\x07"]);
        assert_eq!(events,
                   vec![OscEvent::Cwd("/Users/dev/my repo/portus".into())]);
    }

    #[test]
    fn parses_osc9_notifications_and_osc9_4_progress() {
        let events = scan_chunks(&[b"\x1b]9;Claude needs permission\x07",
                                   b"\x1b]9;4;1;65\x1b\\",
                                   b"\x1b]9;4;3\x07",
                                   b"\x1b]9;4;2;100\x07",
                                   b"\x1b]9;4;0\x07"]);
        assert_eq!(events,
                   vec![OscEvent::Notification { title: None,
                                                 body:  "Claude needs permission".into(), },
                        OscEvent::Progress(TerminalProgress::Normal(65)),
                        OscEvent::Progress(TerminalProgress::Indeterminate),
                        OscEvent::Progress(TerminalProgress::Error(Some(100))),
                        OscEvent::Progress(TerminalProgress::Hidden),]);
    }

    #[test]
    fn parses_osc99_multipart_kitty_notifications() {
        let events = scan_chunks(&[b"\x1b]99;i=job1:d=0:p=title;Claude Code\x1b\\",
                                   b"\x1b]99;i=job1:d=1:p=body;Approve bash command?\x1b\\"]);
        assert_eq!(events,
                   vec![OscEvent::Notification { title: Some("Claude Code".into()),
                                                 body:  "Approve bash command?".into(), }]);
    }

    #[test]
    fn parses_osc777_and_osc133_markers() {
        let events = scan_chunks(&[b"\x1b]777;notify;Codex;Tests passed\x07",
                                   b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x07",
                                   b"\x1b]133;D;127\x1b\\"]);
        assert_eq!(events,
                   vec![OscEvent::Notification { title: Some("Codex".into()),
                                                 body:  "Tests passed".into(), },
                        OscEvent::ShellMark(ShellMark::PromptStart),
                        OscEvent::ShellMark(ShellMark::PromptEnd),
                        OscEvent::ShellMark(ShellMark::CommandStart),
                        OscEvent::ShellMark(ShellMark::CommandFinished(Some(0))),
                        OscEvent::ShellMark(ShellMark::CommandFinished(Some(127))),]);
    }

    #[test]
    fn unwraps_tmux_dcs_passthrough() {
        let events = scan_chunks(&[b"\x1bPtmux;\x1b\x1b]9;Remote agent finished\x07\x1b\\"]);
        assert_eq!(events,
                   vec![OscEvent::Notification { title: None,
                                                 body:  "Remote agent finished".into(), }]);
    }
}
