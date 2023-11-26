use std::{process::{Command, Stdio}, ffi::OsStr, fs::read_link};

use clap::Command;
/// Broker: the broker protocol and implementation

use serde::{Deserialize, Serialize};

use crate::error::{
        Error,
        Result
    };

/// The applets allowed to be spawned by the broker, not all applets allowed here
/// These applets would also need to implement the fake-init function
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Applet {
    Pkgreader
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub(crate) enum Delegation {
    External (String),
    Internal (Applet),
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub(crate) enum Network {
    None,
    Local,
    Full
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct Protocol {
    /// What we should call downwards
    delegation: Delegation,
    /// Drop we drop root identity
    drop: bool,
    /// Should the environment has network
    net: Network,
    /// The args we would pass to downstream
    args: Vec<String>,
}

impl Protocol {
    fn new<I, S>(delegation: Delegation, drop: bool, net: Network, args: I) -> Self 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>
    {
        Self {
            delegation,
            drop,
            net,
            args: args.into_iter().map(|arg|
                arg.as_ref().to_string()).collect(),
        }
    }

    // fn setup() -> Result<()> {

    // }

    fn call(&self) -> Result<()> {
        let (mut command, arg1) = match self.delegation {
            Delegation::External(command) => (Command::new(command), ""),
            Delegation::Internal(applet) => {
                let arg0 = match read_link("/proc/self/exe") {
                    Ok(arg0) => arg0,
                    Err(e) => return Err(e.into()),
                };
                let applet = match applet {
                    Applet::Pkgreader => "pkgreader",
                };
                (Command::new(arg0), applet)
            },
        };
        if ! arg1.is_empty() {
            command.arg(arg1);
        }
        command.args(&self.args);
        command.stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        serde_json::to_writer(child.stdin.take().unwrap(), self);
        Ok(())
    }
}
