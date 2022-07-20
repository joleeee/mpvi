use serde::{Deserialize, Serialize};
use std::io;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        unix::{OwnedReadHalf, OwnedWriteHalf},
        UnixStream,
    },
    select,
    sync::mpsc,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Command {
    command: Vec<serde_json::Value>,
}

#[derive(Debug)]
enum MpvMsg {
    Command(Command),
    NewSub(mpsc::Sender<Event>),
}

struct MpvSocket {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    /// Receives messages from the handles
    handle_receiver: mpsc::Receiver<MpvMsg>,
    /// Vector of senders to send events to
    event_senders: Vec<mpsc::Sender<Event>>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum MpvResponse {
    Awck(Awck),
    Event(Event),
}

#[derive(Deserialize, Debug)]
struct Awck {
    #[allow(dead_code)]
    request_id: u64,
    error: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "event", rename_all = "kebab-case")]
// TODO: add the fields for the events that have them
// https://mpv.io/manual/master/#list-of-events
pub enum Event {
    StartFile,
    EndFile,
    FileLoaded,
    Seek,
    PlaybackRestart,
    Shutdown,
    LogMessage,
    Hook,
    GetPropertyReply,
    SetPropertyReply,
    CommandReply,
    ClientMessage,
    VideoReconfig,
    AudioReconfig,
    PropertyChange,

    // Why are these undocumented?
    Pause,
    Unpause,
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

    async fn handle_message(&mut self, cmd: Command) -> Result<(), String> {
        let txt = serde_json::to_string(&cmd).unwrap();

        self.writer.write_all(txt.as_bytes()).await.unwrap();
        self.writer.write_u8(b'\n').await.unwrap();

        let awck = loop {
            match Self::recv_response(&mut self.reader).await {
                MpvResponse::Awck(awck) => break awck,
                MpvResponse::Event(e) => self.propagate_event(e).await,
            }
        };

        if awck.error == "success" {
            Ok(())
        } else {
            Err(awck.error)
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
                        MpvMsg::Command(cmd) => self.handle_message(cmd).await.unwrap(),
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

#[derive(Clone)]
pub struct MpvHandle {
    sender: mpsc::Sender<MpvMsg>,
}

impl MpvHandle {
    pub async fn new(ipc: &str) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let actor = MpvSocket::new(ipc, receiver).await.unwrap();
        tokio::spawn(actor.run_actor());

        Self { sender }
    }

    pub async fn subscribe_events(&self, sender: mpsc::Sender<Event>) {
        self.sender.send(MpvMsg::NewSub(sender)).await.unwrap();
    }

    pub async fn set_property(&self, property: &str, value: serde_json::Value) {
        self.sender
            .send(MpvMsg::Command(Command {
                command: vec!["set_property".into(), property.into(), value],
            }))
            .await
            .unwrap();
    }

    pub async fn pause(&self) {
        self.set_property("pause", true.into()).await
    }

    pub async fn unpause(&self) {
        self.set_property("pause", false.into()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn mpv_test() {
        let handle = MpvHandle::new("/tmp/mpv.sock").await;

        let (tx, rx) = mpsc::channel(8);
        handle.subscribe_events(tx).await;
        tokio::spawn(print_events(rx));

        println!("Pausing...");
        handle.pause().await;
        sleep(Duration::from_millis(1000)).await;
        println!("Unpausing...");
        handle.unpause().await;
        sleep(Duration::from_millis(1000)).await;
        println!("Pausing...");
        handle.pause().await;

        // have to wait for the message to be sent (until we add in waiting for awck or someting)
        sleep(Duration::from_millis(100)).await;
    }

    async fn print_events(mut rx: mpsc::Receiver<Event>) {
        while let Some(event) = rx.recv().await {
            println!("event {:?}", event);
        }
    }
}
