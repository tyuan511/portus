use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use async_channel::Sender;
use russh::Disconnect;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

use crate::settings::{HostProfile, PortForwardRule, TunnelKind};
use crate::ssh::{SshPrompt, SshUiEvent, connect_target_with_forwarding, runtime};

#[derive(Debug)]
pub enum TunnelEvent {
    Starting(u64),
    Active(u64),
    Stopped(u64),
    Prompt(u64, SshPrompt),
    Error(u64, String),
}

pub struct TunnelHandle {
    stop: Option<oneshot::Sender<()>>,
}

impl TunnelHandle {
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}

pub fn start_tunnel(
    rule: PortForwardRule,
    host: HostProfile,
    all_hosts: Vec<HostProfile>,
    events: Sender<TunnelEvent>,
) -> TunnelHandle {
    let (stop_tx, stop_rx) = oneshot::channel();
    runtime().spawn(async move {
        run_tunnel(rule, host, all_hosts, events, stop_rx).await;
    });
    TunnelHandle {
        stop: Some(stop_tx),
    }
}

async fn run_tunnel(
    rule: PortForwardRule,
    host: HostProfile,
    all_hosts: Vec<HostProfile>,
    events: Sender<TunnelEvent>,
    mut stop_rx: oneshot::Receiver<()>,
) {
    let id = rule.id;
    let _ = events.send(TunnelEvent::Starting(id)).await;

    let (ssh_tx, ssh_rx) = async_channel::unbounded::<SshUiEvent>();
    let bridge_events = events.clone();
    tokio::spawn(async move {
        while let Ok(ev) = ssh_rx.recv().await {
            match ev {
                SshUiEvent::Prompt(prompt) => {
                    let _ = bridge_events.send(TunnelEvent::Prompt(id, prompt)).await;
                }
                SshUiEvent::Error(err) => {
                    let _ = bridge_events.send(TunnelEvent::Error(id, err)).await;
                }
                _ => {}
            }
        }
    });

    let (fwd_tx, mut fwd_rx) = mpsc::unbounded_channel();
    let forwarded_tx = (rule.kind == TunnelKind::Remote).then_some(fwd_tx);

    let session = match connect_target_with_forwarding(&host, &all_hosts, &ssh_tx, forwarded_tx).await {
        Ok(session) => Arc::new(session),
        Err(err) => {
            let _ = events.send(TunnelEvent::Error(id, err)).await;
            let _ = events.send(TunnelEvent::Stopped(id)).await;
            return;
        }
    };

    let bind_addr = if rule.bind_address.trim().is_empty() {
        "127.0.0.1"
    } else {
        rule.bind_address.trim()
    };

    match rule.kind {
        TunnelKind::Local => {
            let listener = match TcpListener::bind((bind_addr, rule.bind_port)).await {
                Ok(l) => l,
                Err(err) => {
                    let _ = events
                        .send(TunnelEvent::Error(
                            id,
                            format!("Bind {bind_addr}:{} failed: {err}", rule.bind_port),
                        ))
                        .await;
                    let _ = events.send(TunnelEvent::Stopped(id)).await;
                    return;
                }
            };
            let _ = events.send(TunnelEvent::Active(id)).await;

            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    accepted = listener.accept() => {
                        let Ok((mut tcp, peer)) = accepted else { break };
                        let session = Arc::clone(&session);
                        let target_host = rule.target_host.clone();
                        let target_port = u32::from(rule.target_port);
                        tokio::spawn(async move {
                            if let Ok(ch) = session
                                .channel_open_direct_tcpip(
                                    target_host,
                                    target_port,
                                    peer.ip().to_string(),
                                    u32::from(peer.port()),
                                )
                                .await
                            {
                                let mut stream = ch.into_stream();
                                let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
                            }
                        });
                    }
                }
            }
        }
        TunnelKind::Dynamic => {
            let listener = match TcpListener::bind((bind_addr, rule.bind_port)).await {
                Ok(l) => l,
                Err(err) => {
                    let _ = events
                        .send(TunnelEvent::Error(
                            id,
                            format!("Bind {bind_addr}:{} failed: {err}", rule.bind_port),
                        ))
                        .await;
                    let _ = events.send(TunnelEvent::Stopped(id)).await;
                    return;
                }
            };
            let _ = events.send(TunnelEvent::Active(id)).await;

            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    accepted = listener.accept() => {
                        let Ok((tcp, peer)) = accepted else { break };
                        let session = Arc::clone(&session);
                        tokio::spawn(async move {
                            let _ = handle_socks5(tcp, peer, session).await;
                        });
                    }
                }
            }
        }
        TunnelKind::Remote => {
            if let Err(err) = session
                .tcpip_forward(bind_addr, u32::from(rule.bind_port))
                .await
            {
                let _ = events
                    .send(TunnelEvent::Error(
                        id,
                        format!("Remote forward {bind_addr}:{} failed: {err}", rule.bind_port),
                    ))
                    .await;
                let _ = events.send(TunnelEvent::Stopped(id)).await;
                return;
            }
            let _ = events.send(TunnelEvent::Active(id)).await;

            loop {
                tokio::select! {
                    _ = &mut stop_rx => {
                        let _ = session
                            .cancel_tcpip_forward(bind_addr, u32::from(rule.bind_port))
                            .await;
                        break;
                    }
                    maybe_ch = fwd_rx.recv() => {
                        let Some(ch) = maybe_ch else { break };
                        let target_host = rule.target_host.clone();
                        let target_port = rule.target_port;
                        tokio::spawn(async move {
                            if let Ok(mut tcp) = TcpStream::connect((target_host.as_str(), target_port)).await {
                                let mut stream = ch.into_stream();
                                let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
                            }
                        });
                    }
                }
            }
        }
    }

    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
    let _ = events.send(TunnelEvent::Stopped(id)).await;
}

