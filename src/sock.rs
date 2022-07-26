use serde::{Deserialize, Serialize};
use std::io;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        unix::{OwnedReadHalf, OwnedWriteHalf},
        UnixStream,
    },
    select,
    sync::{mpsc, oneshot},
};

use crate::Event;

#[derive(Deserialize, Debug)]
struct Awck {
    #[allow(dead_code)]
    request_id: u64,
    error: String,
    #[serde(default)]
    data: serde_json::Value,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum MpvResponse {
    Awck(Awck),
    Event(Event),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Command {
    pub command: Vec<serde_json::Value>,
}

#[derive(Debug)]
pub struct MpvError(pub String);

#[derive(Debug)]
pub enum MpvMsg {
    Command(
        Command,
        oneshot::Sender<Result<serde_json::Value, MpvSocketError>>,
    ),
    NewSub(mpsc::Sender<Event>),
}

pub struct MpvSocket {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    /// Receives messages from the handles
    handle_receiver: mpsc::Receiver<MpvMsg>,
    /// Vector of senders to send events to
    event_senders: Vec<mpsc::Sender<Event>>,
}

#[derive(Debug)]
pub enum MpvSocketError {
    Send(mpsc::error::SendError<Event>),
    Mpv(MpvError),
    Io(io::Error),
    Oneshot,
}

impl From<mpsc::error::SendError<Event>> for MpvSocketError {
    fn from(e: mpsc::error::SendError<Event>) -> Self {
        Self::Send(e)
    }
}

impl From<MpvError> for MpvSocketError {
    fn from(e: MpvError) -> Self {
        Self::Mpv(e)
    }
}

impl From<io::Error> for MpvSocketError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl MpvSocket {
    pub async fn new(path: &str, receiver: mpsc::Receiver<MpvMsg>) -> io::Result<Self> {
        let stream = UnixStream::connect(path).await?;
        let (read, write) = stream.into_split();
        let read = BufReader::new(read);

        Ok(Self {
            handle_receiver: receiver,
            reader: read,
            writer: write,
            event_senders: Vec::new(),
        })
    }

    // work around not being able to share ownership
    async fn recv_response(
        reader: &mut BufReader<OwnedReadHalf>,
    ) -> Result<MpvResponse, MpvSocketError> {
        let mut buf = String::new();
        reader.read_line(&mut buf).await?;
        Ok(serde_json::from_str(&buf).unwrap())
    }

    async fn send_message(&mut self, cmd: Command) -> Result<(), MpvSocketError> {
        let txt = serde_json::to_string(&cmd).expect("failed to serialize");
        self.writer.write_all(txt.as_bytes()).await?;
        self.writer.write_u8(b'\n').await?;
        Ok(())
    }

    async fn get_awck(&mut self) -> Result<serde_json::Value, MpvSocketError> {
        let awck = loop {
            match Self::recv_response(&mut self.reader).await? {
                MpvResponse::Awck(awck) => break awck,
                MpvResponse::Event(e) => self.propagate_event(e).await?,
            }
        };

        if awck.error == "success" {
            Ok(awck.data)
        } else {
            Err(MpvError(awck.error).into())
        }
    }

    async fn propagate_event(&mut self, event: Event) -> Result<(), MpvSocketError> {
        let mut retain = Vec::new();

        // we drop senders that error on the basis that if they are closed immediately after then
        // they were probably closed when the error occured, and that is the case of the error
        for tx in &self.event_senders {
            let keep = match tx.send(event.clone()).await {
                Err(e) => {
                    if tx.is_closed() {
                        false
                    } else {
                        return Err(e.into());
                    }
                }
                Ok(_) => true,
            };

            retain.push(keep);
        }

        let mut retain = retain.iter();

        self.event_senders
            .retain(|_| *retain.next().expect("fewer bools than elements"));

        Ok(())
    }

    pub async fn run_actor(mut self) -> Result<(), MpvSocketError> {
        loop {
            select! {
                Some(msg) = self.handle_receiver.recv() => {
                    match msg {
                        MpvMsg::Command(cmd, oneshot) => {
                            self.send_message(cmd).await?;
                            let awck = self.get_awck().await;
                            // hm
                            oneshot.send(awck).map_err(|_| MpvSocketError::Oneshot)?;
                        },
                        MpvMsg::NewSub(tx) => self.event_senders.push(tx),
                    }
                },
                resp = Self::recv_response(&mut self.reader) => {
                    match resp? {
                        MpvResponse::Event(e) => self.propagate_event(e).await?,
                        MpvResponse::Awck(_) => panic!("should never receieve an unsolicited awck"),
                    }
                }
            }
        }
    }
}
