//! wlr-output-power-management-v1: `wlopm --off '*'` / `--on '*'` and the swayidle configs written
//! for wlroots compositors. niri powers its monitors as one (`power-off-monitors`, input wakes
//! them), so an output's power follows that switch: turning any output off turns every monitor
//! off, turning any on turns them all on, and every power object hears its output's mode change,
//! whichever way it came.

use smithay::output::Output;
use smithay::reexports::wayland_protocols_wlr;
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use wayland_protocols_wlr::output_power_management::v1::server::{
    zwlr_output_power_manager_v1, zwlr_output_power_v1,
};
use zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1;
use zwlr_output_power_v1::ZwlrOutputPowerV1;

use crate::protocols::EmptyData;

const VERSION: u32 = 1;

pub struct OutputPowerManagerState {
    /// Live power objects with their output; one whose output went is failed and dropped.
    powers: Vec<(ZwlrOutputPowerV1, Output)>,
}

pub struct OutputPowerManagerGlobalData {
    filter: Box<dyn for<'c> Fn(&'c Client) -> bool + Send + Sync>,
}

pub trait OutputPowerHandler {
    fn output_power_manager_state(&mut self) -> &mut OutputPowerManagerState;
    /// Whether the output's monitor is on.
    fn output_power_on(&mut self, output: &Output) -> bool;
    /// Turns the output's monitor on or off.
    fn set_output_power(&mut self, output: &Output, on: bool);
}

fn mode(on: bool) -> zwlr_output_power_v1::Mode {
    if on {
        zwlr_output_power_v1::Mode::On
    } else {
        zwlr_output_power_v1::Mode::Off
    }
}

impl OutputPowerManagerState {
    pub fn new<D, F>(display: &DisplayHandle, filter: F) -> Self
    where
        D: GlobalDispatch<ZwlrOutputPowerManagerV1, OutputPowerManagerGlobalData>,
        D: OutputPowerHandler,
        D: 'static,
        F: for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static,
    {
        let global_data = OutputPowerManagerGlobalData {
            filter: Box::new(filter),
        };
        display.create_global::<D, ZwlrOutputPowerManagerV1, _>(VERSION, global_data);
        Self { powers: Vec::new() }
    }

    /// Tells every power object its output's mode: niri's monitors went on or off together.
    pub fn monitors_changed(&mut self, on: bool) {
        for (power, _) in &self.powers {
            power.mode(mode(on));
        }
    }

    pub fn output_removed(&mut self, output: &Output) {
        self.powers.retain(|(power, o)| {
            if o == output {
                power.failed();
                false
            } else {
                true
            }
        });
    }
}

impl<D> GlobalDispatch2<ZwlrOutputPowerManagerV1, D> for OutputPowerManagerGlobalData
where
    D: Dispatch<ZwlrOutputPowerManagerV1, EmptyData>,
    D: OutputPowerHandler,
    D: 'static,
{
    fn bind(
        &self,
        _state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        manager: New<ZwlrOutputPowerManagerV1>,
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(manager, EmptyData);
    }

    fn can_view(&self, client: &Client) -> bool {
        (self.filter)(client)
    }
}

impl<D> Dispatch2<ZwlrOutputPowerManagerV1, D> for EmptyData
where
    D: Dispatch<ZwlrOutputPowerV1, EmptyData>,
    D: OutputPowerHandler,
    D: 'static,
{
    fn request(
        &self,
        state: &mut D,
        _client: &Client,
        _resource: &ZwlrOutputPowerManagerV1,
        request: <ZwlrOutputPowerManagerV1 as Resource>::Request,
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            zwlr_output_power_manager_v1::Request::GetOutputPower { id, output } => {
                let power = data_init.init(id, EmptyData);
                match Output::from_resource(&output) {
                    Some(output) => {
                        let on = state.output_power_on(&output);
                        power.mode(mode(on));
                        state
                            .output_power_manager_state()
                            .powers
                            .push((power, output));
                    }
                    // The wl_output is gone: the object fails at once, as the protocol says.
                    None => power.failed(),
                }
            }
            zwlr_output_power_manager_v1::Request::Destroy => (),
            _ => unreachable!(),
        }
    }
}

impl<D> Dispatch2<ZwlrOutputPowerV1, D> for EmptyData
where
    D: OutputPowerHandler,
    D: 'static,
{
    fn request(
        &self,
        state: &mut D,
        _client: &Client,
        resource: &ZwlrOutputPowerV1,
        request: <ZwlrOutputPowerV1 as Resource>::Request,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            zwlr_output_power_v1::Request::SetMode { mode: asked } => {
                let output = state
                    .output_power_manager_state()
                    .powers
                    .iter()
                    .find(|(power, _)| power == resource)
                    .map(|(_, output)| output.clone());
                // A failed object (its output went) is ignored, as the protocol says.
                let Some(output) = output else {
                    return;
                };
                let on = match asked {
                    WEnum::Value(zwlr_output_power_v1::Mode::On) => true,
                    WEnum::Value(zwlr_output_power_v1::Mode::Off) => false,
                    _ => {
                        resource.post_error(
                            zwlr_output_power_v1::Error::InvalidMode,
                            "invalid power mode",
                        );
                        return;
                    }
                };
                if state.output_power_on(&output) == on {
                    // No change: the protocol still answers a set_mode with the mode.
                    resource.mode(mode(on));
                } else {
                    state.set_output_power(&output, on);
                }
            }
            zwlr_output_power_v1::Request::Destroy => (),
            _ => unreachable!(),
        }
    }

    fn destroyed(&self, state: &mut D, _client: ClientId, resource: &ZwlrOutputPowerV1) {
        state
            .output_power_manager_state()
            .powers
            .retain(|(power, _)| power != resource);
    }
}
