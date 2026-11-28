//! Spawning the binary for `jig cli-help`: once without a terminal (COLUMNS
//! unset or set), once under a pseudo-terminal of each width. stdin is closed
//! for the no-terminal runs; under a pty all three streams are the terminal,
//! as in a real shell. Every call is capped at 10 s.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::{Mode, Output, normalize};

const TIMEOUT: Duration = Duration::from_secs(10);

/// The argument jig passes to provoke an unknown-flag error.
pub const UNKNOWN: &str = "--jig-cli-help-unknown-flag";

/// A program and its leading arguments (`-- <BIN> [ARGS...]`).
pub struct Target {
    pub program: String,
    pub args: Vec<String>,
}

impl Target {
    fn command(&self, extra: &[String]) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args).args(extra);
        c.env_remove("COLUMNS")
            .env_remove("LINES")
            .env("NO_COLOR", "1")
            .env_remove("CLICOLOR_FORCE");
        c
    }

    /// Run with `extra` arguments in `mode`. Err means jig could not judge
    /// (the program would not start, or a call hung).
    pub fn call(&self, extra: &[String], mode: Mode) -> Result<Output, String> {
        let shown = || format!("{} {}", self.display(), extra.join(" "));
        match mode {
            Mode::Plain | Mode::Columns(_) => {
                let mut c = self.command(extra);
                if let Mode::Columns(w) = mode {
                    c.env("COLUMNS", w.to_string());
                }
                c.stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut child = c
                    .spawn()
                    .map_err(|e| format!("cannot run {}: {e}", shown()))?;
                let mut so = child.stdout.take().ok_or("no stdout pipe")?;
                let mut se = child.stderr.take().ok_or("no stderr pipe")?;
                let t_out = thread::spawn(move || {
                    let mut b = Vec::new();
                    let _ = so.read_to_end(&mut b);
                    b
                });
                let t_err = thread::spawn(move || {
                    let mut b = Vec::new();
                    let _ = se.read_to_end(&mut b);
                    b
                });
                let code = wait(&mut child).map_err(|e| format!("{}: {e}", shown()))?;
                let out = t_out.join().unwrap_or_default();
                let _ = t_err.join();
                Ok(Output {
                    code,
                    stdout: normalize(&out),
                })
            }
            Mode::Tty(w) => self
                .call_tty(extra, w)
                .map_err(|e| format!("{}: {e}", shown())),
        }
    }

    fn call_tty(&self, extra: &[String], cols: usize) -> Result<Output, String> {
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        use rustix::termios::{Winsize, tcsetwinsize};

        let master =
            openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).map_err(|e| format!("openpt: {e}"))?;
        grantpt(&master).map_err(|e| format!("grantpt: {e}"))?;
        unlockpt(&master).map_err(|e| format!("unlockpt: {e}"))?;
        let name = ptsname(&master, Vec::new()).map_err(|e| format!("ptsname: {e}"))?;
        let path = name
            .to_str()
            .map_err(|_| "pty name is not UTF-8")?
            .to_string();
        let slave: File = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("open pty: {e}"))?;
        let ws = Winsize {
            ws_row: 50,
            ws_col: u16::try_from(cols).unwrap_or(u16::MAX),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        tcsetwinsize(&slave, ws).map_err(|e| format!("set pty size: {e}"))?;
        let mut c = self.command(extra);
        let s = |f: &File| {
            f.try_clone()
                .map(Stdio::from)
                .map_err(|e| format!("pty: {e}"))
        };
        c.stdin(s(&slave)?).stdout(s(&slave)?).stderr(s(&slave)?);
        let mut child = c.spawn().map_err(|e| format!("cannot run: {e}"))?;
        drop(c);
        let mut reader = File::from(master);
        let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = collected.clone();
        let t = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => sink
                        .lock()
                        .map(|mut v| v.extend_from_slice(&buf[..n]))
                        .unwrap_or(()),
                }
            }
        });
        let code = wait(&mut child)?;
        // Let the reader drain what the child wrote before the last slave
        // handle closes: wait until the buffer stops growing.
        let mut last = usize::MAX;
        let start = Instant::now();
        loop {
            thread::sleep(Duration::from_millis(30));
            let n = collected.lock().map(|v| v.len()).unwrap_or(0);
            if n == last || start.elapsed() > Duration::from_secs(2) {
                break;
            }
            last = n;
        }
        drop(slave);
        let _ = t.join();
        let raw = collected.lock().map(|v| v.clone()).unwrap_or_default();
        // Under a pty stdout and stderr share one stream; help goes to stdout.
        Ok(Output {
            code,
            stdout: normalize(&raw),
        })
    }

    pub fn display(&self) -> String {
        let mut s = self.program.clone();
        for a in &self.args {
            s.push(' ');
            s.push_str(a);
        }
        s
    }
}

fn wait(child: &mut Child) -> Result<Option<i32>, String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Ok(st.code()),
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("no exit after {} s", TIMEOUT.as_secs()));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(e.to_string()),
        }
    }
}
