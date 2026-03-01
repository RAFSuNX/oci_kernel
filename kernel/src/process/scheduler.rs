extern crate alloc;
use alloc::vec::Vec;
use super::{ProcessId, ProcessState};

pub struct Scheduler {
    queue:   Vec<(ProcessId, ProcessState)>,
    current: usize,
}

impl Scheduler {
    pub fn new() -> Self {
        Self { queue: Vec::new(), current: 0 }
    }

    pub fn add(&mut self, pid: ProcessId, state: ProcessState) {
        self.queue.push((pid, state));
    }

    pub fn next(&mut self) -> Option<ProcessId> {
        let len = self.queue.len();
        if len == 0 { return None; }
        for _ in 0..len {
            self.current = (self.current + 1) % len;
            let (pid, state) = self.queue[self.current];
            if state == ProcessState::Ready || state == ProcessState::Running {
                return Some(pid);
            }
        }
        None
    }

    pub fn set_state(&mut self, pid: ProcessId, state: ProcessState) {
        if let Some(entry) = self.queue.iter_mut().find(|(p, _)| *p == pid) {
            entry.1 = state;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessState;

    #[test]
    fn round_robin_cycles() {
        let mut sched = Scheduler::new();
        let p1 = ProcessId(1);
        let p2 = ProcessId(2);
        sched.add(p1, ProcessState::Ready);
        sched.add(p2, ProcessState::Ready);

        assert_eq!(sched.next(), Some(p1));
        assert_eq!(sched.next(), Some(p2));
        assert_eq!(sched.next(), Some(p1));
    }

    #[test]
    fn blocked_process_skipped() {
        let mut sched = Scheduler::new();
        sched.add(ProcessId(1), ProcessState::Blocked);
        sched.add(ProcessId(2), ProcessState::Ready);
        assert_eq!(sched.next(), Some(ProcessId(2)));
    }
}
