use crate::engine::{Player, PlayerEvent};
use anyhow::{anyhow, Context, Result};
use interprocess::local_socket::{
    ConnectOptions, GenericFilePath, GenericNamespaced, Name, Stream, ToFsName, ToNsName,
};
use interprocess::TryClone as _;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RPC_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn build_loadfile(path: &Path, start: Option<f64>) -> Vec<String> {
    let mut command = vec![
        "loadfile".to_string(),
        path.to_string_lossy().into_owned(),
        "replace".to_string(),
    ];
    if let Some(start) = start {
        command.push(format!("start=+{start}"));
    }
    command
}

pub(crate) fn parse_event(line: &str) -> Option<PlayerEvent> {
    let value: Value = serde_json::from_str(line).ok()?;
    let is_end = value.get("event").and_then(Value::as_str) == Some("end-file");
    let is_eof = value.get("reason").and_then(Value::as_str) == Some("eof");
    (is_end && is_eof).then_some(PlayerEvent::FileEnded)
}

pub(crate) fn parse_response(line: &str) -> Option<(u64, Result<Value>)> {
    let value: Value = serde_json::from_str(line).ok()?;
    let id = value.get("request_id")?.as_u64()?;
    let error = value.get("error").and_then(Value::as_str).unwrap_or("missing error field");
    if error == "success" {
        Some((id, Ok(value.get("data").cloned().unwrap_or(Value::Null))))
    } else {
        Some((id, Err(anyhow!("mpv error: {error}"))))
    }
}

fn build_request(command: Value, request_id: u64) -> String {
    let mut object = serde_json::Map::new();
    object.insert("command".into(), command);
    object.insert("request_id".into(), Value::from(request_id));
    Value::Object(object).to_string() + "\n"
}

fn ipc_server_arg() -> (String, Name<'static>) {
    if cfg!(windows) {
        let name = "foltv-mpv".to_ns_name::<GenericNamespaced>().unwrap();
        (r"\\.\pipe\foltv-mpv".to_string(), name)
    } else {
        let name = "/tmp/foltv-mpv.sock".to_fs_name::<GenericFilePath>().unwrap();
        ("/tmp/foltv-mpv.sock".to_string(), name)
    }
}

type Pending = Arc<Mutex<Option<(u64, Sender<Result<Value>>)>>>;

pub struct Mpv {
    child: std::process::Child,
    writer: Stream,
    pending: Pending,
    events: Option<Receiver<PlayerEvent>>,
}

impl Mpv {
    pub fn spawn() -> Result<Self> {
        let (sock_arg, name) = ipc_server_arg();
        let child = std::process::Command::new("mpv")
            .arg("--idle=yes")
            .arg("--no-terminal")
            .arg("--fullscreen")
            .arg("--alang=jpn,ja")
            .arg("--slang=jpn,ja")
            .arg("--no-osc")
            .arg(format!("--input-ipc-server={sock_arg}"))
            .spawn()
            .context("cannot start mpv — is it installed and on PATH?")?;

        let stream = connect_with_retry(&name)?;
        let writer = stream.try_clone().context("cannot clone mpv IPC stream")?;
        let pending: Pending = Arc::default();
        let (event_tx, event_rx) = std::sync::mpsc::channel::<PlayerEvent>();
        let reader_pending = Arc::clone(&pending);
        std::thread::spawn(move || reader_loop(stream, reader_pending, event_tx));

        Ok(Mpv { child, writer, pending, events: Some(event_rx) })
    }

    /// Hand out the event receiver once; std mpsc receivers are not cloneable.
    pub fn take_events(&mut self) -> Option<Receiver<PlayerEvent>> {
        self.events.take()
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn rpc(&mut self, command: Value) -> Result<Value> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (reply_tx, reply_rx) = std::sync::mpsc::channel::<Result<Value>>();
        *self.pending.lock().unwrap() = Some((id, reply_tx));
        let line = build_request(command, id);
        if self.writer.write_all(line.as_bytes()).is_err() {
            *self.pending.lock().unwrap() = None;
            return Err(anyhow!("mpv IPC write failed"));
        }
        match reply_rx.recv_timeout(RPC_TIMEOUT) {
            Ok(result) => result,
            Err(_) => Err(anyhow!("mpv IPC timeout")),
        }
    }
}

