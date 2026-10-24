//! Syncthing's processes (refinement 1, checked with the real 2.1.6, plan 8.2).
//!
//! Even with `--no-restart`, `syncthing serve` runs as **two processes**: a monitor (our
//! child, which also writes the log file) and the real Syncthing (`STMONITORED=yes`, at
//! low priority, **in its own process group**). Measured:
//! - a REST shutdown or `SIGTERM` to the monitor stops both in about 0.4 to 0.5 s;
//! - `SIGKILL` to the monitor leaves the real Syncthing running (reparented to init);
//! - killing the real Syncthing makes the monitor exit too (`--no-restart`).
//!
//! So the supervisor finds the real Syncthing from `/proc` (its parent is the monitor) and
//! the final stage of a stop kills its process group as well as the monitor. Before each
//! start, any Syncthing still using our settings folder (left by a crash) is stopped too.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How the monitor ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal,
}

impl Exit {
    /// Syncthing's own exit code for "restart requested" (the web interface's Restart).
    pub const RESTART: Exit = Exit::Code(3);
}

/// What the supervisor needs from the operating system, so it can be tested without one.
pub trait Procs: Send {
    /// Starts `program args` with `env`, in a new process group, with no input or output.
    /// `on_exit` is called from another thread with its pid and how it ended (it is reaped
    /// there).
    fn spawn(
        &mut self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
        on_exit: Box<dyn FnOnce(u32, Exit) + Send>,
    ) -> std::io::Result<u32>;

    /// Another handle on the same operating system (for the power-off stop).
    fn handle(&self) -> Box<dyn Procs>;

    /// Processes whose parent is `pid`.
    fn children(&self, pid: u32) -> Vec<u32>;

    /// The process exists and has not exited (a zombie counts as gone).
    fn alive(&self, pid: u32) -> bool;

    /// Sends `sig` to `pid`, or to process group `pid` when `group`.
    fn signal(&self, pid: u32, sig: i32, group: bool);

    fn pgid(&self, pid: u32) -> Option<u32>;

    /// Processes running `program` with `--config=<config>` (ours, left over).
    fn using(&self, program: &Path, config: &Path) -> Vec<u32>;

    /// Runs `program args`, writing `input` to it, waiting at most `limit` (then killed).
    /// Returns its exit code and what it printed.
    fn run_with_input(
        &mut self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
        input: &str,
        limit: Duration,
    ) -> std::io::Result<(Option<i32>, String)>;
}

/// The real operating system.
#[derive(Default)]
pub struct System {
    proc_root: PathBuf,
}

impl System {
    pub fn new() -> System {
        System {
            proc_root: PathBuf::from("/proc"),
        }
    }

    fn stat(&self, pid: u32) -> Option<(char, u32, u32)> {
        let s = std::fs::read_to_string(self.proc_root.join(pid.to_string()).join("stat")).ok()?;
        // pid (comm) state ppid pgrp ...; comm may contain spaces and parentheses.
        let rest = &s[s.rfind(')')? + 2..];
        let mut f = rest.split_whitespace();
        let state = f.next()?.chars().next()?;
        let ppid = f.next()?.parse().ok()?;
        let pgrp = f.next()?.parse().ok()?;
        Some((state, ppid, pgrp))
    }

    fn pids(&self) -> Vec<u32> {
        let Ok(rd) = std::fs::read_dir(&self.proc_root) else {
            return Vec::new();
        };
        rd.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
            .collect()
    }
}

impl Procs for System {
    fn spawn(
        &mut self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
        on_exit: Box<dyn FnOnce(u32, Exit) + Send>,
    ) -> std::io::Result<u32> {
        use std::os::unix::process::{CommandExt, ExitStatusExt};
        let mut child = Command::new(program)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        let pid = child.id();
        std::thread::Builder::new()
            .name("slot-sync-wait".into())
            .spawn(move || {
                let exit = match child.wait() {
                    Ok(s) => match s.code() {
                        Some(c) => Exit::Code(c),
                        None if s.signal().is_some() => Exit::Signal,
                        None => Exit::Signal,
                    },
                    Err(_) => Exit::Signal,
                };
                on_exit(pid, exit);
            })?;
        Ok(pid)
    }

