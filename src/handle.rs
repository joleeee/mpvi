use tokio::sync::{mpsc, oneshot};

use super::{Command, Event, MpvMsg, MpvSocket, Property};

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

    async fn send_command(
        &self,
        command: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(MpvMsg::Command(Command { command }, sender))
            .await
            .unwrap();

        receiver.await.unwrap()
    }

    pub async fn set_property(
        &self,
        property: Property,
        value: serde_json::Value,
    ) -> Result<(), String> {
        let property = serde_json::to_string(&property).unwrap();
        let property = property.trim_matches('"'); // eww
        let res = self
            .send_command(vec!["set_property".into(), property.into(), value])
            .await;

        res.map(|val| val.as_null().expect("should not be set"))
    }

    pub async fn get_property(&self, property: &str) -> Result<serde_json::Value, String> {
        self.send_command(vec!["get_property".into(), property.into()])
            .await
    }

    pub async fn pause(&self) -> Result<(), String> {
        self.set_property(Property::Pause, true.into()).await
    }

    pub async fn unpause(&self) -> Result<(), String> {
        self.set_property(Property::Pause, false.into()).await
    }

    pub async fn get_pause(&self) -> Result<bool, String> {
        let res = self.get_property("pause").await;
        res.map(|val| val.as_bool().unwrap())
    }
}
