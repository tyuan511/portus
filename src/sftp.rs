use std::path::PathBuf;

use async_channel::Sender;
use russh::Disconnect;
use russh_sftp::client::SftpSession;
use tokio::sync::mpsc;

use crate::settings::HostProfile;
use crate::ssh::{SshPrompt, SshUiEvent, connect_target, runtime};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: Option<u32>,
}

#[derive(Debug)]
pub enum SftpEvent {
    Connecting(String),
    Prompt(SshPrompt),
    Ready {
        path: String,
        entries: Vec<RemoteEntry>,
    },
    Listed {
        path: String,
        entries: Vec<RemoteEntry>,
    },
    Status(String),
    Error(String),
}

pub enum SftpRequest {
    ListDir(String),
    Upload { local: PathBuf, remote: String },
    Download { remote: String, local: PathBuf },
    Mkdir(String),
    Remove { path: String, is_dir: bool },
    Close,
}

pub struct SftpClient {
    tx: mpsc::UnboundedSender<SftpRequest>,
}

impl SftpClient {
    pub fn spawn(
        host: HostProfile,
        all_hosts: Vec<HostProfile>,
        events: Sender<SftpEvent>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        runtime().spawn(async move {
            run_sftp(host, all_hosts, events, rx).await;
        });
        Self { tx }
    }

    pub fn send(&self, req: SftpRequest) {
        let _ = self.tx.send(req);
    }
}

pub fn join_remote_path(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", dir.trim_end_matches('/'))
    }
}

pub fn parent_remote_path(dir: &str) -> String {
    let trimmed = dir.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return "/".into();
    }
    match trimmed.rsplit_once('/') {
        Some(("", _)) => "/".into(),
        Some((parent, _)) => parent.to_owned(),
        None => "/".into(),
    }
}

