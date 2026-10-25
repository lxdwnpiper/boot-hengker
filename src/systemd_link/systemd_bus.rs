use std::{
    future::ready,
    io, iter, mem,
    os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd},
    time::Duration,
};

use dbus::{
    Message, MessageType, Path,
    arg::{AppendAll, PropMap, RefArg, Variant},
    channel::{BusType, Channel},
};
use futures::{StreamExt, stream};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use tokio::{io::unix::AsyncFd, time::timeout};

use crate::{
    boot_model::{Activity, JobKind, JobResult, UnitFacts},
    console_session::stream_sources::{CarriedItem, readable_source, unfold_carrying},
    systemd_link::link_error::Error,
};

const DESTINATION: &str = "org.freedesktop.systemd1";
const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";
const UNIT_INTERFACE: &str = "org.freedesktop.systemd1.Unit";
const JOB_INTERFACE: &str = "org.freedesktop.systemd1.Job";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const PEER_INTERFACE: &str = "org.freedesktop.DBus.Peer";
const BUS_DESTINATION: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const PRIVATE_SOCKET: &str = "unix:path=/run/systemd/private";
const SIGNAL_RULE: &str = "type='signal',path_namespace='/org/freedesktop/systemd1'";
const CALL_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    System,
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusOutput {
    Silent,
    Configured,
}

impl StatusOutput {
    fn bus_mode(self) -> &'static str {
        match self {
            Self::Silent => "no",
            Self::Configured => "",
        }
    }
}

pub enum Signal {
    JobNew {
        id: u32,
        job: Path<'static>,
        unit: String,
    },
    JobRemoved {
        id: u32,
        result: JobResult,
    },
    UnitChanged {
        path: Path<'static>,
        activity: Activity,
    },
    BootFinished,
}

pub struct FetchedUnit {
    pub path: Path<'static>,
    pub facts: UnitFacts,
    pub activity: Activity,
}

pub struct JobListing {
    pub id: u32,
    pub unit: String,
    pub kind: JobKind,
}

type ListedJob = (u32, String, String, String, Path<'static>, Path<'static>);

impl From<ListedJob> for JobListing {
    fn from((id, unit, kind, ..): ListedJob) -> Self {
        Self {
            id,
            unit,
            kind: serde_plain::from_str(&kind).unwrap_or(JobKind::Unknown),
        }
    }
}

pub struct Session {
    channel: Channel,
    pub peer_to_peer: bool,
}

fn method_call(
    destination: &str,
    path: &str,
    interface: &str,
    member: &str,
    arguments: impl AppendAll,
) -> Result<Message, Error> {
    let Ok(mut call) = Message::new_method_call(destination, path, interface, member) else {
        return Err(Error::MalformedCall);
    };
    call.append_all(arguments);
    Ok(call)
}

fn manager_call(member: &str, arguments: impl AppendAll) -> Result<Message, Error> {
    method_call(
        DESTINATION,
        MANAGER_PATH,
        MANAGER_INTERFACE,
        member,
        arguments,
    )
}

fn property_call(path: &str, interface: &str, property: &str) -> Result<Message, Error> {
    method_call(
        DESTINATION,
        path,
        PROPERTIES_INTERFACE,
        "Get",
        (interface, property),
    )
}

fn boot_in_progress_call() -> Result<Message, Error> {
    property_call(MANAGER_PATH, MANAGER_INTERFACE, "FinishTimestampMonotonic")
}

fn read_boot_in_progress(reply: &Message) -> Result<bool, Error> {
    let finished: Variant<u64> = reply.read1()?;
    Ok(finished.0 == 0)
}

impl Session {
    pub fn open(scope: Scope) -> Result<Self, Error> {
        if scope == Scope::System
            && let Ok(channel) = Channel::open_private(PRIVATE_SOCKET)
        {
            send_authentication_carrier(&channel)?;
            return Ok(Self::watching(channel, true));
        }
        let bus = match scope {
            Scope::System => BusType::System,
            Scope::User => BusType::Session,
        };
        let session = Self::watching(Channel::get_private(bus)?, false);
        session.blocking_call(method_call(
            BUS_DESTINATION,
            BUS_PATH,
            BUS_DESTINATION,
            "AddMatch",
            (SIGNAL_RULE,),
        )?)?;
        Ok(session)
    }

    fn watching(mut channel: Channel, peer_to_peer: bool) -> Self {
        channel.set_watch_enabled(true);
        Self {
            channel,
            peer_to_peer,
        }
    }

    fn blocking_call(&self, call: Message) -> Result<Message, Error> {
        Ok(self.channel.send_with_reply_and_block(call, CALL_TIMEOUT)?)
    }

    pub fn set_show_status(&self, output: StatusOutput) -> Result<(), Error> {
        self.blocking_call(manager_call("SetShowStatus", (output.bus_mode(),))?)?;
        Ok(())
    }

