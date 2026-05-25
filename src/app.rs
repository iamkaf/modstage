use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod cli;
mod commands;
mod config;
mod hash;
mod java;
mod metadata;
mod resolve;
mod run;
mod run_forge;
mod run_launch;
mod run_lock;
mod run_process;
mod run_report;
mod run_staging;
mod state;

use commands::*;
use config::*;
use hash::*;
use java::*;
use metadata::*;
use resolve::*;
use run::*;
use run_forge::*;
use run_launch::*;
use run_lock::*;
use run_process::*;
use run_report::*;
use run_staging::*;
use state::*;

pub(crate) fn run(args: Vec<String>) -> Result<(), String> {
    cli::run(args)
}