    fn handle(&self) -> Box<dyn Procs> {
        Box::new(System {
            proc_root: self.proc_root.clone(),
        })
    }

    fn children(&self, pid: u32) -> Vec<u32> {
        self.pids()
            .into_iter()
            .filter(|p| self.stat(*p).is_some_and(|(_, ppid, _)| ppid == pid))
            .collect()
    }

    fn alive(&self, pid: u32) -> bool {
        self.stat(pid)
            .is_some_and(|(state, _, _)| state != 'Z' && state != 'X')
    }

    fn signal(&self, pid: u32, sig: i32, group: bool) {
        let target = if group { -(pid as i32) } else { pid as i32 };
        if pid == 0 || pid == 1 {
            return;
        }
        // SAFETY: kill(2) with a pid or negative process group id; never 0, -1 or init.
        unsafe {
            libc::kill(target, sig);
        }
    }

    fn pgid(&self, pid: u32) -> Option<u32> {
        self.stat(pid).map(|(_, _, pgrp)| pgrp)
    }

    fn using(&self, program: &Path, config: &Path) -> Vec<u32> {
        // The monitor runs the real Syncthing with its resolved path, so the path may differ
        // from ours (a symlinked card, review M2): match the program's name and our exact
        // settings folder instead.
        let name = program
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let want_conf = format!("--config={}", config.to_string_lossy());
        self.pids()
            .into_iter()
            .filter(|p| {
                let Ok(raw) = std::fs::read(self.proc_root.join(p.to_string()).join("cmdline"))
                else {
                    return false;
                };
                let args: Vec<String> = raw
                    .split(|b| *b == 0)
                    .map(|a| String::from_utf8_lossy(a).into_owned())
                    .collect();
                args.first().is_some_and(|a| {
                    Path::new(a)
                        .file_name()
                        .is_some_and(|f| f.to_string_lossy() == name)
                }) && args.contains(&want_conf)
                    && self.alive(*p)
            })
            .collect()
    }

    fn run_with_input(
        &mut self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
        input: &str,
        limit: Duration,
    ) -> std::io::Result<(Option<i32>, String)> {
        use std::io::Read;
        use std::os::unix::process::CommandExt;
        let mut child = Command::new(program)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(o) = out.as_mut() {
                let _ = o.read_to_string(&mut s);
            }
            if let Some(e) = err.as_mut() {
                let _ = e.read_to_string(&mut s);
            }
            s
        });
        let until = Instant::now() + limit;
        let code = loop {
            if let Some(status) = child.try_wait()? {
                break status.code();
            }
            if Instant::now() >= until {
                // SAFETY: its own process group, made above.
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("did not finish within {} s", limit.as_secs()),
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        Ok((code, reader.join().unwrap_or_default()))
    }
}

/// The arguments for `syncthing serve` (approved: plan 3.4, 8.4 refinements 1 and 2).
pub fn serve_args(config: &Path, data: &Path, log: &Path) -> Vec<String> {
    vec![
        "serve".into(),
        format!("--config={}", config.to_string_lossy()),
        format!("--data={}", data.to_string_lossy()),
        "--no-browser".into(),
        "--no-upgrade".into(),
        "--no-restart".into(),
        "--no-port-probing".into(),
        format!("--log-file={}", log.to_string_lossy()),
        format!("--log-max-size={}", super::LOG_MAX_BYTES),
        format!("--log-max-old-files={}", super::LOG_OLD_FILES),
    ]
}