async fn read_remote_dir(sftp: &SftpSession, path: &str) -> Result<Vec<RemoteEntry>, String> {
    let dir = sftp.read_dir(path).await.map_err(|e| e.to_string())?;
    let mut entries: Vec<RemoteEntry> = dir
        .filter_map(|entry| {
            let name = entry.file_name();
            if name == "." || name == ".." {
                return None;
            }
            let meta = entry.metadata();
            let is_dir = meta.is_dir();
            let size = meta.size.unwrap_or(0);
            let permissions = meta.permissions;
            Some(RemoteEntry {
                path: join_remote_path(path, &name),
                name,
                is_dir,
                size,
                permissions,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

async fn run_sftp(
    host: HostProfile,
    all_hosts: Vec<HostProfile>,
    events: Sender<SftpEvent>,
    mut rx: mpsc::UnboundedReceiver<SftpRequest>,
) {
    let (ssh_tx, ssh_rx) = async_channel::unbounded::<SshUiEvent>();
    let bridge_events = events.clone();
    tokio::spawn(async move {
        while let Ok(ev) = ssh_rx.recv().await {
            match ev {
                SshUiEvent::Connecting(target) => {
                    let _ = bridge_events.send(SftpEvent::Connecting(target)).await;
                }
                SshUiEvent::Prompt(prompt) => {
                    let _ = bridge_events.send(SftpEvent::Prompt(prompt)).await;
                }
                SshUiEvent::Error(err) => {
                    let _ = bridge_events.send(SftpEvent::Error(err)).await;
                }
                SshUiEvent::Connected | SshUiEvent::Telemetry(_) => {}
            }
        }
    });

    let session = match connect_target(&host, &all_hosts, &ssh_tx).await {
        Ok(session) => session,
        Err(err) => {
            let _ = events.send(SftpEvent::Error(err)).await;
            return;
        }
    };

    let channel = match session.channel_open_session().await {
        Ok(ch) => ch,
        Err(err) => {
            let _ = events
                .send(SftpEvent::Error(format!("Open channel failed: {err}")))
                .await;
            return;
        }
    };
    if let Err(err) = channel.request_subsystem(true, "sftp").await {
        let _ = events
            .send(SftpEvent::Error(format!("SFTP subsystem failed: {err}")))
            .await;
        return;
    }

    let sftp = match SftpSession::new(channel.into_stream()).await {
        Ok(sftp) => sftp,
        Err(err) => {
            let _ = events
                .send(SftpEvent::Error(format!("SFTP init failed: {err}")))
                .await;
            return;
        }
    };

    let home = sftp
        .canonicalize(".")
        .await
        .unwrap_or_else(|_| "/".to_owned());
    match read_remote_dir(&sftp, &home).await {
        Ok(entries) => {
            let _ = events
                .send(SftpEvent::Ready {
                    path: home,
                    entries,
                })
                .await;
        }
        Err(err) => {
            let _ = events.send(SftpEvent::Error(err)).await;
        }
    }

    while let Some(req) = rx.recv().await {
        match req {
            SftpRequest::ListDir(path) => match read_remote_dir(&sftp, &path).await {
                Ok(entries) => {
                    let _ = events.send(SftpEvent::Listed { path, entries }).await;
                }
                Err(err) => {
                    let _ = events.send(SftpEvent::Error(err)).await;
                }
            },
            SftpRequest::Upload { local, remote } => {
                let name = local
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let res: Result<(), String> = async {
                    let mut src = tokio::fs::File::open(&local)
                        .await
                        .map_err(|e| e.to_string())?;
                    let mut dst = sftp.create(&remote).await.map_err(|e| e.to_string())?;
                    tokio::io::copy(&mut src, &mut dst)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok(())
                }
                .await;
                match res {
                    Ok(()) => {
                        let _ = events
                            .send(SftpEvent::Status(format!("Uploaded {name}")))
                            .await;
                        let parent = parent_remote_path(&remote);
                        if let Ok(entries) = read_remote_dir(&sftp, &parent).await {
                            let _ = events
                                .send(SftpEvent::Listed {
                                    path: parent,
                                    entries,
                                })
                                .await;
                        }
                    }
                    Err(err) => {
                        let _ = events
                            .send(SftpEvent::Error(format!("Upload {name} failed: {err}")))
                            .await;
                    }
                }
            }
            SftpRequest::Download { remote, local } => {
                let name = local
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let res: Result<(), String> = async {
                    let mut src = sftp.open(&remote).await.map_err(|e| e.to_string())?;
                    let mut dst = tokio::fs::File::create(&local)
                        .await
                        .map_err(|e| e.to_string())?;
                    tokio::io::copy(&mut src, &mut dst)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok(())
                }
                .await;
                match res {
                    Ok(()) => {
                        let _ = events
                            .send(SftpEvent::Status(format!("Downloaded {name}")))
                            .await;
                    }
                    Err(err) => {
                        let _ = events
                            .send(SftpEvent::Error(format!("Download {name} failed: {err}")))
                            .await;
                    }
                }
            }
            SftpRequest::Mkdir(path) => match sftp.create_dir(&path).await {
                Ok(()) => {
                    let parent = parent_remote_path(&path);
                    if let Ok(entries) = read_remote_dir(&sftp, &parent).await {
                        let _ = events
                            .send(SftpEvent::Listed {
                                path: parent,
                                entries,
                            })
                            .await;
                    }
                }
                Err(err) => {
                    let _ = events.send(SftpEvent::Error(err.to_string())).await;
                }
            },
            SftpRequest::Remove { path, is_dir } => {
                let res = if is_dir {
                    sftp.remove_dir(&path).await
                } else {
                    sftp.remove_file(&path).await
                };
                match res {
                    Ok(()) => {
                        let parent = parent_remote_path(&path);
                        if let Ok(entries) = read_remote_dir(&sftp, &parent).await {
                            let _ = events
                                .send(SftpEvent::Listed {
                                    path: parent,
                                    entries,
                                })
                                .await;
                        }
                    }
                    Err(err) => {
                        let _ = events.send(SftpEvent::Error(err.to_string())).await;
                    }
                }
            }
            SftpRequest::Close => {
                let _ = sftp.close().await;
                let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_and_walks_remote_paths() {
        assert_eq!(join_remote_path("/", "etc"), "/etc");
        assert_eq!(join_remote_path("/var/log/", "syslog"), "/var/log/syslog");
        assert_eq!(parent_remote_path("/var/log/syslog"), "/var/log");
        assert_eq!(parent_remote_path("/var"), "/");
        assert_eq!(parent_remote_path("/"), "/");
    }
}
