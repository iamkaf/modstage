use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod cli;
mod commands;
mod config;
mod hash;
mod java;
mod metadata;
mod resolve;
mod run;
mod state;

use commands::*;
use config::*;
use hash::*;
use java::*;
use metadata::*;
use resolve::*;
use run::*;
use state::*;

pub(crate) fn run(args: Vec<String>) -> Result<(), String> {
    cli::run(args)
}