impl Player for Mpv {
    fn load_file(&mut self, path: &Path, start: Option<f64>) -> Result<()> {
        let command: Vec<Value> = build_loadfile(path, start).into_iter().map(Value::String).collect();
        self.rpc(Value::Array(command))?;
        Ok(())
    }

    fn position(&mut self) -> Result<f64> {
        self.rpc(json!(["get_property", "time-pos"]))?
            .as_f64()
            .ok_or_else(|| anyhow!("no file playing"))
    }

    fn set_paused(&mut self, paused: bool) -> Result<()> {
        self.rpc(json!(["set_property", "pause", paused]))?;
        Ok(())
    }

    fn set_volume(&mut self, volume: f64) -> Result<()> {
        self.rpc(json!(["set_property", "volume", volume]))?;
        Ok(())
    }

    fn set_mute(&mut self, mute: bool) -> Result<()> {
        self.rpc(json!(["set_property", "mute", mute]))?;
        Ok(())
    }
}

fn connect_with_retry(name: &Name) -> Result<Stream> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match ConnectOptions::new().name(name.borrow()).connect_sync() {
            Ok(stream) => return Ok(stream),
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(anyhow!("cannot connect to mpv IPC socket: {err}")),
        }
    }
}

fn reader_loop(stream: Stream, pending: Pending, event_tx: Sender<PlayerEvent>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return, // mpv closed the socket
            Ok(_) => {}
            Err(_) => return,
        }
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        if let Some((id, result)) = parse_response(line) {
            let mut slot = pending.lock().unwrap();
            if let Some((current_id, reply)) = slot.take() {
                if current_id == id {
                    let _ = reply.send(result);
                } else {
                    *slot = Some((current_id, reply)); // late reply to a timed-out request
                }
            }
        } else if let Some(event) = parse_event(line) {
            let _ = event_tx.send(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn build_loadfile_with_start() {
        let cmd = build_loadfile(Path::new("e1.mkv"), Some(12.5));
        assert_eq!(
            cmd,
            vec![
                "loadfile".to_string(),
                "e1.mkv".to_string(),
                "replace".to_string(),
                "start=+12.5".to_string(),
            ]
        );
    }

    #[test]
    fn build_loadfile_without_start() {
        let cmd = build_loadfile(Path::new("e1.mkv"), None);
        assert_eq!(cmd, vec!["loadfile", "e1.mkv", "replace"]);
    }

    #[test]
    fn parse_event_eof_is_file_ended() {
        assert!(matches!(
            parse_event(r#"{"event":"end-file","reason":"eof"}"#),
            Some(PlayerEvent::FileEnded)
        ));
    }

    #[test]
    fn parse_event_stop_is_ignored() {
        assert!(parse_event(r#"{"event":"end-file","reason":"stop"}"#).is_none());
        assert!(parse_event(r#"{"event":"end-file","reason":"redirect"}"#).is_none());
    }

    #[test]
    fn parse_event_unknown_is_none() {
        assert!(parse_event(r#"{"event":"idle"}"#).is_none());
        assert!(parse_event("garbage not json").is_none());
    }

    #[test]
    fn parse_response_ok() {
        let (id, result) = parse_response(r#"{"error":"success","request_id":3}"#).unwrap();
        assert_eq!(id, 3);
        assert!(result.is_ok());
    }

    #[test]
    fn parse_response_err() {
        let (id, result) = parse_response(r#"{"error":"invalid parameter","request_id":3}"#).unwrap();
        assert_eq!(id, 3);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid parameter"));
    }

    #[test]
    #[ignore = "needs mpv installed"]
    fn mpv_spawn_and_rpc_roundtrip() {
        let mut mpv = Mpv::spawn().unwrap();
        let events = mpv.take_events().unwrap();

        let version = mpv
            .rpc(serde_json::json!(["get_property", "mpv-version"]))
            .unwrap();
        assert!(!version.as_str().unwrap_or("").is_empty());

        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(matches!(
            events.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));

        mpv.kill();
    }
}