async fn handle_socks5(
    mut tcp: TcpStream,
    peer: std::net::SocketAddr,
    session: Arc<russh::client::Handle<crate::ssh::ClientHandler>>,
) -> Result<(), String> {
    let mut header = [0u8; 2];
    tcp.read_exact(&mut header)
        .await
        .map_err(|e| e.to_string())?;
    if header[0] != 0x05 {
        return Err("Unsupported SOCKS version".into());
    }
    let mut methods = vec![0u8; usize::from(header[1])];
    tcp.read_exact(&mut methods)
        .await
        .map_err(|e| e.to_string())?;
    // Reply: version 5, no auth (0x00)
    tcp.write_all(&[0x05, 0x00])
        .await
        .map_err(|e| e.to_string())?;

    let mut req = [0u8; 4];
    tcp.read_exact(&mut req).await.map_err(|e| e.to_string())?;
    if req[0] != 0x05 || req[1] != 0x01 {
        let _ = tcp
            .write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await;
        return Err("Unsupported SOCKS5 command".into());
    }
    let atyp = req[3];
    let target_host = match atyp {
        0x01 => {
            let mut addr = [0u8; 4];
            tcp.read_exact(&mut addr)
                .await
                .map_err(|e| e.to_string())?;
            Ipv4Addr::from(addr).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            tcp.read_exact(&mut len).await.map_err(|e| e.to_string())?;
            let mut domain = vec![0u8; usize::from(len[0])];
            tcp.read_exact(&mut domain)
                .await
                .map_err(|e| e.to_string())?;
            String::from_utf8(domain).map_err(|e| e.to_string())?
        }
        0x04 => {
            let mut addr = [0u8; 16];
            tcp.read_exact(&mut addr)
                .await
                .map_err(|e| e.to_string())?;
            Ipv6Addr::from(addr).to_string()
        }
        _ => return Err("Unsupported SOCKS5 address type".into()),
    };
    let mut port_buf = [0u8; 2];
    tcp.read_exact(&mut port_buf)
        .await
        .map_err(|e| e.to_string())?;
    let target_port = u16::from_be_bytes(port_buf);

    let ch = match session
        .channel_open_direct_tcpip(
            target_host,
            u32::from(target_port),
            peer.ip().to_string(),
            u32::from(peer.port()),
        )
        .await
    {
        Ok(ch) => ch,
        Err(err) => {
            let _ = tcp
                .write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await;
            return Err(err.to_string());
        }
    };

    tcp.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
        .map_err(|e| e.to_string())?;
    let mut stream = ch.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
    Ok(())
}
