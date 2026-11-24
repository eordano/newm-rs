use fixture::Fixture;

mod client;
mod fixture;
mod server;
mod test_input_backend;

mod animations;
mod binds;
#[cfg(feature = "canvas")]
mod blocks;
mod fake_input;
mod floating;
mod fullscreen;
mod layer_shell;
#[cfg(feature = "canvas")]
mod live_options;
#[cfg(feature = "canvas")]
mod mod_pan;
#[cfg(feature = "canvas")]
mod pinch;
mod remove_output;
#[cfg(feature = "synth")]
mod synth;
mod transactions;
mod virtual_pointer;
mod window_opening;
