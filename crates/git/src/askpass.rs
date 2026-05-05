use std::{ffi::OsStr, path::PathBuf};

use anyhow::Result;
use futures::{SinkExt, channel::mpsc, channel::oneshot};
use gpui::{AsyncApp, BackgroundExecutor, Task};

#[derive(Clone)]
pub struct EncryptedPassword(String);

impl TryFrom<&str> for EncryptedPassword {
    type Error = anyhow::Error;

    fn try_from(password: &str) -> Result<Self> {
        Ok(Self(password.to_string()))
    }
}

pub struct IKnowWhatIAmDoingAndIHaveReadTheDocs;

impl EncryptedPassword {
    pub fn decrypt(self, _: IKnowWhatIAmDoingAndIHaveReadTheDocs) -> Result<String> {
        Ok(self.0)
    }
}

#[derive(PartialEq, Eq)]
pub enum AskPassResult {
    CancelledByUser,
    Timedout,
}

pub struct AskPassDelegate {
    tx: mpsc::UnboundedSender<(String, oneshot::Sender<EncryptedPassword>)>,
    executor: BackgroundExecutor,
    _task: Task<()>,
}

impl AskPassDelegate {
    pub fn new(
        cx: &mut AsyncApp,
        password_prompt: impl Fn(String, oneshot::Sender<EncryptedPassword>, &mut AsyncApp)
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded::<(String, oneshot::Sender<_>)>();
        let task = cx.spawn(async move |cx: &mut AsyncApp| {
            use futures::StreamExt as _;
            while let Some((prompt, channel)) = rx.next().await {
                password_prompt(prompt, channel, cx);
            }
        });
        Self {
            tx,
            executor: cx.background_executor().clone(),
            _task: task,
        }
    }

    pub fn ask_password(&mut self, prompt: String) -> Task<Option<EncryptedPassword>> {
        let mut this_tx = self.tx.clone();
        self.executor.spawn(async move {
            let (tx, rx) = oneshot::channel();
            this_tx.send((prompt, tx)).await.ok()?;
            rx.await.ok()
        })
    }
}

pub struct AskPassSession {
    script_path: PathBuf,
}

impl AskPassSession {
    pub async fn new(_: BackgroundExecutor, _: AskPassDelegate) -> Result<Self> {
        Ok(Self {
            script_path: unavailable_askpass_program(),
        })
    }

    pub async fn run(&mut self) -> AskPassResult {
        AskPassResult::CancelledByUser
    }

    #[cfg(target_os = "windows")]
    pub fn get_password(&self) -> Option<EncryptedPassword> {
        None
    }

    pub fn script_path(&self) -> impl AsRef<OsStr> {
        self.script_path.as_os_str()
    }
}

#[cfg(not(target_os = "windows"))]
fn unavailable_askpass_program() -> PathBuf {
    PathBuf::from("/usr/bin/false")
}

#[cfg(target_os = "windows")]
fn unavailable_askpass_program() -> PathBuf {
    PathBuf::from("cmd.exe")
}
