use std::{error, fmt, io};

use dbus::{Error as BusError, arg::TypeMismatchError};
use rustix::io::Errno;

#[derive(Debug)]
pub enum Error {
    Bus(BusError),
    Reply(TypeMismatchError),
    MalformedCall,
    Disconnected,
    Console(io::Error),
    RootSwitch(Errno),
    ManagerUnreachable,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bus(error) => write!(formatter, "systemd bus: {error}"),
            Self::Reply(error) => write!(formatter, "unexpected reply from systemd: {error}"),
            Self::MalformedCall => write!(formatter, "could not build a method call"),
            Self::Disconnected => write!(formatter, "the service manager closed the connection"),
            Self::Console(error) => write!(formatter, "console: {error}"),
            Self::RootSwitch(error) => write!(formatter, "entering the new root: {error}"),
            Self::ManagerUnreachable => write!(formatter, "the service manager did not come back"),
        }
    }
}

impl error::Error for Error {}

impl From<BusError> for Error {
    fn from(error: BusError) -> Self {
        Self::Bus(error)
    }
}

impl From<TypeMismatchError> for Error {
    fn from(error: TypeMismatchError) -> Self {
        Self::Reply(error)
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Console(error)
    }
}

impl From<Errno> for Error {
    fn from(error: Errno) -> Self {
        Self::RootSwitch(error)
    }
}
