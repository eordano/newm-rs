use std::collections::HashMap;

use smithay::output::Output;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New,
};
use smithay::utils::{Logical, Point, Rectangle, Size};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use wayland_backend::server::ClientId;
use zkde_screencast_stream_unstable_v1::ZkdeScreencastStreamUnstableV1;
use zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1;

use super::raw::zkde_screencast::v1::server::{
    zkde_screencast_stream_unstable_v1, zkde_screencast_unstable_v1,
};
use crate::protocols::EmptyData;
use crate::utils::CastSessionId;

const VERSION: u32 = 5;

pub struct KdeScreencastManagerState {
    streams: HashMap<CastSessionId, ZkdeScreencastStreamUnstableV1>,
}

pub struct KdeScreencastGlobalData {
    filter: Box<dyn for<'c> Fn(&'c Client) -> bool + Send + Sync>,
}

pub struct KdeScreencastStreamData {
    session_id: CastSessionId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdePointer {
    Hidden,
    Embedded,
    Metadata,
}

#[derive(Debug)]
pub enum KdeCastSource {
    Output(Output),
    Window(String),
    Region {
        rect: Rectangle<i32, Logical>,
        scale: Option<f64>,
    },
}

pub trait KdeScreencastHandler {
    fn kde_screencast_state(&mut self) -> &mut KdeScreencastManagerState;
    fn start_kde_cast(
        &mut self,
        session_id: CastSessionId,
        source: KdeCastSource,
        pointer: KdePointer,
    );
    fn stop_kde_cast(&mut self, session_id: CastSessionId);
}

impl KdeScreencastManagerState {
    pub fn new<D, F>(display: &DisplayHandle, filter: F) -> Self
    where
        D: GlobalDispatch<ZkdeScreencastUnstableV1, KdeScreencastGlobalData>,
        D: 'static,
        F: for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static,
    {
        let global_data = KdeScreencastGlobalData {
            filter: Box::new(filter),
        };
        display.create_global::<D, ZkdeScreencastUnstableV1, _>(VERSION, global_data);

        Self {
            streams: HashMap::new(),
        }
    }

    pub fn stream(&self, session_id: CastSessionId) -> Option<ZkdeScreencastStreamUnstableV1> {
        self.streams.get(&session_id).cloned()
    }

