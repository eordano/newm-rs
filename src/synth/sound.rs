//! Listening to the sound playing, for the synth background: one PipeWire capture stream on the
//! default sink's monitor, mono F32, made only while the background listens.
//!
//! The stream is **passive** (`node.passive = true`), as pipewire-pulse makes a stream that asks
//! not to inhibit auto-suspend (pavucontrol's level meters): its link to the sink is passive, so
//! it never makes the sink run. While something plays, the sink runs and the stream gets its
//! blocks; in silence the sink goes idle, WirePlumber suspends it after its timeout, and the
//! stream gets no callbacks at all. PipeWire's main loop runs inside calloop, as for screencasts,
//! so blocks arrive on the compositor's thread without a thread of their own.

use std::cell::Cell;
use std::io::Cursor;
use std::os::fd::{AsFd, BorrowedFd};
use std::rc::Rc;

use anyhow::Context as _;
use newm_synth::level_of_rms;
use pipewire::context::ContextRc;
use pipewire::core::CoreRc;
use pipewire::loop_::Timeout;
use pipewire::main_loop::MainLoopRc;
use pipewire::properties::PropertiesBox;
use pipewire::spa;
use pipewire::spa::pod::Pod;
use pipewire::stream::{StreamFlags, StreamListener, StreamRc};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction, RegistrationToken};

use crate::niri::State;

#[cfg_attr(test, allow(dead_code))]
/// Frames per block asked for (about 23 blocks a second at 48 kHz); the graph's own quantum
/// wins when it is smaller.
const LATENCY: &str = "2048/48000";

/// The open capture. Dropping it disconnects the stream and leaves PipeWire.
pub struct Sound {
    stream: StreamRc,
    _listener: StreamListener<()>,
    _core: CoreRc,
    _context: ContextRc,
    token: RegistrationToken,
    event_loop: LoopHandle<'static, State>,
}

impl std::fmt::Debug for Sound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sound")
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        let _ = self.stream.disconnect();
        self.event_loop.remove(self.token);
    }
}

/// The stream's properties: a passive capture of the default sink's monitor.
pub fn properties() -> [(&'static str, &'static str); 8] {
    [
        ("media.type", "Audio"),
        ("media.category", "Capture"),
        ("media.role", "Music"),
        ("stream.capture.sink", "true"),
        ("node.passive", "true"),
        ("node.latency", LATENCY),
        ("node.name", "newm-synth"),
        ("application.name", "newm synth background"),
    ]
}

/// Opens the capture (tests use a stand-in instead). Each block's level goes into `peak` (the
/// loudest since the compositor last took it); the compositor's refresh after the dispatch picks it
/// up.
#[cfg_attr(test, allow(dead_code))]
pub fn open(event_loop: LoopHandle<'static, State>, peak: Rc<Cell<f32>>) -> anyhow::Result<Sound> {
    let main_loop = MainLoopRc::new(None).context("error creating MainLoop")?;
    let context = ContextRc::new(&main_loop, None).context("error creating Context")?;
    let core = context
        .connect_rc(None)
        .context("error connecting to PipeWire")?;

    let mut props = PropertiesBox::new();
    for (k, v) in properties() {
        props.insert(k, v);
    }
    let stream =
        StreamRc::new(core.clone(), "newm-synth", props).context("error creating Stream")?;

    let listener = stream
        .add_local_listener_with_user_data(())
        .process(move |stream, ()| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let (offset, size) = (data.chunk().offset() as usize, data.chunk().size() as usize);
            let Some(bytes) = data.data() else {
                return;
            };
            let Some(bytes) = bytes.get(offset..offset + size) else {
                return;
            };
            // F32LE mono, aligned as PipeWire maps it; read through bytes to stay safe.
            let mut sum = 0f32;
            let mut n = 0usize;
            for s in bytes.chunks_exact(4) {
                let v = f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                sum += v * v;
                n += 1;
            }
            if n > 0 {
                let level = level_of_rms((sum / n as f32).sqrt());
                peak.set(peak.get().max(level));
            }
        })
        .register()
        .context("error registering the stream listener")?;

    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_channels(1);
    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    let bytes = spa::pod::serialize::PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .context("error serializing the audio format")?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&bytes).context("error reading the audio format")?];
    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .context("error connecting the stream")?;

    struct AsFdWrapper(MainLoopRc);
    impl AsFd for AsFdWrapper {
        fn as_fd(&self) -> BorrowedFd<'_> {
            self.0.loop_().fd()
        }
    }
    let generic = Generic::new(AsFdWrapper(main_loop), Interest::READ, Mode::Level);
    let token = event_loop
        .insert_source(generic, move |_, wrapper, _| {
            let _span = tracy_client::span!("synth sound iteration");
            wrapper.0.loop_().iterate(Timeout::None);
            Ok(PostAction::Continue)
        })
        .map_err(|e| anyhow::anyhow!("error adding PipeWire to the event loop: {e}"))?;

    Ok(Sound {
        stream,
        _listener: listener,
        _core: core,
        _context: context,
        token,
        event_loop,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stream_is_a_passive_capture_of_the_sink_monitor() {
        let props: std::collections::HashMap<_, _> = properties().into_iter().collect();
        // Passive: its link never makes the sink run, so a silent sink can suspend.
        assert_eq!(props["node.passive"], "true");
        // The default sink's monitor, following the default as it changes.
        assert_eq!(props["stream.capture.sink"], "true");
        assert_eq!(props["media.category"], "Capture");
        assert_eq!(props["media.type"], "Audio");
        assert!(!props.contains_key("target.object"));
    }
}
