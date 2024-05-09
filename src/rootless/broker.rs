use std::io::{stdin, Write};

use serde::{Serialize, Deserialize};

use crate::{Error, Result};

use super::init::InitPayload;

#[derive(Serialize, Deserialize)]
pub(crate) enum BrokerCommand {
    Run {
        program: String,
        args: Vec<String>,
    },
    MountProc,
}

impl BrokerCommand {
    fn work(self) -> Result<()> {
        match self {
            BrokerCommand::Run { program, args } => todo!(),
            BrokerCommand::MountProc => todo!(),
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BrokerPayload {
    pub(crate) init_payload: InitPayload,
    pub(crate) commands: Vec<BrokerCommand>,
}

impl BrokerPayload {
    pub(crate) fn try_read() -> Result<Self> {
        let payload = rmp_serde::from_read(stdin())?;
        Ok(payload)
    }

    pub(crate) fn try_write<W: Write>(&self, mut writer: W) -> Result<()> {
        let value = rmp_serde::to_vec(self)?;
        writer.write_all(&value)?;
        Ok(())
    }

    pub(crate) fn work(self) -> Result<()> {
        for command in self.commands {
            command.work()?
        }
        Ok(())
    }
}