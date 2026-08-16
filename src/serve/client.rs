use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::ask::AskSink;
use crate::StrikeReply;

use super::proto::{Msg, Req};
use super::pty::PtyScreen;
use super::Report;

#[derive(Debug)]
pub struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl Client {
    pub fn connect(sock: impl AsRef<Path>) -> io::Result<Self> {
        let stream = UnixStream::connect(sock.as_ref())?;
        stream.set_read_timeout(Some(Duration::from_secs(600)))?;
        let writer = stream.try_clone()?;
        Ok(Self {
            writer,
            reader: BufReader::new(stream),
            next_id: 1,
        })
    }

    pub fn set_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.writer.set_read_timeout(Some(timeout))?;
        self.writer.set_write_timeout(Some(timeout))?;
        let reader = self.reader.get_mut();
        reader.set_read_timeout(Some(timeout))?;
        reader.set_write_timeout(Some(timeout))
    }

    pub fn ping(&mut self) -> io::Result<()> {
        let id = self.next();
        self.send(&Req::Ping { id: id.clone() })?;
        match self.recv_for(&id)? {
            Msg::Pong { .. } => Ok(()),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn strike(&mut self, session: &str, code: &str) -> io::Result<StrikeReply> {
        let id = self.next();
        self.send(&Req::Strike {
            id: id.clone(),
            session: session.into(),
            code: code.into(),
        })?;
        match self.recv_for(&id)? {
            Msg::Reply { reply, .. } => Ok(reply),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn reset(&mut self, session: &str) -> io::Result<StrikeReply> {
        let id = self.next();
        self.send(&Req::Reset {
            id: id.clone(),
            session: session.into(),
        })?;
        match self.recv_for(&id)? {
            Msg::Reply { reply, .. } => Ok(reply),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn ask(
        &mut self,
        session: &str,
        prompt: &str,
        provider: Option<&str>,
        model: Option<&str>,
        sink: &mut impl AskSink,
    ) -> io::Result<String> {
        let id = self.next();
        self.send(&Req::Ask {
            id: id.clone(),
            session: session.into(),
            prompt: prompt.into(),
            provider: provider.map(str::to_string),
            model: model.map(str::to_string),
        })?;
        loop {
            match self.recv_for(&id)? {
                Msg::Status { text, .. } => sink.on_status(&text),
                Msg::Draft { text, .. } => sink.on_draft(&text),
                Msg::Strike {
                    code,
                    stdout,
                    stderr,
                    error,
                    ok,
                    ..
                } => {
                    let reply = StrikeReply {
                        id: id.clone(),
                        ok,
                        value: serde_json::Value::Null,
                        stdout,
                        stderr,
                        error,
                    };
                    sink.on_strike(&code, &reply);
                }
                Msg::Answer { text, .. } => return Ok(text),
                Msg::Error { text, .. } => return Err(io::Error::other(text)),
                Msg::Pong { .. }
                | Msg::Reply { .. }
                | Msg::Bye { .. }
                | Msg::Inspect { .. }
                | Msg::Mounted { .. }
                | Msg::Unmounted { .. }
                | Msg::PtyScreen { .. }
                | Msg::EditBuf { .. } => {}
            }
        }
    }

    pub fn expose(&mut self, session: &str) -> io::Result<()> {
        let id = self.next();
        self.send(&Req::Expose {
            id: id.clone(),
            session: session.into(),
        })?;
        match self.recv_for(&id)? {
            Msg::Pong { .. } => Ok(()),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn mount(&mut self, kind: &str, slot: Option<&str>) -> io::Result<(String, String)> {
        let id = self.next();
        self.send(&Req::Mount {
            id: id.clone(),
            kind: kind.into(),
            slot: slot.map(str::to_string),
        })?;
        match self.recv_for(&id)? {
            Msg::Mounted { mount_id, slot, .. } => Ok((mount_id, slot)),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn unmount(&mut self, mount_id: &str) -> io::Result<()> {
        let id = self.next();
        self.send(&Req::Unmount {
            id: id.clone(),
            mount_id: mount_id.into(),
        })?;
        match self.recv_for(&id)? {
            Msg::Unmounted { .. } => Ok(()),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn pty_open(&mut self, name: &str, cols: u16, rows: u16) -> io::Result<PtyScreen> {
        let id = self.next();
        self.send(&Req::PtyOpen {
            id: id.clone(),
            name: name.into(),
            cols,
            rows,
        })?;
        self.recv_pty(&id)
    }

    pub fn pty_write(&mut self, name: &str, data: &str) -> io::Result<PtyScreen> {
        let id = self.next();
        self.send(&Req::PtyWrite {
            id: id.clone(),
            name: name.into(),
            data: data.into(),
        })?;
        self.recv_pty(&id)
    }

    pub fn pty_resize(&mut self, name: &str, cols: u16, rows: u16) -> io::Result<PtyScreen> {
        let id = self.next();
        self.send(&Req::PtyResize {
            id: id.clone(),
            name: name.into(),
            cols,
            rows,
        })?;
        self.recv_pty(&id)
    }

    pub fn pty_snap(&mut self, name: &str) -> io::Result<PtyScreen> {
        let id = self.next();
        self.send(&Req::PtySnap {
            id: id.clone(),
            name: name.into(),
        })?;
        self.recv_pty(&id)
    }

    fn recv_pty(&mut self, id: &str) -> io::Result<PtyScreen> {
        match self.recv_for(id)? {
            Msg::PtyScreen {
                name,
                cols,
                rows,
                cursor_col,
                cursor_row,
                lines,
                runs,
                alive,
                ..
            } => Ok(PtyScreen {
                name,
                cols,
                rows,
                cursor_col,
                cursor_row,
                lines,
                runs,
                alive,
            }),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn edit_snap(&mut self, name: &str) -> io::Result<super::EditBuf> {
        let id = self.next();
        self.send(&Req::EditSnap {
            id: id.clone(),
            name: name.into(),
        })?;
        self.recv_edit(&id)
    }

    pub fn edit(
        &mut self,
        name: &str,
        op: super::EditOp,
        text: &str,
    ) -> io::Result<super::EditBuf> {
        let id = self.next();
        self.send(&Req::Edit {
            id: id.clone(),
            name: name.into(),
            edit_op: op,
            text: text.into(),
        })?;
        self.recv_edit(&id)
    }

    fn recv_edit(&mut self, id: &str) -> io::Result<super::EditBuf> {
        match self.recv_for(id)? {
            Msg::EditBuf {
                name, text, cursor, ..
            } => Ok(super::EditBuf { name, text, cursor }),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn warm(&mut self, workspace: &str) -> io::Result<()> {
        let id = self.next();
        self.send(&Req::Warm {
            id: id.clone(),
            workspace: workspace.into(),
        })?;
        match self.recv_for(&id)? {
            Msg::Pong { .. } => Ok(()),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn inspect(&mut self) -> io::Result<Report> {
        let id = self.next();
        self.send(&Req::Inspect { id: id.clone() })?;
        match self.recv_for(&id)? {
            Msg::Inspect { report, .. } => Ok(report),
            Msg::Error { text, .. } => Err(io::Error::other(text)),
            other => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        let id = self.next();
        self.send(&Req::Shutdown { id: id.clone() })?;
        match self.recv_for(&id) {
            Ok(Msg::Bye { .. }) | Err(_) => Ok(()),
            Ok(Msg::Error { text, .. }) => Err(io::Error::other(text)),
            Ok(other) => Err(io::Error::other(format!("unexpected {other:?}"))),
        }
    }

    fn next(&mut self) -> String {
        let id = self.next_id.to_string();
        self.next_id += 1;
        id
    }

    fn send(&mut self, req: &Req) -> io::Result<()> {
        let line = serde_json::to_string(req).expect("req is serializable");
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }

    fn recv_for(&mut self, id: &str) -> io::Result<Msg> {
        loop {
            let msg = self.recv()?;
            if msg.id() == id {
                return Ok(msg);
            }
        }
    }

    fn recv(&mut self) -> io::Result<Msg> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "serve closed the socket",
            ));
        }
        serde_json::from_str(line.trim()).map_err(|err| io::Error::other(err.to_string()))
    }
}
