//! The commands newm-mac spawns, reaped without blocking: every spawn and the poll timer sweep
//! them with `try_wait`, so a child that exits does not stay a zombie.

use std::process::{Child, Command};

#[derive(Default)]
pub struct Children(Vec<Child>);

impl Children {
    /// Runs `cmd` through `/bin/sh -c`.
    pub fn spawn(&mut self, cmd: &str) -> std::io::Result<()> {
        self.reap();
        self.0.push(Command::new("/bin/sh").arg("-c").arg(cmd).spawn()?);
        Ok(())
    }

    /// Reaps the children that have exited; returns how many still run.
    pub fn reap(&mut self) -> usize {
        self.0.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn exited_children_are_reaped_and_running_ones_kept() {
        let mut c = Children::default();
        c.spawn("exit 0").unwrap();
        c.spawn("exit 3").unwrap();
        c.spawn("sleep 0.5").unwrap();
        let pids: Vec<u32> = c.0.iter().map(Child::id).collect();
        let t0 = Instant::now();
        while c.reap() > 1 {
            assert!(t0.elapsed() < Duration::from_secs(10), "the exited children are reaped");
            std::thread::sleep(Duration::from_millis(10));
        }
        if t0.elapsed() < Duration::from_millis(400) {
            assert_eq!(c.0.len(), 1, "the sleeping child is still running");
        }
        while c.reap() > 0 {
            assert!(t0.elapsed() < Duration::from_secs(10), "the sleeping child is reaped");
            std::thread::sleep(Duration::from_millis(10));
        }
        if cfg!(target_os = "linux") {
            for pid in pids {
                assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists(), "pid {pid} is a zombie");
            }
        }
    }
}
