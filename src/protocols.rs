use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_terminal::{ExitReport, GridSize, PtyCommand, PtyTransport, TerminalSink, Transport};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::settings::HostProfile;
use crate::ssh::runtime;

// --- Telnet Transport ---

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_NAWS: u8 = 31;

enum TelnetCmd {
    Data(Vec<u8>),
    Resize(GridSize),
    Terminate,
}

pub struct TelnetTransport {
    tx: mpsc::UnboundedSender<TelnetCmd>,
}

impl TelnetTransport {
    pub fn spawn(address: String, port: u16, sink: TerminalSink) -> gpui_terminal::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        runtime().spawn(async move {
            run_telnet(address, port, sink, rx).await;
        });
        Ok(Self { tx })
    }
}

impl Transport for TelnetTransport {
    fn write(&mut self, bytes: &[u8]) -> gpui_terminal::Result<()> {
        self.tx
            .send(TelnetCmd::Data(bytes.to_vec()))
            .map_err(|_| gpui_terminal::Error::Closed)
    }

    fn resize(&mut self, size: GridSize) -> gpui_terminal::Result<()> {
        let _ = self.tx.send(TelnetCmd::Resize(size));
        Ok(())
    }

    fn terminate(&mut self) -> gpui_terminal::Result<()> {
        let _ = self.tx.send(TelnetCmd::Terminate);
        Ok(())
    }
}

#[allow(dead_code)]
pub fn parse_telnet_stream(input: &[u8], replies: &mut Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    parse_telnet_stream_into(input, replies, &mut out);
    out
}

fn parse_telnet_stream_into(input: &[u8], replies: &mut Vec<u8>, out: &mut Vec<u8>) {
    out.clear();
    replies.clear();
    let mut i = 0;
    while i < input.len() {
        if input[i] != IAC {
            out.push(input[i]);
            i += 1;
            continue;
        }
        if i + 1 >= input.len() {
            break;
        }
        let cmd = input[i + 1];
        match cmd {
            IAC => {
                out.push(IAC);
                i += 2;
            }
            WILL | WONT | DO | DONT => {
                if i + 2 >= input.len() {
                    break;
                }
                let opt = input[i + 2];
                match (cmd, opt) {
                    (DO, OPT_NAWS) | (DO, OPT_SGA) => replies.extend_from_slice(&[IAC, WILL, opt]),
                    (DO, _) => replies.extend_from_slice(&[IAC, WONT, opt]),
                    (WILL, OPT_ECHO) | (WILL, OPT_SGA) => {
                        replies.extend_from_slice(&[IAC, DO, opt]);
                    }
                    (WILL, _) => replies.extend_from_slice(&[IAC, DONT, opt]),
                    _ => {}
                }
                i += 3;
            }
            SB => {
                if let Some(end) = input[i + 2..].windows(2).position(|w| w == [IAC, SE]) {
                    i += 2 + end + 2;
                } else {
                    break;
                }
            }
            _ => i += 2,
        }
    }
}

fn naws_packet(size: GridSize) -> [u8; 9] {
    let w = u16::try_from(size.columns.max(1)).unwrap_or(80).to_be_bytes();
    let h = u16::try_from(size.rows.max(1)).unwrap_or(24).to_be_bytes();
    [IAC, SB, OPT_NAWS, w[0], w[1], h[0], h[1], IAC, SE]
}

