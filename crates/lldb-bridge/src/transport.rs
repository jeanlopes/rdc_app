use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{error, info, trace, warn};

/// Raw DAP message received from the adapter.
#[derive(Debug, Clone)]
pub enum DapMessage {
    Response(serde_json::Value),
    Event(serde_json::Value),
}

/// Manages the child process (codelldb) and JSON-RPC communication over stdin/stdout.
pub struct DapTransport {
    _child: Child,
    writer: Arc<Mutex<ChildStdin>>,
    seq: Arc<Mutex<u64>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
    event_tx: mpsc::Sender<serde_json::Value>,
}

impl DapTransport {
    /// Spawn codelldb and start the I/O loops.
    pub fn spawn(codelldb_path: PathBuf) -> anyhow::Result<(Self, mpsc::Receiver<serde_json::Value>)> {
        info!(path = %codelldb_path.display(), "spawning codelldb");

        let mut child = Command::new(&codelldb_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // inherit stderr so codelldb errors are visible and the pipe never blocks
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn codelldb: {}", e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("codelldb stdin not available"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("codelldb stdout not available"))?;

        let (event_tx, event_rx) = mpsc::channel::<serde_json::Value>(64);
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let writer = Arc::new(Mutex::new(stdin));
        let pending_read = Arc::clone(&pending);
        let event_tx_reader = event_tx.clone();

        // Spawn stdout reader task in its own thread with a local Tokio runtime
        // so that DapTransport::spawn() can be called from a sync context.
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create local tokio runtime for codelldb reader");
            rt.block_on(async {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut header = String::new();
                    match reader.read_line(&mut header).await {
                        Ok(0) => {
                            info!("codelldb stdout closed");
                            break;
                        }
                        Ok(_) => {
                            let header = header.trim();
                            if header.is_empty() {
                                continue;
                            }
                            let len = if header.starts_with("Content-Length: ") {
                                header["Content-Length: ".len()..].parse::<usize>().unwrap_or(0)
                            } else {
                                warn!("unexpected header: {}", header);
                                continue;
                            };
                            // consume the empty line after header
                            let mut empty = String::new();
                            let _ = reader.read_line(&mut empty).await;

                            let mut buf = vec![0u8; len];
                            if let Err(e) = reader.read_exact(&mut buf).await {
                                error!("failed to read DAP body: {}", e);
                                break;
                            }
                            let body = match String::from_utf8(buf) {
                                Ok(s) => s,
                                Err(e) => {
                                    error!("invalid utf8 in DAP body: {}", e);
                                    continue;
                                }
                            };
                            trace!("dap-recv: {}", body);
                            let msg: serde_json::Value = match serde_json::from_str(&body) {
                                Ok(v) => v,
                                Err(e) => {
                                    error!("failed to parse DAP JSON: {}", e);
                                    continue;
                                }
                            };

                            if msg.get("type") == Some(&serde_json::Value::String("response".to_string())) {
                                let request_seq = msg.get("request_seq").and_then(|v| v.as_u64()).unwrap_or(0);
                                let mut guard = pending_read.lock().await;
                                if let Some(tx) = guard.remove(&request_seq) {
                                    let _ = tx.send(msg);
                                } else {
                                    warn!("received response for unknown request_seq {}", request_seq);
                                }
                            } else if msg.get("type") == Some(&serde_json::Value::String("event".to_string())) {
                                if event_tx_reader.send(msg).await.is_err() {
                                    info!("event receiver dropped, exiting reader");
                                    break;
                                }
                            } else {
                                trace!("unknown DAP message type: {:?}", msg.get("type"));
                            }
                        }
                        Err(e) => {
                            error!("error reading from codelldb stdout: {}", e);
                            break;
                        }
                    }
                }
            });
        });

        Ok((
            DapTransport {
                _child: child,
                writer,
                seq: Arc::new(Mutex::new(1)),
                pending,
                event_tx,
            },
            event_rx,
        ))
    }

    /// Send a DAP request and wait for the matching response.
    pub async fn request(
        &self,
        command: &str,
        arguments: Option<serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        let seq = {
            let mut g = self.seq.lock().await;
            let s = *g;
            *g += 1;
            s
        };

        let mut body = serde_json::Map::new();
        body.insert("seq".to_string(), seq.into());
        body.insert("type".to_string(), "request".into());
        body.insert("command".to_string(), command.into());
        if let Some(args) = arguments {
            body.insert("arguments".to_string(), args);
        }
        let json = serde_json::Value::Object(body);
        let payload = serde_json::to_string(&json)?;
        let full = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);

        trace!("dap-send: {}", payload);

        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.pending.lock().await;
            guard.insert(seq, tx);
        }

        {
            let mut writer = self.writer.lock().await;
            writer.write_all(full.as_bytes()).await?;
            writer.flush().await?;
        }

        let response = rx.await.map_err(|_| anyhow::anyhow!("DAP response channel dropped"))?;
        Ok(response)
    }
}
