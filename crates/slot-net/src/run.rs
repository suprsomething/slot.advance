//! Every command the network code runs goes through here: each has a time limit, and each
//! can be cancelled, which kills it at once (the `link_radio.rs` pattern). A child that does
//! not die straight away (stuck inside a driver) is left to be reaped later, so the caller
//! is never held up by it.
//!
//! Passwords and keys are never passed as arguments: the derived key only ever reaches a
//! configuration file under `/run` (see `wifi.rs`).

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A command to run: the program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    /// Whether to collect what it prints. Off for anything that may leave a background
    /// process behind (a daemon could hold the output pipe open indefinitely).
    pub capture: bool,
}

impl Cmd {
    /// A command whose output is collected.
    pub fn new(program: &str, args: &[&str]) -> Cmd {
        Cmd {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            capture: true,
        }
    }

    /// A command whose output is thrown away.
    pub fn quiet(program: &str, args: &[&str]) -> Cmd {
        Cmd {
            capture: false,
            ..Cmd::new(program, args)
        }
    }

    /// The command as one line, for logs and tests.
    pub fn line(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    /// The exit code, if it exited by itself.
    pub code: Option<i32>,
    pub stdout: String,
    pub timed_out: bool,
    pub cancelled: bool,
}

impl Ran {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    pub fn failed(why: &str) -> Ran {
        Ran {
            code: None,
            stdout: why.to_string(),
            timed_out: false,
            cancelled: false,
        }
    }
}

/// Lets a running job be abandoned. Cloned freely; every clone sees the same flag.
#[derive(Clone, Debug, Default)]
pub struct Cancel {
    raised: Arc<AtomicU64>,
    seen: u64,
}

impl Cancel {
    pub fn new() -> Cancel {
        Cancel::default()
    }

    /// A token that reports cancelled once `raise` has been called after this point.
    pub fn token(&self) -> Cancel {
        Cancel {
            raised: self.raised.clone(),
            seen: self.raised.load(Ordering::SeqCst),
        }
    }

    pub fn raise(&self) {
        self.raised.fetch_add(1, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.raised.load(Ordering::SeqCst) != self.seen
    }
}

pub trait Runner: Send {
    fn run(&mut self, cmd: &Cmd, limit: Duration, cancel: &Cancel) -> Ran;
}

/// The real thing.
#[derive(Default)]
pub struct System {
    stuck: Arc<Mutex<Vec<Child>>>,
}

impl System {
    pub fn new() -> System {
        System::default()
    }

    fn reap(&self) {
        if let Ok(mut stuck) = self.stuck.lock() {
            stuck.retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_)) | Err(_)));
        }
    }
}

impl Runner for System {
    fn run(&mut self, cmd: &Cmd, limit: Duration, cancel: &Cancel) -> Ran {
        self.reap();
        if cancel.is_cancelled() {
            return Ran {
                cancelled: true,
                ..Ran::default()
            };
        }
        let mut child = match Command::new(&cmd.program)
            .args(&cmd.args)
            .stdin(Stdio::null())
            .stdout(if cmd.capture {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return Ran::failed(&format!("{}: {e}", cmd.program)),
        };
        // Read output on its own thread, so a chatty child can never fill the pipe and stall.
        let reader = child.stdout.take().map(|mut out| {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut s = Vec::new();
                let _ = out.read_to_end(&mut s);
                let _ = tx.send(String::from_utf8_lossy(&s).into_owned());
            });
            rx
        });
        let until = Instant::now() + limit;
        let mut ran = Ran::default();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    ran.code = status.code();
                    break;
                }
                Ok(None) => {}
                Err(_) => break,
            }
            let stop = if cancel.is_cancelled() {
                ran.cancelled = true;
                true
            } else if Instant::now() >= until {
                ran.timed_out = true;
                true
            } else {
                false
            };
            if stop {
                let _ = child.kill();
                // Give it a moment to go; if it is stuck in the kernel, leave it to be
                // reaped later rather than wait.
                let gone = (0..10).any(|_| {
                    std::thread::sleep(Duration::from_millis(2));
                    matches!(child.try_wait(), Ok(Some(_)))
                });
                if !gone {
                    if let Ok(mut stuck) = self.stuck.lock() {
                        stuck.push(child);
                    }
                    // The reader would wait for the stuck child; do not wait for it.
                    return ran;
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(rx) = reader {
            // Something it started may still hold the pipe; never wait long for it.
            ran.stdout = rx
                .recv_timeout(Duration::from_millis(500))
                .unwrap_or_default();
        }
        ran
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_and_exit_codes_come_back() {
        let mut s = System::new();
        let r = s.run(
            &Cmd::new("/bin/sh", &["-c", "echo hello; exit 3"]),
            Duration::from_secs(5),
            &Cancel::new(),
        );
        assert_eq!(r.code, Some(3));
        assert_eq!(r.stdout, "hello\n");
        assert!(!r.ok());
    }

    #[test]
    fn a_slow_command_is_killed_at_its_limit_with_its_output_so_far() {
        let mut s = System::new();
        let began = Instant::now();
        let r = s.run(
            &Cmd::new("/bin/sh", &["-c", "echo early; exec sleep 30"]),
            Duration::from_millis(300),
            &Cancel::new(),
        );
        assert!(r.timed_out);
        assert!(began.elapsed() < Duration::from_secs(3));
        assert_eq!(r.stdout, "early\n");
    }

    #[test]
    fn cancelling_kills_the_command_at_once() {
        let mut s = System::new();
        let cancel = Cancel::new();
        let token = cancel.token();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            c2.raise();
        });
        let began = Instant::now();
        let r = s.run(
            &Cmd::new("/bin/sh", &["-c", "exec sleep 30"]),
            Duration::from_secs(30),
            &token,
        );
        assert!(r.cancelled);
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
    }

    #[test]
    fn a_background_child_holding_the_pipe_never_holds_up_the_caller() {
        let mut s = System::new();
        let began = Instant::now();
        let r = s.run(
            &Cmd::new("/bin/sh", &["-c", "echo started; sleep 5 & exit 0"]),
            Duration::from_secs(5),
            &Cancel::new(),
        );
        assert_eq!(r.code, Some(0));
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
        let q = s.run(
            &Cmd::quiet("/bin/sh", &["-c", "echo hidden; sleep 5 & exit 0"]),
            Duration::from_secs(5),
            &Cancel::new(),
        );
        assert!(q.ok() && q.stdout.is_empty());
    }

    #[test]
    fn a_token_only_sees_cancels_raised_after_it() {
        let c = Cancel::new();
        c.raise();
        let t = c.token();
        assert!(!t.is_cancelled());
        c.raise();
        assert!(t.is_cancelled());
    }

    #[test]
    fn a_missing_program_is_a_failure_not_a_panic() {
        let mut s = System::new();
        let r = s.run(
            &Cmd::new("/nonexistent/prog", &[]),
            Duration::from_secs(1),
            &Cancel::new(),
        );
        assert!(!r.ok());
        assert!(r.stdout.contains("prog"));
    }
}