    pub fn end(&mut self, session_id: CastSessionId, failure: Option<&str>) -> bool {
        let Some(stream) = self.streams.remove(&session_id) else {
            return false;
        };
        match failure {
            Some(error) => stream.failed(error.to_owned()),
            None => stream.closed(),
        }
        true
    }
}

impl KdePointer {
    fn from_wire(pointer: u32) -> Self {
        match pointer {
            2 => Self::Embedded,
            4 => Self::Metadata,
            _ => Self::Hidden,
        }
    }
}

impl<D> GlobalDispatch2<ZkdeScreencastUnstableV1, D> for KdeScreencastGlobalData
where
    D: Dispatch<ZkdeScreencastUnstableV1, EmptyData>,
    D: 'static,
{
    fn bind(
        &self,
        _state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        manager: New<ZkdeScreencastUnstableV1>,
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(manager, EmptyData);
    }

    fn can_view(&self, client: &Client) -> bool {
        (self.filter)(client)
    }
}

impl<D> Dispatch2<ZkdeScreencastUnstableV1, D> for EmptyData
where
    D: Dispatch<ZkdeScreencastStreamUnstableV1, KdeScreencastStreamData>,
    D: KdeScreencastHandler,
    D: 'static,
{
    fn request(
        &self,
        state: &mut D,
        _client: &Client,
        _manager: &ZkdeScreencastUnstableV1,
        request: zkde_screencast_unstable_v1::Request,
        _display: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        use zkde_screencast_unstable_v1::Request;

        let (stream, source, pointer) = match request {
            Request::StreamOutput {
                stream,
                output,
                pointer,
            } => (
                stream,
                Output::from_resource(&output).map(KdeCastSource::Output),
                pointer,
            ),
            Request::StreamWindow {
                stream,
                window_uuid,
                pointer,
            } => (stream, Some(KdeCastSource::Window(window_uuid)), pointer),
            Request::StreamRegion {
                stream,
                x,
                y,
                width,
                height,
                scale,
                pointer,
            } => (stream, region_source(x, y, width, height, scale), pointer),
            Request::StreamVirtualOutput { stream, .. }
            | Request::StreamVirtualOutputWithDescription { stream, .. } => {
                let session_id = CastSessionId::next();
                let stream = data_init.init(stream, KdeScreencastStreamData { session_id });
                stream.failed(String::from("newm-rs has no virtual outputs"));
                return;
            }
            Request::Destroy => return,
        };

        let session_id = CastSessionId::next();
        let stream = data_init.init(stream, KdeScreencastStreamData { session_id });
        let Some(source) = source else {
            stream.failed(String::from("the requested source does not exist"));
            return;
        };

        state
            .kde_screencast_state()
            .streams
            .insert(session_id, stream);
        state.start_kde_cast(session_id, source, KdePointer::from_wire(pointer));
    }
}

impl<D> Dispatch2<ZkdeScreencastStreamUnstableV1, D> for KdeScreencastStreamData
where
    D: KdeScreencastHandler,
    D: 'static,
{
    fn request(
        &self,
        _state: &mut D,
        _client: &Client,
        _stream: &ZkdeScreencastStreamUnstableV1,
        request: zkde_screencast_stream_unstable_v1::Request,
        _display: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            zkde_screencast_stream_unstable_v1::Request::Close => (),
        }
    }

    fn destroyed(
        &self,
        state: &mut D,
        _client: ClientId,
        _stream: &ZkdeScreencastStreamUnstableV1,
    ) {
        if state
            .kde_screencast_state()
            .streams
            .remove(&self.session_id)
            .is_some()
        {
            state.stop_kde_cast(self.session_id);
        }
    }
}

/// The farthest a region's corner may lie from the origin, and the largest side it may have, in
/// logical pixels: far past any layout of outputs, and far from i32's limits, which the cast's
/// geometry (corner minus output position, overflow-checked) would otherwise cross.
const REGION_REACH: i32 = 1 << 20;
const REGION_SIDE: u32 = 1 << 16;

/// The largest scale a region may ask for.
const REGION_SCALE: f64 = 16.;

/// What a `stream_region` request asks for: none for an empty region; a client's corner and size
/// held within [`REGION_REACH`] and [`REGION_SIDE`], and a scale only when it is a sane one.
fn region_source(x: i32, y: i32, width: u32, height: u32, scale: f64) -> Option<KdeCastSource> {
    let side = |n: u32| n.min(REGION_SIDE) as i32;
    let reach = |n: i32| n.clamp(-REGION_REACH, REGION_REACH);
    let rect = Rectangle::new(
        Point::from((reach(x), reach(y))),
        Size::from((side(width), side(height))),
    );
    (!rect.is_empty()).then_some(KdeCastSource::Region {
        rect,
        scale: (scale > 0. && scale <= REGION_SCALE).then_some(scale),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_stays_far_from_integer_limits() {
        let Some(KdeCastSource::Region { rect, scale }) =
            region_source(i32::MIN, i32::MAX, u32::MAX, 10, f64::INFINITY)
        else {
            panic!("a region");
        };
        assert_eq!(rect.loc, Point::from((-REGION_REACH, REGION_REACH)));
        assert_eq!(rect.size, Size::from((REGION_SIDE as i32, 10)));
        assert_eq!(scale, None);
        assert!(region_source(0, 0, 0, 10, 1.).is_none());
        assert!(matches!(
            region_source(10, 20, 30, 40, 2.),
            Some(KdeCastSource::Region {
                scale: Some(2.),
                ..
            })
        ));
    }
}