    pub fn boot_in_progress(&self) -> Result<bool, Error> {
        read_boot_in_progress(&self.blocking_call(boot_in_progress_call()?)?)
    }

    fn send(&self, call: Message) -> Result<u32, Error> {
        let serial = self.channel.send(call).map_err(|()| Error::Disconnected)?;
        self.channel.flush();
        Ok(serial)
    }

    fn socket_has_unread_bytes(&self) -> bool {
        let mut watched = [PollFd::new(self, PollFlags::IN)];
        poll(&mut watched, Some(&Timespec::default())).is_ok_and(|ready| ready > 0)
    }

    fn read_one_chunk(&self) -> Result<(), Error> {
        let still_connected =
            self.channel.read_write(Some(Duration::ZERO)).is_ok() && self.channel.is_connected();
        still_connected.then_some(()).ok_or(Error::Disconnected)
    }

    fn arrived_messages(&self) -> Result<Vec<Message>, Error> {
        iter::repeat_with(|| self.read_one_chunk())
            .find(|outcome| outcome.is_err() || !self.socket_has_unread_bytes())
            .unwrap_or(Ok(()))?;
        Ok(iter::from_fn(|| self.channel.pop_message()).collect())
    }
}

pub struct BusConnection {
    session: AsyncFd<Session>,
    early_signals: Vec<Signal>,
}

fn into_results(batch: Result<Vec<Message>, Error>) -> Vec<Result<Message, Error>> {
    match batch {
        Ok(messages) => messages.into_iter().map(Ok).collect(),
        Err(error) => vec![Err(error)],
    }
}

impl BusConnection {
    pub fn register(session: Session) -> io::Result<Self> {
        Ok(Self {
            session: readable_source(session)?,
            early_signals: Vec::new(),
        })
    }

    #[must_use]
    pub fn peer_to_peer(&self) -> bool {
        self.session.get_ref().peer_to_peer
    }

    fn drain_replies(&mut self) -> Result<Vec<Message>, Error> {
        let (signals, replies): (Vec<Message>, Vec<Message>) = self
            .session
            .get_ref()
            .arrived_messages()?
            .into_iter()
            .partition(|message| message.msg_type() == MessageType::Signal);
        self.early_signals
            .extend(signals.iter().filter_map(parse_signal));
        Ok(replies)
    }

    async fn next_replies(&mut self) -> Result<Vec<Message>, Error> {
        let drained = self.drain_replies()?;
        if !drained.is_empty() {
            return Ok(drained);
        }
        self.session.readable_mut().await?.clear_ready();
        self.drain_replies()
    }

    async fn reply_to(&mut self, serial: u32) -> Result<Message, Error> {
        unfold_carrying(self, |connection| async move {
            let replies = connection.next_replies().await;
            Some(CarriedItem {
                item: replies,
                state: connection,
            })
        })
        .flat_map(|replies| stream::iter(into_results(replies)))
        .filter(|reply| {
            ready(
                reply
                    .as_ref()
                    .map_or(true, |message| message.get_reply_serial() == Some(serial)),
            )
        })
        .take(1)
        .fold(Err(Error::Disconnected), |_, reply| ready(reply))
        .await
    }

    async fn call(&mut self, call: Message) -> Result<Message, Error> {
        let serial = self.session.get_ref().send(call)?;
        let Ok(reply) = timeout(CALL_TIMEOUT, self.reply_to(serial)).await else {
            return Err(Error::CallTimedOut);
        };
        let mut reply = reply?;
        reply.as_result()?;
        Ok(reply)
    }

    pub async fn subscribe(&mut self) -> Result<(), Error> {
        self.call(manager_call("Subscribe", ())?).await?;
        Ok(())
    }

    pub async fn set_show_status(&mut self, output: StatusOutput) -> Result<(), Error> {
        self.call(manager_call("SetShowStatus", (output.bus_mode(),))?)
            .await?;
        Ok(())
    }

    pub async fn default_target(&mut self) -> Result<String, Error> {
        let reply = self.call(manager_call("GetDefaultTarget", ())?).await?;
        Ok(reply.read1()?)
    }

    pub async fn boot_in_progress(&mut self) -> Result<bool, Error> {
        read_boot_in_progress(&self.call(boot_in_progress_call()?).await?)
    }

    pub async fn list_jobs(&mut self) -> Result<Vec<JobListing>, Error> {
        let reply = self.call(manager_call("ListJobs", ())?).await?;
        let listed: Vec<ListedJob> = reply.read1()?;
        Ok(listed.into_iter().map(JobListing::from).collect())
    }

    pub async fn job_kind(&mut self, job: &Path<'_>) -> Result<JobKind, Error> {
        let reply = self
            .call(property_call(job, JOB_INTERFACE, "JobType")?)
            .await?;
        let kind: Variant<String> = reply.read1()?;
        Ok(serde_plain::from_str(&kind.0).unwrap_or(JobKind::Unknown))
    }

