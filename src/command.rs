// Copyright (C) 2022 Electronic Arts, Inc. All rights reserved.

//! Run a binary executable
//!
//! This will run a Unix command, and keep track of stdout, stderr, and any errors.

use std::{
    error::Error,
    ffi::OsString,
    fmt::Display,
    process::{Output, Stdio},
    str::{self, Utf8Error},
    time::{Duration, Instant},
};
use tokio::{io, process::Command};

use crate::duration::duration_string;

#[derive(Debug)]
pub struct CommandSuccess {
    pub cmd: CommandLine,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub exit_code: i32, // This field stores the exit code of the command to determine if it was successful or not, and whether or not there were any diffs.
}

impl CommandSuccess {
    #[allow(clippy::unused_self)]
    pub fn result_line(&self) -> String {
        "Command was successful".to_string()
    }
}

impl Display for CommandSuccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let summary = self.result_line();

        f.write_str("result: ")?;
        f.write_str(&summary)?;
        f.write_str("\n")?;

        f.write_str("exit code: ")?;
        f.write_str(&self.exit_code.to_string())?;
        f.write_str("\n")?;

        f.write_str("command: ")?;
        f.write_str(&self.cmd.to_string())?;
        f.write_str("\n")?;

        f.write_str("duration: ")?;
        f.write_str(&duration_string(&self.duration))?;
        f.write_str("\n")?;

        f.write_str("stdout:\n")?;
        f.write_str(&self.stdout)?;
        f.write_str("\n")?;

        f.write_str("stderr:\n")?;
        for line in self.stderr.lines() {
            f.write_str("--> ")?;
            f.write_str(line)?;
            f.write_str("\n")?;
        }
        f.write_str("\n")?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct CommandError {
    pub cmd: CommandLine,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub exit_code: i32,
    pub kind: CommandErrorKind,
}

#[derive(Debug)]
pub enum CommandErrorKind {
    BadExitCode {},
    FailedToStart { err: std::io::Error },
    Utf8Error { err: Utf8Error },
}

impl From<Utf8Error> for CommandErrorKind {
    fn from(err: Utf8Error) -> Self {
        CommandErrorKind::Utf8Error { err }
    }
}

impl CommandError {
    pub fn result_line(&self) -> String {
        match &self.kind {
            CommandErrorKind::BadExitCode {} => format!("Bad Exit code {}", self.exit_code),
            CommandErrorKind::FailedToStart { err } => format!("Failed to start: {err}"),
            CommandErrorKind::Utf8Error { err } => format!("UTF-8 error: {err}"),
        }
    }
}

impl Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let summary = self.result_line();

        f.write_str("result: ")?;
        f.write_str(&summary)?;
        f.write_str("\n")?;

        f.write_str("exit code: ")?;
        f.write_str(&self.exit_code.to_string())?;
        f.write_str("\n")?;

        f.write_str("command: ")?;
        f.write_str(&self.cmd.to_string())?;
        f.write_str("\n")?;

        f.write_str("duration: ")?;
        f.write_str(&duration_string(&self.duration))?;
        f.write_str("\n")?;

        f.write_str("stdout:\n")?;
        for line in self.stdout.lines() {
            f.write_str("--> ")?;
            f.write_str(line)?;
            f.write_str("\n")?;
        }
        f.write_str("\n")?;

        f.write_str("stderr:\n")?;
        for line in self.stderr.lines() {
            f.write_str("--> ")?;
            f.write_str(line)?;
            f.write_str("\n")?;
        }
        f.write_str("\n")?;

        Ok(())
    }
}

impl Error for CommandError {}

pub type CommandResult = Result<CommandSuccess, CommandError>;

#[derive(Clone, Eq, PartialEq)]
pub struct CommandLine {
    cmd: OsString,
    args: Vec<OsString>,
    allowed_exit_codes: Vec<i32>,
}

impl CommandLine {
    pub fn new(cmd: OsString, args: Vec<OsString>) -> Self {
        CommandLine {
            cmd,
            args,
            allowed_exit_codes: vec![0],
        }
    }

    pub fn with_allowed_exit_codes(mut self, allowed_exit_codes: Vec<i32>) -> Self {
        self.allowed_exit_codes = allowed_exit_codes;
        self
    }
}

fn get_exit_code(output: &Result<Output, io::Error>) -> i32 {
    output
        .as_ref()
        .map_or(-1, |output| output.status.code().unwrap_or(-1))
}

fn get_stdin_out(output: &Result<Output, io::Error>) -> Result<(String, String), CommandErrorKind> {
    if let Ok(output) = &output {
        let stdin = str::from_utf8(&output.stdout)?;
        let stderr = str::from_utf8(&output.stderr)?;
        Ok((stdin.to_string(), stderr.to_string()))
    } else {
        Ok((String::new(), String::new()))
    }
}

impl CommandLine {
    pub async fn run(&self) -> CommandResult {
        let start = Instant::now();

        let CommandLine {
            cmd,
            args,
            allowed_exit_codes,
        } = &self;
        let output = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        let exit_code = get_exit_code(&output);
        let duration = start.elapsed();

        let (stdout, stderr) = match get_stdin_out(&output) {
            Ok(output) => output,
            Err(err) => {
                return Err(CommandError {
                    cmd: self.clone(),
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code,
                    duration,
                    kind: err,
                });
            }
        };

        let kind = match output {
            Err(err) => Err(CommandErrorKind::FailedToStart { err }),
            Ok(_output) => {
                if allowed_exit_codes.contains(&exit_code) {
                    Ok(())
                } else {
                    Err(CommandErrorKind::BadExitCode {})
                }
            }
        };

        match kind {
            Ok(()) => Ok(CommandSuccess {
                cmd: self.clone(),
                stdout,
                stderr,
                exit_code,
                duration,
            }),
            Err(kind) => Err(CommandError {
                cmd: self.clone(),
                stdout,
                stderr,
                exit_code,
                duration,
                kind,
            }),
        }
    }
}

impl Display for CommandLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cmd.to_string_lossy())?;
        for arg in &self.args {
            write!(f, " {}", arg.to_string_lossy())?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for CommandLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CommandLine(\"{:?}", self.cmd)?;
        for arg in &self.args {
            write!(f, " {arg:?}")?;
        }
        write!(f, "\")")?;
        Ok(())
    }
}
