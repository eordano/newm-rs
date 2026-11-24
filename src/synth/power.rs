//! Whether the machine runs on battery, from the kernel's power supplies.
//!
//! The state is read from `/sys/class/power_supply` once when the watch opens and again on each
//! udev `power_supply` event, which the kernel sends when an adapter comes or goes (and on
//! battery level changes, which re-read the same few files). Nothing polls.

use std::path::Path;

use newm_synth::power::{on_battery, SYSFS};
use smithay::reexports::calloop::{LoopHandle, RegistrationToken};

use crate::niri::State;

/// The watch: its event source and the last state read.
#[derive(Debug)]
pub struct Watch {
    pub on_battery: bool,
    /// Removes the udev source from the event loop on drop; `None` in tests.
    source: Option<(LoopHandle<'static, State>, RegistrationToken)>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some((event_loop, token)) = self.source.take() {
            event_loop.remove(token);
        }
    }
}

impl Watch {
    /// Opens the udev monitor and reads the state once.
    #[cfg(not(test))]
    pub fn open(event_loop: &LoopHandle<'static, State>) -> anyhow::Result<Self> {
        use smithay::reexports::calloop::generic::Generic;
        use smithay::reexports::calloop::{Interest, Mode, PostAction};
        use smithay::reexports::udev::MonitorBuilder;

        let socket = MonitorBuilder::new()?
            .match_subsystem("power_supply")?
            .listen()?;
        let source = Generic::new(socket, Interest::READ, Mode::Level);
        let token = event_loop
            .insert_source(source, |_, socket, state| {
                // Drain the events; what changed is read back from sysfs.
                for _ in socket.iter() {}
                state.niri.synth_power_changed();
                Ok(PostAction::Continue)
            })
            .map_err(|e| anyhow::anyhow!("inserting the udev source: {e}"))?;
        Ok(Self {
            on_battery: on_battery(Path::new(SYSFS)),
            source: Some((event_loop.clone(), token)),
        })
    }

    /// In tests: a stand-in on mains, so no test reads the machine's supplies.
    #[cfg(test)]
    pub fn open(_event_loop: &LoopHandle<'static, State>) -> anyhow::Result<Self> {
        Ok(Self::fake(false))
    }

    #[cfg(test)]
    pub fn fake(on_battery: bool) -> Self {
        Self {
            on_battery,
            source: None,
        }
    }

    /// Reads the state again.
    pub fn read(&mut self) {
        if self.source.is_some() {
            self.on_battery = on_battery(Path::new(SYSFS));
        }
    }
}
