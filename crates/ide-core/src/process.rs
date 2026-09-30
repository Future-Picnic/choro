//! Bounded subprocess capture for background operations.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;

/// Capture both streams while the child runs. The deadline includes waiting
/// for inherited pipes to close, even if the direct child has already exited.
/// Call this on a background thread, never on the UI thread.
pub fn output_with_timeout(command: &mut Command, timeout: Duration) -> Result<Output> {
    crate::blocking_guard::debug_warn_if_ui_thread("process::output_with_timeout");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command.spawn().context("Could not start the command")?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout_reader = thread::spawn(move || read_output(stdout));
    let stderr_reader = thread::spawn(move || read_output(stderr));
    let started = Instant::now();
    let result = (|| loop {
        let status = child.try_wait().context("Could not poll the command")?;
        if let Some(status) = status {
            if stdout_reader.is_finished() && stderr_reader.is_finished() {
                return Ok(status);
            }
        }
        if started.elapsed() >= timeout {
            anyhow::bail!("Command timed out after {} seconds", timeout.as_secs());
        }
        thread::sleep(Duration::from_millis(20).min(timeout.saturating_sub(started.elapsed())));
    })();

    let status = match result {
        Ok(status) => status,
        Err(error) => {
            // Helpers can outlive the direct child and keep its pipes open.
            #[cfg(unix)]
            unsafe {
                let _ = libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            // Do not join readers here: an escaped descendant may still own a
            // pipe. Returning the error must remain bounded as well.
            return Err(error);
        }
    };
    Ok(Output {
        status,
        stdout: stdout_reader
            .join()
            .map_err(|_| anyhow!("Could not collect stdout"))??,
        stderr: stderr_reader
            .join()
            .map_err(|_| anyhow!("Could not collect stderr"))??,
    })
}

fn read_output(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)?;
    Ok(output)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn captures_output_larger_than_both_pipe_buffers() {
        let output = output_with_timeout(
            Command::new("/bin/sh").args([
                "-c",
                "head -c 262144 /dev/zero; head -c 262144 /dev/zero >&2; exit 7",
            ]),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout.len(), 262144);
        assert_eq!(output.stderr.len(), 262144);
    }

    #[test]
    fn times_out_a_running_command() {
        let started = Instant::now();
        let error = output_with_timeout(
            Command::new("/bin/sh").args(["-c", "sleep 5"]),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn deadline_covers_pipes_inherited_by_a_descendant() {
        let started = Instant::now();
        let error = output_with_timeout(
            Command::new("/bin/sh").args(["-c", "sleep 5 & printf ready; exit 0"]),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
