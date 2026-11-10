//! Runs an external program and streams its output line by line.
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

/// A function that receives one line of output. Cheap to clone, safe to share between threads.
pub type Log = Arc<dyn Fn(String) + Send + Sync>;

/// Don't flash a black console window when we start a child process (Windows only).
#[cfg(windows)]
pub fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
#[cfg(not(windows))]
pub fn no_window(_cmd: &mut Command) {}

/// Run `program args...`, send every output line to `log`, return the exit code.
pub fn run<S: AsRef<OsStr>>(
    program: S,
    args: &[String],
    env: &[(String, String)],
    stdin: Option<&str>,
    log: &Log,
) -> std::io::Result<i32> {
    run_notify(program, args, env, stdin, log, &|_| {})
}

/// Same as `run`, but also tells the caller the process id as soon as it starts
/// (needed to be able to stop a long-running program like the emulator).
pub fn run_notify<S: AsRef<OsStr>>(
    program: S,
    args: &[String],
    env: &[(String, String)],
    stdin: Option<&str>,
    log: &Log,
    on_spawn: &dyn Fn(u32),
) -> std::io::Result<i32> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    no_window(&mut cmd);
    let mut child = cmd.spawn()?;
    on_spawn(child.id());

    // Feed the (short) stdin text, then close it by dropping the handle.
    if let Some(mut si) = child.stdin.take() {
        if let Some(text) = stdin {
            let _ = si.write_all(text.as_bytes());
        }
    }

    let stderr = child.stderr.take().expect("stderr piped");
    let stdout = child.stdout.take().expect("stdout piped");
    let log2 = Arc::clone(log);
    let t = thread::spawn(move || pump(stderr, &log2));
    pump(stdout, log);
    let _ = t.join();

    Ok(child.wait()?.code().unwrap_or(-1))
}

/// Read a stream and call `log` for each line. Splits on \n and \r so progress bars work too.
fn pump<R: Read>(mut r: R, log: &Log) {
    let mut buf = [0u8; 4096];
    let mut line: Vec<u8> = Vec::new();
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                for &b in &buf[..n] {
                    if b == b'\n' || b == b'\r' {
                        flush(&mut line, log);
                    } else {
                        line.push(b);
                    }
                }
            }
        }
    }
    flush(&mut line, log);
}

fn flush(line: &mut Vec<u8>, log: &Log) {
    if !line.is_empty() {
        let s = String::from_utf8_lossy(line).trim().to_string();
        if !s.is_empty() {
            log(s);
        }
        line.clear();
    }
}
