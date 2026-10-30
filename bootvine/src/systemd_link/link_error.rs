use std::{convert::Infallible, io};

use dbus::{Error as BusError, arg::TypeMismatchError};
use rustix::io::Errno;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("systemd bus: {0}")]
    Bus(#[from] BusError),
    #[error("unexpected reply from systemd: {0}")]
    Reply(#[from] TypeMismatchError),
    #[error("could not build a method call")]
    MalformedCall,
    #[error("the service manager closed the connection")]
    Disconnected,
    #[error("the service manager did not answer in time")]
    CallTimedOut,
    #[error("console: {0}")]
    Console(#[from] io::Error),
    #[error("entering the new root: {0}")]
    RootSwitch(#[from] Errno),
    #[error("the service manager did not come back")]
    ManagerUnreachable,
}

impl From<Infallible> for Error {
    fn from(never: Infallible) -> Self {
        match never {}
    }
}
