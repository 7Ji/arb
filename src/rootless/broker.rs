use std::{ffi::OsString, io::{stdin, Write}, iter::empty, path::{Path, PathBuf}};

use serde::{Serialize, Deserialize};

use crate::{mount::{mount_all, mount_all_except_proc}, Error, Result};

use super::{action::run_action_stateless_no_program_no_args, init::InitPayload, InitCommand};

#[derive(Serialize, Deserialize)]
pub(crate) enum BrokerCommand {
    MountForRoot { // Mount everything for 
        root: OsString,
    },
}

impl BrokerCommand {
    fn work(self) -> Result<()> {
        match self {
            BrokerCommand::MountForRoot { root } => {
                log::debug!("Mounting for root '{}'", root.to_string_lossy());
                mount_all_except_proc(root)
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BrokerPayload {
    init_payload: InitPayload,
    commands: Vec<BrokerCommand>,
}

impl BrokerPayload {
    pub(crate) fn new_with_root<P: AsRef<Path>>(root: P) -> Self {
        let command = BrokerCommand::MountForRoot { 
            root: root.as_ref().into() };
        Self {
            init_payload: InitPayload::new_with_root(root),
            commands: vec![command],
        }
    }

    pub(crate) fn try_read() -> Result<Self> {
        match rmp_serde::from_read(stdin()) {
            Ok(payload) => Ok(payload),
            Err(e) => {
                log::error!("Failed to deserialize broker payload from stdin: \
                            {}", e);
                Err(e.into())
            },
        }
    }

    pub(crate) fn try_into_bytes(&self) -> Result<Vec<u8>> {
        match rmp_serde::to_vec(self) {
            Ok(bytes) => Ok(bytes),
            Err(e) => {
                log::error!("Failed to serialize broker payload to bytes: {}", 
                            e);
                Err(e.into())
            },
        }
    }

    pub(crate) fn work(self) -> Result<()> {
        for command in self.commands {
            command.work()?
        }
        run_action_stateless_no_program_no_args(
            "init", Some(self.init_payload.try_into_bytes()?))
    }

    pub(crate) fn add_command(&mut self, command: BrokerCommand) {
        self.commands.push(command)
    }

    pub(crate) fn add_init_command(&mut self, command: InitCommand) {
        self.init_payload.add_command(command)
    }

    pub(crate) fn add_init_command_run_program<S1, S2, I, S3>(
        &mut self, logfile: S1, program: S2, args: I
    ) 
    where
        S1: Into<OsString>,
        S2: Into<OsString>,
        I: IntoIterator<Item = S3>,
        S3: Into<OsString>
    {
        self.init_payload.add_command_run_program(logfile, program, args)
    }

    pub(crate) fn add_init_command_run_applet<S1, I, S2>(
        &mut self, applet: S1, args: I
    ) 
    where
        S1: Into<OsString>,
        I: IntoIterator<Item = S2>,
        S2: Into<OsString>
    {
        self.init_payload.add_command_run_applet(applet, args)
    }
}