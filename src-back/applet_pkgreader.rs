use std::{ffi::OsString, path::PathBuf};

use crate::error::{
        Error,
        Result
    };


// use clap::Parser;

// #[derive(Parser, Debug)]
// #[command()]
// struct Args {
//     /// pkgs 
//     #[arg()]
//     pkgs: Vec<String>,

//     /// The file to write result into
//     #[arg(short, long)]
//     result: PathBuf
// }

pub(crate) fn main<I, S>(_args: I) -> Result<()>
where
    I: Iterator<Item = S>,
    S: Into<OsString> + Clone,
{
    crate::init::prepare_and_drop()?;
    


    crate::init::finish()?;
    Err(Error::ImpossibleLogic)
}