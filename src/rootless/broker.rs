use std::{ffi::OsString, io::{stdin, Write}, iter::empty, path::{Path, PathBuf}};

use serde::{Serialize, Deserialize};

use crate::{mount::{mount_all, mount_all_except_proc}, Error, Result};

use super::{action::run_action_stateless_no_program_no_args, init::InitPayload};

#[derive(Serialize, Deserialize)]
pub(crate) enum BrokerCommand {
    MountForRoot { // Mount everything for 
        root: OsString,
    },
}

impl BrokerCommand {
    fn work(self) -> Result<()> {
        match self {
            BrokerCommand::MountForRoot { root } => 
                mount_all_except_proc(root),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BrokerPayload {
    pub(crate) init_payload: InitPayload,
    pub(crate) commands: Vec<BrokerCommand>,
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
}