use std::{process::Command, ffi::OsStr, fs::read_link};

use clap::Command;
/// Broker: the broker protocol and implementation

use serde::{Deserialize, Serialize};

use crate::error::{
        Error,
        Result
    };

/// The applets allowed to be spawned by the broker, not all applets allowed here
/// These applets would also need to implement the fake-init function
#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Applet {
    Pkgreader
}

#[derive(Debug, PartialEq, Deserialize)]
pub(crate) enum Delegant {
    External (String),
    Internal (Applet),
}

#[derive(Debug, PartialEq, Deserialize)]
pub(crate) struct Protocol {
    /// What we should call downwards
    delegant: Delegant,
    /// Whether we would be mapped to root
    maproot: bool,
    /// The args we would pass to downstream
    args: Vec<String>,
}

impl Protocol {
    fn new<I, S>(delegant: Delegant, maproot: bool, args: I) -> Self 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>
    {
        Self {
            delegant,
            maproot,
            args: args.into_iter().map(|arg|
                arg.as_ref().to_string()).collect(),
        }
    }

    fn call(&self) -> Result<()> {
        let (mut command, arg1) = match self.delegant {
            Delegant::External(command) => (Command::new(command), ""),
            Delegant::Internal(applet) => {
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
        Ok(())
    }
}
