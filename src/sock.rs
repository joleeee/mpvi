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

use super::Event;

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
        oneshot::Sender<Result<serde_json::Value, MpvError>>,
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
    async fn recv_response(reader: &mut BufReader<OwnedReadHalf>) -> MpvResponse {
        let mut buf = String::new();
        reader.read_line(&mut buf).await.unwrap();
        serde_json::from_str(&buf).unwrap()
    }

    async fn send_message(&mut self, cmd: Command) {
        let txt = serde_json::to_string(&cmd).unwrap();
        self.writer.write_all(txt.as_bytes()).await.unwrap();
        self.writer.write_u8(b'\n').await.unwrap();
    }

    async fn get_awck(&mut self) -> Result<serde_json::Value, MpvError> {
        let awck = loop {
            match Self::recv_response(&mut self.reader).await {
                MpvResponse::Awck(awck) => break awck,
                MpvResponse::Event(e) => self.propagate_event(e).await,
            }
        };

        if awck.error == "success" {
            Ok(awck.data)
        } else {
            Err(MpvError(awck.error))
        }
    }

    async fn propagate_event(&mut self, event: Event) {
        for tx in &self.event_senders {
            tx.send(event.clone()).await.unwrap();
        }
    }

    pub async fn run_actor(mut self) {
        loop {
            select! {
                Some(msg) = self.handle_receiver.recv() => {
                    match msg {
                        MpvMsg::Command(cmd, oneshot) => {
                            self.send_message(cmd).await;
                            let awck = self.get_awck().await;
                            oneshot.send(awck).unwrap();
                        },
                        MpvMsg::NewSub(tx) => self.event_senders.push(tx),
                    }
                },
                resp = Self::recv_response(&mut self.reader) => {
                    match resp {
                        MpvResponse::Event(e) => self.propagate_event(e).await,
                        MpvResponse::Awck(_) => panic!("should never receieve an unsolicited awck"),
                    }
                }
            }
        }
    }
}
