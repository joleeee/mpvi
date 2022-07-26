use tokio::sync::{mpsc, oneshot};

use crate::{sock::MpvSocketError, Command, Event, MpvMsg, MpvSocket, Property};

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

    #[derive(Serialize, Debug, Clone, Copy)]
    #[serde(rename_all = "kebab-case")]
    pub enum Insertion {
        Append,
        Replace,
    }
}

#[derive(Clone)]
pub struct MpvHandle {
    sender: mpsc::Sender<MpvMsg>,
}

#[derive(Debug)]
pub enum HandleError {
    RecvError(mpsc::error::SendError<MpvMsg>),
    MpvSocketError(MpvSocketError),
}

impl From<mpsc::error::SendError<MpvMsg>> for HandleError {
    fn from(e: mpsc::error::SendError<MpvMsg>) -> Self {
        Self::RecvError(e)
    }
}

impl From<MpvSocketError> for HandleError {
    fn from(e: MpvSocketError) -> Self {
        Self::MpvSocketError(e)
    }
}

impl MpvHandle {
    pub async fn new(ipc: &str) -> tokio::io::Result<Self> {
        let (sender, receiver) = mpsc::channel(8);
        let actor = MpvSocket::new(ipc, receiver).await?;
        tokio::spawn(actor.run_actor());

        Ok(Self { sender })
    }

    pub async fn subscribe_events(
        &self,
        sender: mpsc::Sender<Event>,
    ) -> Result<(), mpsc::error::SendError<MpvMsg>> {
        self.sender.send(MpvMsg::NewSub(sender)).await
    }

    async fn send_command(
        &self,
        command: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, HandleError> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(MpvMsg::Command(Command { command }, sender))
            .await?;

        receiver.await.expect("receive failed").map_err(Into::into)
    }

    pub async fn set_property(
        &self,
        property: Property,
        value: serde_json::Value,
    ) -> Result<(), HandleError> {
        let res = self
            .send_command(vec![
                "set_property".into(),
                property.to_string().into(),
                value,
            ])
            .await;

        res.map(|val| val.as_null().expect("should not be set"))
    }

    pub async fn get_property(&self, property: Property) -> Result<serde_json::Value, HandleError> {
        self.send_command(vec!["get_property".into(), property.to_string().into()])
            .await
    }

    pub async fn pause(&self) -> Result<(), HandleError> {
        self.set_property(Property::Pause, true.into()).await
    }

    pub async fn unpause(&self) -> Result<(), HandleError> {
        self.set_property(Property::Pause, false.into()).await
    }

    pub async fn get_pause(&self) -> Result<bool, HandleError> {
        let res = self.get_property(Property::Pause).await;
        res.map(|val| val.as_bool().expect("pause is not a bool"))
    }

    pub async fn seek<S: ToString>(
        &self,
        seconds: S,
        mode: option::Seek,
    ) -> Result<(), HandleError> {
        let mode = serde_json::to_value(&mode).expect("failed to serialize");

        self.send_command(vec!["seek".into(), seconds.to_string().into(), mode])
            .await
            .map(|v| v.as_null().expect("should not be set"))
    }

    pub async fn load_file(
        &self,
        file: &str,
        insertion: option::Insertion,
    ) -> Result<(), HandleError> {
        let insertion = serde_json::to_value(&insertion).expect("failed to serialize");

        self.send_command(vec!["loadfile".into(), file.into(), insertion])
            .await
            // if replace, this returns something, just ignore it
            .map(|_| ())
    }
}
