use crate::{Command, CommandError, Core, Effect, Event, Snapshot, TimeMs, Timeouts};

/// A backend starts/cancels effects without blocking and reports evidence via poll.
/// On a new generation it must cancel obsolete work before confirming cleanup.
pub trait Backend {
    fn stream_request(&self) -> crate::StreamRequest {
        crate::StreamRequest::default()
    }
    fn execute(&mut self, effect: Effect, now: TimeMs);
    fn poll(&mut self, now: TimeMs) -> Vec<Event>;
}

/// Both TUI and future desktop callers use this same runtime and snapshot contract.
pub struct Runtime<B: Backend> {
    core: Core,
    backend: B,
}

impl<B: Backend> Runtime<B> {
    pub fn new(backend: B) -> Self {
        Self::with_timeouts(backend, Timeouts::default())
    }
    pub fn with_timeouts(backend: B, timeouts: Timeouts) -> Self {
        Self {
            core: Core::with_timeouts(timeouts).with_stream_request(backend.stream_request()),
            backend,
        }
    }
    pub fn snapshot(&self) -> &Snapshot {
        self.core.snapshot()
    }
    pub fn with_host_mute(backend: B, timeouts: Timeouts, enabled: bool) -> Self {
        Self {
            core: Core::with_host_mute(timeouts, enabled)
                .with_stream_request(backend.stream_request()),
            backend,
        }
    }
    pub fn backend(&self) -> &B {
        &self.backend
    }
    pub fn command(&mut self, command: Command, now: TimeMs) -> Result<(), CommandError> {
        for effect in self.core.command(command, now)? {
            self.backend.execute(effect, now);
        }
        Ok(())
    }
    pub fn advance(&mut self, now: TimeMs) {
        // Deadline evaluation precedes feedback: evidence arriving after a timeout is stale.
        for effect in self.core.tick(now) {
            self.backend.execute(effect, now);
        }
        for event in self.backend.poll(now) {
            for effect in self.core.event(event, now) {
                self.backend.execute(effect, now);
            }
        }
    }
}
