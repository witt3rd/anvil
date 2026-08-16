use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::ask::AskSink;
use crate::StrikeReply;

use super::proto::{Msg, Req};
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
                Msg::Pong { .. } | Msg::Reply { .. } | Msg::Bye { .. } | Msg::Inspect { .. } => {}
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