    pub async fn fetch_unit(&mut self, name: &str) -> Result<FetchedUnit, Error> {
        let path: Path<'static> = self
            .call(manager_call("GetUnit", (name,))?)
            .await?
            .read1()?;
        let properties: PropMap = self
            .call(method_call(
                DESTINATION,
                &path,
                PROPERTIES_INTERFACE,
                "GetAll",
                (UNIT_INTERFACE,),
            )?)
            .await?
            .read1()?;
        Ok(FetchedUnit {
            facts: unit_facts_from(name, &properties),
            activity: activity_from(&properties),
            path,
        })
    }

    pub async fn receive_signals(&mut self, patience: Duration) -> Result<Vec<Signal>, Error> {
        if self.early_signals.is_empty()
            && let Ok(Ok(mut guard)) = timeout(patience, self.session.readable_mut()).await
        {
            guard.clear_ready();
        }
        self.drain_replies()?;
        Ok(mem::take(&mut self.early_signals))
    }
}

fn unit_facts_from(name: &str, properties: &PropMap) -> UnitFacts {
    let text = |property: &str| {
        properties
            .get(property)
            .and_then(|value| value.0.as_str())
            .map(ToOwned::to_owned)
    };
    let names = |property: &str| {
        properties
            .get(property)
            .and_then(|value| value.0.as_iter())
            .map(|items| {
                items
                    .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    UnitFacts {
        name: text("Id").unwrap_or_else(|| name.to_owned()),
        description: text("Description").filter(|description| !description.is_empty()),
        after: names("After"),
        before: names("Before"),
    }
}

impl AsRawFd for Session {
    fn as_raw_fd(&self) -> RawFd {
        self.channel.watch().fd
    }
}

impl AsFd for Session {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}

fn send_authentication_carrier(channel: &Channel) -> Result<(), Error> {
    let Ok(mut ping) = Message::new_method_call(DESTINATION, MANAGER_PATH, PEER_INTERFACE, "Ping")
    else {
        return Err(Error::MalformedCall);
    };
    ping.set_no_reply(true);
    if channel.send(ping).is_err() {
        return Err(Error::Disconnected);
    }
    channel.flush();
    Ok(())
}

fn parse_signal(message: &Message) -> Option<Signal> {
    if message.msg_type() != MessageType::Signal {
        return None;
    }
    let interface = message.interface()?;
    let member = message.member()?;
    match (&*interface, &*member) {
        (MANAGER_INTERFACE, "JobNew" | "JobRemoved") => parse_job_signal(message, &member),
        (MANAGER_INTERFACE, "StartupFinished") => Some(Signal::BootFinished),
        (PROPERTIES_INTERFACE, "PropertiesChanged") => parse_unit_change(message),
        _ => None,
    }
}

fn parse_job_signal(message: &Message, member: &str) -> Option<Signal> {
    match member {
        "JobNew" => {
            let (id, job, unit): (u32, Path<'static>, String) = message.read3().ok()?;
            Some(Signal::JobNew { id, job, unit })
        }
        "JobRemoved" => {
            let (id, _job, _unit, result): (u32, Path<'static>, String, String) =
                message.read4().ok()?;
            Some(Signal::JobRemoved {
                id,
                result: serde_plain::from_str(&result).unwrap_or(JobResult::Failed),
            })
        }
        _ => None,
    }
}

fn parse_unit_change(message: &Message) -> Option<Signal> {
    let (changed_interface, changed, _invalidated): (String, PropMap, Vec<String>) =
        message.read3().ok()?;
    if changed_interface != UNIT_INTERFACE {
        return None;
    }
    Some(Signal::UnitChanged {
        path: message.path()?.into_static(),
        activity: activity_from(&changed),
    })
}

fn activity_from(properties: &PropMap) -> Activity {
    let monotonic = |property: &str| {
        properties
            .get(property)
            .and_then(|value| value.0.as_u64())
            .filter(|micros| *micros != 0)
            .map(Duration::from_micros)
    };
    Activity {
        state: properties
            .get("ActiveState")
            .and_then(|value| value.0.as_str())
            .and_then(|raw| serde_plain::from_str(raw).ok()),
        inactive_exit: monotonic("InactiveExitTimestampMonotonic"),
        active_enter: monotonic("ActiveEnterTimestampMonotonic"),
        active_exit: monotonic("ActiveExitTimestampMonotonic"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_finished_signal_marks_the_boot_finished() {
        let startup_finished = Message::signal(
            &Path::from(MANAGER_PATH),
            &MANAGER_INTERFACE.into(),
            &"StartupFinished".into(),
        )
        .append3(0_u64, 0_u64, 0_u64)
        .append3(0_u64, 0_u64, 0_u64);
        assert!(matches!(
            parse_signal(&startup_finished),
            Some(Signal::BootFinished)
        ));
    }
}
