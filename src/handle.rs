use tokio::sync::{mpsc, oneshot};

use super::{Command, Event, MpvMsg, MpvSocket, Property};

pub mod option {
    use serde::Serialize;

    #[derive(Serialize, Debug, Clone, Copy)]
    #[serde(rename_all = "kebab-case")]
    pub enum Seek {
        Absolute,
        Relative,
        AbsolutePercent,
        RelativePercent,
    }
}

#[derive(Clone)]
pub struct MpvHandle {
    sender: mpsc::Sender<MpvMsg>,
}

impl MpvHandle {
    pub async fn new(ipc: &str) -> tokio::io::Result<Self> {
        let (sender, receiver) = mpsc::channel(8);
        let actor = MpvSocket::new(ipc, receiver).await?;
        tokio::spawn(actor.run_actor());

        Ok(Self { sender })
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
        let res = self
            .send_command(vec![
                "set_property".into(),
                property.to_string().into(),
                value,
            ])
            .await;

        res.map(|val| val.as_null().expect("should not be set"))
    }

    pub async fn get_property(&self, property: Property) -> Result<serde_json::Value, String> {
        self.send_command(vec!["get_property".into(), property.to_string().into()])
            .await
    }

    pub async fn pause(&self) -> Result<(), String> {
        self.set_property(Property::Pause, true.into()).await
    }

    pub async fn unpause(&self) -> Result<(), String> {
        self.set_property(Property::Pause, false.into()).await
    }

    pub async fn get_pause(&self) -> Result<bool, String> {
        let res = self.get_property(Property::Pause).await;
        res.map(|val| val.as_bool().unwrap())
    }

    pub async fn seek<S: ToString>(&self, seconds: S, mode: option::Seek) -> Result<(), String> {
        let mode = serde_json::to_value(&mode).unwrap();

        self.send_command(vec!["seek".into(), seconds.to_string().into(), mode])
            .await
            .map(|v| v.as_null().expect("should not be set"))
    }
}