/// The environment for every Syncthing command: `HOME` inside our folder (Syncthing uses it
/// only to expand `~`), and upgrades off.
pub fn env(home: &Path) -> Vec<(String, String)> {
    vec![
        ("HOME".into(), home.to_string_lossy().into_owned()),
        ("STNOUPGRADE".into(), "1".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_serve_arguments_are_as_approved() {
        let a = serve_args(
            Path::new("/c/config"),
            Path::new("/c/data"),
            Path::new("/c/syncthing.log"),
        );
        assert_eq!(
            a,
            [
                "serve",
                "--config=/c/config",
                "--data=/c/data",
                "--no-browser",
                "--no-upgrade",
                "--no-restart",
                "--no-port-probing",
                "--log-file=/c/syncthing.log",
                "--log-max-size=1000000",
                "--log-max-old-files=1"
            ]
        );
    }

    #[test]
    fn stat_lines_with_odd_names_are_read() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("42");
        std::fs::create_dir(&p).unwrap();
        std::fs::write(
            p.join("stat"),
            "42 (sync thing (x)) S 7 42 42 0 -1 4194560 0 0 0 0 0 0 0 0 20 0\n",
        )
        .unwrap();
        let s = System {
            proc_root: d.path().to_path_buf(),
        };
        assert_eq!(s.stat(42), Some(('S', 7, 42)));
        assert_eq!(s.children(7), [42]);
        assert!(s.alive(42));
        std::fs::write(p.join("stat"), "42 (x) Z 7 42 42\n").unwrap();
        assert!(!s.alive(42), "a zombie has exited");
        assert!(!s.alive(43));
    }

    #[test]
    fn leftovers_are_found_by_program_and_settings_folder() {
        let d = tempfile::tempdir().unwrap();
        for (pid, cmd) in [
            (
                50,
                "/c/System/syncthing\0serve\0--config=/c/Config/syncthing/config\0",
            ),
            (51, "/c/System/syncthing\0serve\0--config=/other/config\0"),
            (
                52,
                "/usr/bin/syncthing\0serve\0--config=/c/Config/syncthing/config\0",
            ),
            (53, "/c/System/slot\0--config=/c/Config/syncthing/config\0"),
        ] {
            let p = d.path().join(pid.to_string());
            std::fs::create_dir(&p).unwrap();
            std::fs::write(p.join("cmdline"), cmd).unwrap();
            std::fs::write(
                p.join("stat"),
                format!("{pid} (syncthing) S 1 {pid} {pid}\n"),
            )
            .unwrap();
        }
        let s = System {
            proc_root: d.path().to_path_buf(),
        };
        let mut found = s.using(
            Path::new("/c/System/syncthing"),
            Path::new("/c/Config/syncthing/config"),
        );
        found.sort_unstable();
        assert_eq!(
            found,
            [50, 52],
            "the real Syncthing may run under its resolved path"
        );
    }

    #[test]
    fn a_real_child_and_its_exit_are_seen() {
        let mut s = System::new();
        let (tx, rx) = std::sync::mpsc::channel();
        let pid = s
            .spawn(
                Path::new("/bin/sh"),
                &["-c".into(), "exit 3".into()],
                &[],
                Box::new(move |_, e| {
                    let _ = tx.send(e);
                }),
            )
            .unwrap();
        assert!(pid > 1);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Exit::RESTART
        );
    }

    #[test]
    fn input_reaches_the_program_and_a_hang_is_bounded() {
        let mut s = System::new();
        let (code, out) = s
            .run_with_input(
                Path::new("/bin/sh"),
                &["-c".into(), "read x; echo got $x".into()],
                &[],
                "secret\n",
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(code, Some(0));
        assert_eq!(out.trim(), "got secret");
        let began = Instant::now();
        let e = s
            .run_with_input(
                Path::new("/bin/sh"),
                &["-c".into(), "sleep 30".into()],
                &[],
                "",
                Duration::from_millis(200),
            )
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        assert!(began.elapsed() < Duration::from_secs(3));
    }
}