async fn run_telnet(
    address: String,
    port: u16,
    sink: TerminalSink,
    mut rx: mpsc::UnboundedReceiver<TelnetCmd>,
) {
    let mut stream = match TcpStream::connect((address.as_str(), port)).await {
        Ok(s) => s,
        Err(err) => {
            sink.output(format!("\r\nTelnet connection failed: {err}\r\n").as_bytes());
            sink.exited(ExitReport::new(None));
            return;
        }
    };

    let mut buf = [0u8; 4096];
    let mut replies = Vec::with_capacity(32);
    let mut clean = Vec::with_capacity(buf.len());
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    Some(TelnetCmd::Data(bytes)) => {
                        if stream.write_all(&bytes).await.is_err() {
                            break;
                        }
                    }
                    Some(TelnetCmd::Resize(size)) => {
                        let pkt = naws_packet(size);
                        let _ = stream.write_all(&pkt).await;
                    }
                    Some(TelnetCmd::Terminate) | None => {
                        let _ = stream.shutdown().await;
                        break;
                    }
                }
            }
            read = stream.read(&mut buf) => {
                match read {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        parse_telnet_stream_into(&buf[..n], &mut replies, &mut clean);
                        if !replies.is_empty() {
                            let _ = stream.write_all(&replies).await;
                        }
                        if !clean.is_empty() {
                            sink.output(&clean);
                        }
                    }
                }
            }
        }
    }
    sink.exited(ExitReport::new(Some(0)));
}

// --- Serial Transport ---

pub struct SerialTransport {
    writer: Option<Box<dyn serialport::SerialPort>>,
    closed: Arc<AtomicBool>,
}

impl SerialTransport {
    pub fn spawn(
        device: &str,
        baud_rate: u32,
        sink: TerminalSink,
    ) -> gpui_terminal::Result<Self> {
        let port = serialport::new(device, baud_rate)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| gpui_terminal::Error::transport(std::io::Error::other(e.to_string())))?;
        let mut reader = port
            .try_clone()
            .map_err(|e| gpui_terminal::Error::transport(std::io::Error::other(e.to_string())))?;
        let closed = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&closed);

        std::thread::Builder::new()
            .name("portus-serial".into())
            .spawn(move || {
                let mut buf = [0u8; 1024];
                while !stop.load(Ordering::Relaxed) {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => sink.output(&buf[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                        Err(_) => break,
                    }
                }
                sink.exited(ExitReport::new(Some(0)));
            })
            .map_err(gpui_terminal::Error::Io)?;

        Ok(Self {
            writer: Some(port),
            closed,
        })
    }
}

impl Transport for SerialTransport {
    fn write(&mut self, bytes: &[u8]) -> gpui_terminal::Result<()> {
        let Some(writer) = &mut self.writer else {
            return Err(gpui_terminal::Error::Closed);
        };
        writer.write_all(bytes).map_err(gpui_terminal::Error::Io)?;
        writer.flush().map_err(gpui_terminal::Error::Io)
    }

    fn resize(&mut self, _size: GridSize) -> gpui_terminal::Result<()> {
        Ok(())
    }

    fn terminate(&mut self) -> gpui_terminal::Result<()> {
        self.closed.store(true, Ordering::Relaxed);
        self.writer.take();
        Ok(())
    }
}

// --- Mosh (via local mosh binary in PTY) ---

pub fn spawn_mosh_pty(host: &HostProfile, sink: TerminalSink) -> gpui_terminal::Result<PtyTransport> {
    let mut ssh_cmd = format!("ssh -p {}", host.port);
    if !host.identity_file.trim().is_empty() {
        ssh_cmd.push_str(&format!(" -i {}", host.identity_file.trim()));
    }
    let mut cmd = PtyCommand::new("mosh")
        .arg("--ssh")
        .arg(ssh_cmd)
        .arg(format!("{}@{}", host.username, host.address))
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor");
    if let Some(home) = dirs::home_dir() {
        cmd = cmd.cwd(home);
    }
    PtyTransport::spawn(&cmd, sink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_telnet_iac_negotiation() {
        let mut replies = Vec::new();
        let data = [
            b'H', b'i', IAC, DO, OPT_NAWS, IAC, WILL, OPT_ECHO, IAC, DO, 99, b'!',
        ];
        let clean = parse_telnet_stream(&data, &mut replies);
        assert_eq!(clean, b"Hi!");
        assert_eq!(
            replies,
            vec![IAC, WILL, OPT_NAWS, IAC, DO, OPT_ECHO, IAC, WONT, 99]
        );
    }
}
