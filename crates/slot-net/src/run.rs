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
    /// Stop it early, as finished, once its output holds this text this many times
    /// (`ntpd -w` never exits by itself).
    pub stop_when: Option<(&'static str, usize)>,
}

impl Cmd {
    /// A command whose output is collected.
    pub fn new(program: &str, args: &[&str]) -> Cmd {
        Cmd {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            capture: true,
            stop_when: None,
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
    /// Everything it printed: standard output, then standard error. BusyBox tools such as
    /// `ntpd` report on standard error (the Phase 0 kits captured both).
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
        use std::os::unix::process::CommandExt;
        use std::sync::mpsc::{channel, Receiver};
        self.reap();
        if cancel.is_cancelled() {
            return Ran {
                cancelled: true,
                ..Ran::default()
            };
        }
        let pipe = || {
            if cmd.capture {
                Stdio::piped()
            } else {
                Stdio::null()
            }
        };
        // Its own process group, so stopping it also stops anything it started (such as
        // the DHCP client's script), which could otherwise bring wlan0 back up.
        let mut child = match Command::new(&cmd.program)
            .args(&cmd.args)
            .stdin(Stdio::null())
            .stdout(pipe())
            .stderr(pipe())
            .process_group(0)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return Ran::failed(&format!("{}: {e}", cmd.program)),
        };
        // Output is read on its own threads, a chunk at a time, so a chatty child never
        // fills a pipe and stalls, and what has arrived can be looked at while it runs.
        fn reader<R: Read + Send + 'static>(pipe: Option<R>) -> Option<Receiver<String>> {
            pipe.map(|mut out| {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    loop {
                        match out.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if tx
                                    .send(String::from_utf8_lossy(&buf[..n]).into_owned())
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                });
                rx
            })
        }
        let readers: Vec<Receiver<String>> =
            [reader(child.stdout.take()), reader(child.stderr.take())]
                .into_iter()
                .flatten()
                .collect();
        let mut got = [String::new(), String::new()];
        let drain = |got: &mut [String; 2]| {
            for (i, rx) in readers.iter().enumerate() {
                while let Ok(chunk) = rx.try_recv() {
                    got[i].push_str(&chunk);
                }
            }
        };
        let pid = child.id() as libc::pid_t;
        let until = Instant::now() + limit;
        let mut ran = Ran::default();
        let mut enough = false;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    ran.code = status.code();
                    break;
                }
                Ok(None) => {}
                Err(_) => break,
            }
            drain(&mut got);
            if let Some((needle, n)) = cmd.stop_when {
                enough = got.iter().map(|g| g.matches(needle).count()).sum::<usize>() >= n;
            }
            let stop = if enough {
                true
            } else if cancel.is_cancelled() {
                ran.cancelled = true;
                true
            } else if Instant::now() >= until {
                ran.timed_out = true;
                true
            } else {
                false
            };
            if stop {
                // SAFETY: signals the child's own process group (made for it above).
                unsafe { libc::kill(-pid, libc::SIGKILL) };
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
                    drain(&mut got);
                    ran.stdout = got.concat();
                    return ran;
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // Collect the rest; something it started may still hold a pipe, so never wait long.
        let deadline = Instant::now() + Duration::from_millis(500);
        for (i, rx) in readers.iter().enumerate() {
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(chunk) => got[i].push_str(&chunk),
                    Err(_) => break,
                }
            }
        }
        ran.stdout = got.concat();
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
    fn what_goes_to_standard_error_is_collected_too() {
        let mut s = System::new();
        let r = s.run(
            &Cmd::new(
                "/bin/sh",
                &[
                    "-c",
                    "echo out; echo 'ntpd: reply from 1.2.3.4: offset:+1.5' >&2",
                ],
            ),
            Duration::from_secs(5),
            &Cancel::new(),
        );
        assert_eq!(r.stdout, "out\nntpd: reply from 1.2.3.4: offset:+1.5\n");
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
    fn it_can_stop_once_enough_has_been_said() {
        let mut s = System::new();
        let began = Instant::now();
        let mut cmd = Cmd::new(
            "/bin/sh",
            &[
                "-c",
                "echo 'reply from a' >&2; echo 'reply from b' >&2; exec sleep 30",
            ],
        );
        cmd.stop_when = Some(("reply from", 2));
        let r = s.run(&cmd, Duration::from_secs(20), &Cancel::new());
        assert!(began.elapsed() < Duration::from_secs(3));
        assert!(!r.timed_out && !r.cancelled);
        assert_eq!(r.stdout.matches("reply from").count(), 2);
    }

    #[test]
    fn stopping_it_stops_what_it_started() {
        let d = tempfile::tempdir().unwrap();
        let mark = d.path().join("still-running");
        let script = format!("(sleep 1; touch {}) & exec sleep 30", mark.display());
        let mut s = System::new();
        let r = s.run(
            &Cmd::quiet("/bin/sh", &["-c", &script]),
            Duration::from_millis(200),
            &Cancel::new(),
        );
        assert!(r.timed_out);
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!mark.exists(), "a process it started kept running");
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
