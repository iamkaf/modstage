use super::*;

pub(super) struct TimedOutput {
    pub(super) status: std::process::ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) timed_out: bool,
}

pub(super) fn run_process_with_timeout(
    command: &mut Command,
    timeout: Option<Duration>,
) -> Result<TimedOutput, String> {
    let Some(timeout) = timeout else {
        let output = command
            .output()
            .map_err(|error| format!("process execution failed: {error}"))?;
        return Ok(TimedOutput {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
            timed_out: false,
        });
    };

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("process spawn failed: {error}"))?;
    let deadline = Instant::now() + timeout;

    loop {
        if child
            .try_wait()
            .map_err(|error| format!("process wait failed: {error}"))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("process output collection failed: {error}"))?;
            return Ok(TimedOutput {
                status: output.status,
                stdout: output.stdout,
                stderr: output.stderr,
                timed_out: false,
            });
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|error| format!("process output collection failed: {error}"))?;
            return Ok(TimedOutput {
                status: output.status,
                stdout: output.stdout,
                stderr: output.stderr,
                timed_out: true,
            });
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn parse_duration(value: &str) -> Result<Duration, String> {
    if let Some(ms) = value.strip_suffix("ms") {
        let millis = ms
            .parse()
            .map_err(|error| format!("invalid timeout `{value}`: {error}"))?;
        return Ok(Duration::from_millis(millis));
    }

    if let Some(seconds) = value.strip_suffix('s') {
        let seconds = seconds
            .parse()
            .map_err(|error| format!("invalid timeout `{value}`: {error}"))?;
        return Ok(Duration::from_secs(seconds));
    }

    Err(format!("timeout `{value}` must use `ms` or `s`"))
}
