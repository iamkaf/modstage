use super::*;

pub(super) struct TimedOutput {
    pub(super) status: std::process::ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) timed_out: bool,
    pub(super) streamed: bool,
    pub(super) graceful_stop: bool,
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
            streamed: false,
            graceful_stop: false,
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
                streamed: false,
                graceful_stop: false,
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
                streamed: false,
                graceful_stop: false,
            });
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn run_server_process_with_timeout(
    command: &mut Command,
    timeout: Option<Duration>,
) -> Result<TimedOutput, String> {
    ProcessSupervisor::new(ProcessPolicy::Server).run(command, timeout)
}

pub(super) fn run_client_process_with_timeout(
    command: &mut Command,
    timeout: Option<Duration>,
) -> Result<TimedOutput, String> {
    ProcessSupervisor::new(ProcessPolicy::Client).run(command, timeout)
}

struct ProcessSupervisor {
    policy: ProcessPolicy,
}

impl ProcessSupervisor {
    fn new(policy: ProcessPolicy) -> Self {
        Self { policy }
    }

    fn run(&self, command: &mut Command, timeout: Option<Duration>) -> Result<TimedOutput, String> {
        if matches!(self.policy, ProcessPolicy::Server) {
            command.stdin(Stdio::piped());
        }

        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("process spawn failed: {error}"))?;
        let mut child_stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to capture process stdout".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "failed to capture process stderr".to_string())?;
        let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
        let stderr_buffer = Arc::new(Mutex::new(Vec::new()));
        let (event_sender, event_receiver) = mpsc::channel();
        let stdout_thread = spawn_process_reader(
            stdout,
            Arc::clone(&stdout_buffer),
            Some(event_sender),
            ProcessStream::Stdout,
        );
        let stderr_thread = spawn_process_reader(
            stderr,
            Arc::clone(&stderr_buffer),
            None,
            ProcessStream::Stderr,
        );
        let deadline = timeout.map(|timeout| Instant::now() + timeout);
        let mut sent_stop = false;

        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("process wait failed: {error}"))?
            {
                stdout_thread
                    .join()
                    .map_err(|_| "stdout reader thread panicked".to_string())?;
                stderr_thread
                    .join()
                    .map_err(|_| "stderr reader thread panicked".to_string())?;
                return Ok(TimedOutput {
                    status,
                    stdout: clone_buffer(&stdout_buffer)?,
                    stderr: clone_buffer(&stderr_buffer)?,
                    timed_out: false,
                    streamed: true,
                    graceful_stop: false,
                });
            }

            for event in event_receiver.try_iter() {
                if self.policy.should_send_stop(&event, sent_stop) {
                    if let Some(mut stdin) = child_stdin.take() {
                        stdin.write_all(b"stop\n").map_err(|error| {
                            format!("failed to write server stop command: {error}")
                        })?;
                        stdin.flush().map_err(|error| {
                            format!("failed to flush server stop command: {error}")
                        })?;
                    }
                    sent_stop = true;
                } else if self.policy.should_finish_gracefully(&event, sent_stop) {
                    let _ = child.kill();
                    let status = child
                        .wait()
                        .map_err(|error| format!("process wait failed: {error}"))?;
                    stdout_thread
                        .join()
                        .map_err(|_| "stdout reader thread panicked".to_string())?;
                    stderr_thread
                        .join()
                        .map_err(|_| "stderr reader thread panicked".to_string())?;
                    return Ok(TimedOutput {
                        status,
                        stdout: clone_buffer(&stdout_buffer)?,
                        stderr: clone_buffer(&stderr_buffer)?,
                        timed_out: false,
                        streamed: true,
                        graceful_stop: true,
                    });
                }
            }

            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                let _ = child.kill();
                let status = child
                    .wait()
                    .map_err(|error| format!("process wait failed: {error}"))?;
                stdout_thread
                    .join()
                    .map_err(|_| "stdout reader thread panicked".to_string())?;
                stderr_thread
                    .join()
                    .map_err(|_| "stderr reader thread panicked".to_string())?;
                return Ok(TimedOutput {
                    status,
                    stdout: clone_buffer(&stdout_buffer)?,
                    stderr: clone_buffer(&stderr_buffer)?,
                    timed_out: true,
                    streamed: true,
                    graceful_stop: false,
                });
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

enum ProcessPolicy {
    Server,
    Client,
}

impl ProcessPolicy {
    fn should_send_stop(&self, event: &ProcessEvent, sent_stop: bool) -> bool {
        matches!(self, Self::Server) && matches!(event, ProcessEvent::Ready) && !sent_stop
    }

    fn should_finish_gracefully(&self, event: &ProcessEvent, sent_stop: bool) -> bool {
        match self {
            Self::Server => matches!(event, ProcessEvent::ShutdownComplete) && sent_stop,
            Self::Client => matches!(event, ProcessEvent::ClientReady),
        }
    }
}

enum ProcessStream {
    Stdout,
    Stderr,
}

enum ProcessEvent {
    Ready,
    ClientReady,
    ShutdownComplete,
}

fn spawn_process_reader<R>(
    stream: R,
    buffer: Arc<Mutex<Vec<u8>>>,
    event_sender: Option<mpsc::Sender<ProcessEvent>>,
    process_stream: ProcessStream,
) -> std::thread::JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        let mut saw_stopping = false;

        loop {
            line.clear();
            let Ok(read) = reader.read_until(b'\n', &mut line) else {
                break;
            };
            if read == 0 {
                break;
            }

            match process_stream {
                ProcessStream::Stdout => {
                    print!("{}", String::from_utf8_lossy(&line));
                    let _ = std::io::stdout().flush();
                }
                ProcessStream::Stderr => {
                    eprint!("{}", String::from_utf8_lossy(&line));
                    let _ = std::io::stderr().flush();
                }
            }

            if let Ok(mut output) = buffer.lock() {
                output.extend_from_slice(&line);
            }

            if let Some(sender) = &event_sender {
                let text = String::from_utf8_lossy(&line);
                if text.contains("Done (") && text.contains("For help, type") {
                    let _ = sender.send(ProcessEvent::Ready);
                }
                if text.contains("TeaKit listening on ") {
                    let _ = sender.send(ProcessEvent::ClientReady);
                }
                if text.contains("Stopping server") || text.contains("Stopping the server") {
                    saw_stopping = true;
                }
                if saw_stopping && text.contains("All dimensions are saved") {
                    let _ = sender.send(ProcessEvent::ShutdownComplete);
                }
            }
        }
    })
}

fn clone_buffer(buffer: &Arc<Mutex<Vec<u8>>>) -> Result<Vec<u8>, String> {
    buffer
        .lock()
        .map(|buffer| buffer.clone())
        .map_err(|_| "process output buffer lock poisoned".to_string())
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
