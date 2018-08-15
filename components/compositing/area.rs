/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use CompositionPipeline;
use SendableFrameTree;
use euclid::{TypedPoint2D, TypedVector2D, TypedScale};
use msg::constellation_msg::{PipelineId, PipelineIndex, PipelineNamespaceId};
use script_traits::{MouseEventType, ScrollState, TouchEventType, TouchId};
use script_traits::{UntrustedNodeAddress, WindowSizeData, WindowSizeType};
use script_traits::ConstellationMsg;
use script_traits::{MouseButton, EventResult};
use script_traits::CompositorEvent::{MouseMoveEvent, MouseButtonEvent, TouchEvent};
use servo_geometry::DeviceIndependentPixel;
use style_traits::{CSSPixel, DevicePixel, PinchZoomFactor};
use style_traits::cursor::CursorKind;
use style_traits::viewport::ViewportConstraints;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::rc::Rc;
use touch::{TouchHandler, TouchAction};
use webrender_api::{self, DeviceIntPoint, DevicePoint, HitTestFlags, HitTestResult};
use webrender_api::{LayoutVector2D, ScrollLocation};
use windowing::{AreaCoordinates, EmbedderCoordinates, MouseWindowEvent};
use libc::c_void;
use std::sync::mpsc::Sender;

// FIXME: removed zoom_action, zoom_time, in_scroll_transaction, frame_tree_id, FrameTreeId

// Default viewport constraints
const MAX_ZOOM: f32 = 8.0;
const MIN_ZOOM: f32 = 0.1;

/// One pixel in layer coordinate space.
///
/// This unit corresponds to a "pixel" in layer coordinate space, which after scaling and
/// transformation becomes a device pixel.
#[derive(Clone, Copy, Debug)]
enum LayerPixel {}

pub type ScrollStates = HashMap<PipelineId, Vec<ScrollState>>;

#[derive(Clone, Copy)]
struct ScrollZoomEvent {
    /// Change the pinch zoom level by this factor
    magnification: f32,
    /// Scroll by this offset, or to Start or End
    scroll_location: ScrollLocation,
    /// Apply changes to the frame at this location
    cursor: DeviceIntPoint,
    /// The number of OS events that have been coalesced together into this one event.
    event_count: u32,
}

trait ConvertPipelineIdFromWebRender {
    fn from_webrender(&self) -> PipelineId;
}

impl ConvertPipelineIdFromWebRender for webrender_api::PipelineId {
    fn from_webrender(&self) -> PipelineId {
        PipelineId {
            namespace_id: PipelineNamespaceId(self.0),
            index: PipelineIndex(NonZeroU32::new(self.1).expect("Webrender pipeline zero?")),
        }
    }
}

pub struct Area {
    /// FIXME
    coordinates: AreaCoordinates,
    embedder_coordinates: EmbedderCoordinates,

    constellation_chan: Sender<ConstellationMsg>,

    /// Whether a scroll is in progress; i.e. whether the user's fingers are down.
    scroll_in_progress: bool,

    /// Whether we're waiting on a recomposite after dispatching a scroll.
    waiting_for_results_of_scroll: bool,

    /// The root pipeline.
    pub root_pipeline: Option<CompositionPipeline>,

    /// The scene scale, to allow for zooming and high-resolution painting.
    scale: TypedScale<f32, LayerPixel, DevicePixel>,

    /// "Mobile-style" zoom that does not reflow the page.
    viewport_zoom: PinchZoomFactor,

    /// Viewport zoom constraints provided by @viewport.
    min_viewport_zoom: Option<PinchZoomFactor>,
    max_viewport_zoom: Option<PinchZoomFactor>,

    /// "Desktop-style" zoom that resizes the viewport to fit the window.
    page_zoom: TypedScale<f32, CSSPixel, DeviceIndependentPixel>,

    /// Touch input state machine
    touch_handler: TouchHandler,

    /// Pending scroll/zoom events.
    pending_scroll_zoom_events: Vec<ScrollZoomEvent>,

    /// FIXME
    webrender_api: Rc<webrender_api::RenderApi>,
 
    /// FIXME
    // FIXME: pub?
    pub webrender_document: webrender_api::DocumentId,
}

impl Area {
    pub fn new(constellation_chan: Sender<ConstellationMsg>,
               coordinates: AreaCoordinates,
               embedder_coordinates: EmbedderCoordinates,
               webrender_api: Rc<webrender_api::RenderApi>,
               layer: webrender_api::DocumentLayer) -> Area {

        println!("wr add_document {:?}", coordinates.size);
        let webrender_document = webrender_api.add_document(coordinates.size, layer);

        let mut area = Area {
            scroll_in_progress: false,
            waiting_for_results_of_scroll: false,
            root_pipeline: None,
            scale: TypedScale::new(1.0),
            viewport_zoom: PinchZoomFactor::new(1.0),
            min_viewport_zoom: None,
            max_viewport_zoom: None,
            page_zoom: TypedScale::new(1.0),
            touch_handler: TouchHandler::new(),
            pending_scroll_zoom_events: vec![],
            constellation_chan,
            coordinates,
            embedder_coordinates,
            webrender_api,
            webrender_document,
        };

        area.update_zoom_transform();
        area.send_window_size(WindowSizeType::Initial);
        area
    }

    pub fn get_id(&self) -> webrender_api::DocumentId {
        self.webrender_document
    }

    pub fn set_frame_tree(&mut self, frame_tree: &SendableFrameTree) -> Option<ScrollStates> {
        debug!("Setting the frame tree for pipeline {}", frame_tree.pipeline.id);

        self.root_pipeline = Some(frame_tree.pipeline.clone());

        let pipeline_id = frame_tree.pipeline.id.to_webrender();
        let mut txn = webrender_api::Transaction::new();
        txn.set_root_pipeline(pipeline_id);
        txn.generate_frame();
        println!("webrender_api.set_root_pipeline {:?}", pipeline_id);
        self.webrender_api.send_transaction(self.webrender_document, txn);

        self.send_window_size(WindowSizeType::Initial);
        Some(self.send_viewport_rects())
    }

    pub fn on_touch_event_processed(&mut self, result: EventResult) {
        self.touch_handler.on_event_processed(result);
    }

    pub fn on_scroll_event(&mut self,
                           delta: ScrollLocation,
                           cursor: DeviceIntPoint,
                           phase: TouchEventType) {
        match phase {
            TouchEventType::Move => self.on_scroll_window_event(delta, cursor),
            TouchEventType::Up | TouchEventType::Cancel => {
                self.on_scroll_end_window_event(delta, cursor);
            }
            TouchEventType::Down => {
                self.on_scroll_start_window_event(delta, cursor);
            }
        }
    }

    fn on_scroll_window_event(&mut self,
                              scroll_location: ScrollLocation,
                              cursor: DeviceIntPoint) {
        self.pending_scroll_zoom_events.push(ScrollZoomEvent {
            magnification: 1.0,
            scroll_location: scroll_location,
            cursor: cursor,
            event_count: 1,
        });
    }

    fn on_scroll_start_window_event(&mut self,
                                    scroll_location: ScrollLocation,
                                    cursor: DeviceIntPoint) {
        self.scroll_in_progress = true;
        self.pending_scroll_zoom_events.push(ScrollZoomEvent {
            magnification: 1.0,
            scroll_location: scroll_location,
            cursor: cursor,
            event_count: 1,
        });
    }

    fn on_scroll_end_window_event(&mut self,
                                  scroll_location: ScrollLocation,
                                  cursor: DeviceIntPoint) {
        self.scroll_in_progress = false;
        self.pending_scroll_zoom_events.push(ScrollZoomEvent {
            magnification: 1.0,
            scroll_location: scroll_location,
            cursor: cursor,
            event_count: 1,
        });
    }

    pub fn pinch_zoom_level(&self) -> f32 {
        self.viewport_zoom.get()
    }

    fn set_pinch_zoom_level(&mut self, mut zoom: f32) {
        if let Some(min) = self.min_viewport_zoom {
            zoom = f32::max(min.get(), zoom);
        }
        if let Some(max) = self.max_viewport_zoom {
            zoom = f32::min(max.get(), zoom);
        }
        self.viewport_zoom = PinchZoomFactor::new(zoom);
    }

    fn process_pending_scroll_events(&mut self) -> Option<ScrollStates> {
        let had_events = self.pending_scroll_zoom_events.len() > 0;

        // Batch up all scroll events into one, or else we'll do way too much painting.
        let mut last_combined_event: Option<ScrollZoomEvent> = None;
        for scroll_event in self.pending_scroll_zoom_events.drain(..) {
            let this_cursor = scroll_event.cursor;

            let this_delta = match scroll_event.scroll_location {
                ScrollLocation::Delta(delta) => delta,
                ScrollLocation::Start | ScrollLocation::End => {
                    // If this is an event which is scrolling to the start or end of the page,
                    // disregard other pending events and exit the loop.
                    last_combined_event = Some(scroll_event);
                    break;
                }
            };

            match &mut last_combined_event {
                last_combined_event @ &mut None => {
                    *last_combined_event = Some(ScrollZoomEvent {
                        magnification: scroll_event.magnification,
                        scroll_location: ScrollLocation::Delta(webrender_api::LayoutVector2D::from_untyped(
                                                               &this_delta.to_untyped())),
                        cursor: this_cursor,
                        event_count: 1,
                    })
                }
                &mut Some(ref mut last_combined_event) => {
                    // Mac OS X sometimes delivers scroll events out of vsync during a
                    // fling. This causes events to get bunched up occasionally, causing
                    // nasty-looking "pops". To mitigate this, during a fling we average
                    // deltas instead of summing them.
                    if let ScrollLocation::Delta(delta) = last_combined_event.scroll_location {
                        let old_event_count =
                            TypedScale::new(last_combined_event.event_count as f32);
                        last_combined_event.event_count += 1;
                        let new_event_count =
                            TypedScale::new(last_combined_event.event_count as f32);
                        last_combined_event.scroll_location = ScrollLocation::Delta(
                            (delta * old_event_count + this_delta) /
                            new_event_count);
                    }
                    last_combined_event.magnification *= scroll_event.magnification;
                }
            }
        }

        if let Some(combined_event) = last_combined_event {
            let scroll_location = match combined_event.scroll_location {
                ScrollLocation::Delta(delta) => {
                    let scaled_delta = (TypedVector2D::from_untyped(&delta.to_untyped()) / self.scale)
                                       .to_untyped();
                    let calculated_delta = webrender_api::LayoutVector2D::from_untyped(&scaled_delta);
                                           ScrollLocation::Delta(calculated_delta)
                },
                // Leave ScrollLocation unchanged if it is Start or End location.
                sl @ ScrollLocation::Start | sl @ ScrollLocation::End => sl,
            };
            let cursor = (combined_event.cursor.to_f32() / self.scale).to_untyped();
            let cursor = webrender_api::WorldPoint::from_untyped(&cursor);
            let mut txn = webrender_api::Transaction::new();
            txn.scroll(scroll_location, cursor);
            if combined_event.magnification != 1.0 {
                let old_zoom = self.pinch_zoom_level();
                self.set_pinch_zoom_level(old_zoom * combined_event.magnification);
                txn.set_pinch_zoom(webrender_api::ZoomFactor::new(self.pinch_zoom_level()));
            }
            txn.generate_frame();
            self.webrender_api.send_transaction(self.webrender_document, txn);
            self.waiting_for_results_of_scroll = true
        }

        if had_events {
            Some(self.send_viewport_rects())
        } else {
            None
        }
    }

    // FIXME: this is semantically weird
    pub fn composite_done(&mut self) {
        self.waiting_for_results_of_scroll = false;
    }

    pub fn constrain_viewport(&mut self, constraints: ViewportConstraints) {
        self.viewport_zoom = constraints.initial_zoom;
        self.min_viewport_zoom = constraints.min_zoom;
        self.max_viewport_zoom = constraints.max_zoom;
        self.update_zoom_transform();
    }

    // FIXME: name is a bit wrong
    fn send_viewport_rects(&self) -> ScrollStates {
        let mut scroll_states_per_pipeline = HashMap::new();
        for scroll_layer_state in self.webrender_api.get_scroll_node_state(self.webrender_document) {
            let scroll_state = ScrollState {
                scroll_id: scroll_layer_state.id,
                scroll_offset: scroll_layer_state.scroll_offset.to_untyped(),
            };

            scroll_states_per_pipeline
                .entry(scroll_layer_state.id.pipeline_id().from_webrender())
                .or_insert(vec![])
                .push(scroll_state);
        }

        scroll_states_per_pipeline
    }


    pub fn process_pending_scroll_events_if_needed(&mut self) -> Option<ScrollStates> {
        if !self.pending_scroll_zoom_events.is_empty() && !self.waiting_for_results_of_scroll {
            self.process_pending_scroll_events()
        } else {
            None
        }
    }

    fn update_zoom_transform(&mut self) {
        let scale = self.page_zoom * self.embedder_coordinates.hidpi_factor;
        self.scale = TypedScale::new(scale.get());
    }

    pub fn on_zoom_reset_window_event(&mut self) {
        self.page_zoom = TypedScale::new(1.0);
        self.update_zoom_transform();
        self.send_window_size(WindowSizeType::Resize);
        self.update_page_zoom_for_webrender();
    }

    pub fn on_zoom_window_event(&mut self, magnification: f32) {
        self.page_zoom = TypedScale::new((self.page_zoom.get() * magnification)
                                          .max(MIN_ZOOM).min(MAX_ZOOM));
        self.update_zoom_transform();
        self.send_window_size(WindowSizeType::Resize);
        self.update_page_zoom_for_webrender();
    }

    fn update_page_zoom_for_webrender(&mut self) {
        let page_zoom = webrender_api::ZoomFactor::new(self.page_zoom.get());

        let mut txn = webrender_api::Transaction::new();
        txn.set_page_zoom(page_zoom);
        println!("wr set_page_zoom: {:?}", page_zoom);
        self.webrender_api.send_transaction(self.webrender_document, txn);
    }

    // FIXME: reorder functions
    /// Simulate a pinch zoom
    pub fn on_pinch_zoom_window_event(&mut self, magnification: f32) {
        self.pending_scroll_zoom_events.push(ScrollZoomEvent {
            magnification: magnification,
            scroll_location: ScrollLocation::Delta(TypedVector2D::zero()), // TODO: Scroll to keep the center in view?
            cursor:  TypedPoint2D::new(-1, -1), // Make sure this hits the base layer.
            event_count: 1,
        });
    }

    fn send_window_size(&self, size_type: WindowSizeType) {
        let dppx = self.page_zoom * self.embedder_coordinates.hidpi_factor;

        println!("wr set_window_parameters {:?}, {:?}", self.embedder_coordinates.framebuffer, self.coordinates);
        self.webrender_api.set_window_parameters(self.webrender_document,
                                                 self.embedder_coordinates.framebuffer,
                                                 self.coordinates,
                                                 self.embedder_coordinates.hidpi_factor.get());

        let initial_viewport = self.coordinates.size.to_f32() / dppx;

        let data = WindowSizeData {
            device_pixel_ratio: dppx,
            initial_viewport: initial_viewport,
        };

        let top_level_browsing_context_id = self.root_pipeline.as_ref().map(|pipeline| {
            pipeline.top_level_browsing_context_id
        });

        let msg = ConstellationMsg::WindowSize(top_level_browsing_context_id, data, size_type);

        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending window resize to constellation failed ({}).", e);
        }
    }

    pub fn on_resize_window_event(&mut self, embedder_coordinates: EmbedderCoordinates) {
        let old_coords = self.embedder_coordinates;
        self.embedder_coordinates = embedder_coordinates;

        // A size change could also mean a resolution change.
        if self.embedder_coordinates.hidpi_factor != old_coords.hidpi_factor {
            self.update_zoom_transform();
        }

        // FIXME:
        // if self.embedder_coordinates.viewport == old_coords.viewport &&
        //    self.embedder_coordinates.framebuffer == old_coords.framebuffer {
        //     return;
        // }

        self.send_window_size(WindowSizeType::Resize);
    }

    pub fn dispatch_mouse_window_event_class(&mut self, mouse_window_event: MouseWindowEvent) {
        let point = match mouse_window_event {
            MouseWindowEvent::Click(_, p) => p,
            MouseWindowEvent::MouseDown(_, p) => p,
            MouseWindowEvent::MouseUp(_, p) => p,
        };

        let results = self.hit_test_at_point(point);
        let result = match results.items.first() {
            Some(result) => result,
            None => return,
        };

        let (button, event_type) = match mouse_window_event {
            MouseWindowEvent::Click(button, _) => (button, MouseEventType::Click),
            MouseWindowEvent::MouseDown(button, _) => (button, MouseEventType::MouseDown),
            MouseWindowEvent::MouseUp(button, _) => (button, MouseEventType::MouseUp),
        };

        let event_to_send = MouseButtonEvent(
            event_type,
            button,
            result.point_in_viewport.to_untyped(),
            Some(UntrustedNodeAddress(result.tag.0 as *const c_void)),
            Some(result.point_relative_to_item.to_untyped()),
        );

        let pipeline_id = PipelineId::from_webrender(result.pipeline);
        let msg = ConstellationMsg::ForwardEvent(pipeline_id, event_to_send);
        if let Err(e) = self.constellation_chan.send(msg) {
            warn!("Sending event to constellation failed ({}).", e);
        }
    }

    fn hit_test_at_point(&self, point: DevicePoint) -> HitTestResult {
        let dppx = self.page_zoom * self.embedder_coordinates.hidpi_factor;
        let scaled_point = (point / dppx).to_untyped();

        let world_cursor = webrender_api::WorldPoint::from_untyped(&scaled_point);
        self.webrender_api.hit_test(
            self.webrender_document,
            None,
            world_cursor,
            HitTestFlags::empty()
        )

    }

    pub fn dispatch_mouse_window_move_event_class(&mut self, cursor: DevicePoint) {
        let root_pipeline_id = match self.root_pipeline.as_ref().map(|pipeline| pipeline.id) {
            Some(root_pipeline_id) => root_pipeline_id,
            None => return,
        };
        // FIXME: could that ever happen?
        // if self.pipeline(root_pipeline_id).is_none() {
        //     return;
        // }

        let results = self.hit_test_at_point(cursor);
        if let Some(item) = results.items.first() {
            let node_address = Some(UntrustedNodeAddress(item.tag.0 as *const c_void));
            let event = MouseMoveEvent(Some(item.point_in_viewport.to_untyped()), node_address);
            let pipeline_id = PipelineId::from_webrender(item.pipeline);
            let msg = ConstellationMsg::ForwardEvent(pipeline_id, event);
            if let Err(e) = self.constellation_chan.send(msg) {
                warn!("Sending event to constellation failed ({}).", e);
            }

            if let Some(cursor) =  CursorKind::from_u8(item.tag.1 as _).ok() {
                let msg = ConstellationMsg::SetCursor(cursor);
                if let Err(e) = self.constellation_chan.send(msg) {
                    warn!("Sending event to constellation failed ({}).", e);
                }
            }
        }
    }

    fn send_touch_event(
        &self,
        event_type: TouchEventType,
        identifier: TouchId,
        point: DevicePoint)
    {
        let results = self.hit_test_at_point(point);
        if let Some(item) = results.items.first() {
            let event = TouchEvent(
                event_type,
                identifier,
                item.point_in_viewport.to_untyped(),
                Some(UntrustedNodeAddress(item.tag.0 as *const c_void)),
            );
            let pipeline_id = PipelineId::from_webrender(item.pipeline);
            let msg = ConstellationMsg::ForwardEvent(pipeline_id, event);
            if let Err(e) = self.constellation_chan.send(msg) {
                warn!("Sending event to constellation failed ({}).", e);
            }
        }
    }

    pub fn on_touch_event(&mut self,
                          event_type: TouchEventType,
                          identifier: TouchId,
                          location: DevicePoint) {
        match event_type {
            TouchEventType::Down => self.on_touch_down(identifier, location),
            TouchEventType::Move => self.on_touch_move(identifier, location),
            TouchEventType::Up => self.on_touch_up(identifier, location),
            TouchEventType::Cancel => self.on_touch_cancel(identifier, location),
        }
    }

    pub fn on_touch_down(&mut self, identifier: TouchId, point: DevicePoint) {
        self.touch_handler.on_touch_down(identifier, point);
        self.send_touch_event(TouchEventType::Down, identifier, point);
    }

    pub fn on_touch_move(&mut self, identifier: TouchId, point: DevicePoint) {
        match self.touch_handler.on_touch_move(identifier, point) {
            TouchAction::Scroll(delta) => {
                self.on_scroll_window_event(
                    ScrollLocation::Delta(
                        LayoutVector2D::from_untyped(&delta.to_untyped())
                    ),
                    point.cast()
                )
            }
            TouchAction::Zoom(magnification, scroll_delta) => {
                let cursor = TypedPoint2D::new(-1, -1);  // Make sure this hits the base layer.
                self.pending_scroll_zoom_events.push(ScrollZoomEvent {
                    magnification: magnification,
                    scroll_location: ScrollLocation::Delta(webrender_api::LayoutVector2D::from_untyped(
                                                           &scroll_delta.to_untyped())),
                    cursor: cursor,
                    event_count: 1,
                });
            }
            TouchAction::DispatchEvent => {
                self.send_touch_event(TouchEventType::Move, identifier, point);
            }
            _ => {}
        }
    }

    pub fn generate_frame(&self) {
        let mut txn = webrender_api::Transaction::new();
        txn.generate_frame();
        self.webrender_api.send_transaction(self.webrender_document, txn);
    }

    pub fn on_touch_up(&mut self, identifier: TouchId, point: DevicePoint) {
        self.send_touch_event(TouchEventType::Up, identifier, point);

        if let TouchAction::Click = self.touch_handler.on_touch_up(identifier, point) {
            self.simulate_mouse_click(point);
        }
    }

    /// <http://w3c.github.io/touch-events/#mouse-events>
    fn simulate_mouse_click(&mut self, p: DevicePoint) {
        let button = MouseButton::Left;
        self.dispatch_mouse_window_move_event_class(p);
        self.dispatch_mouse_window_event_class(MouseWindowEvent::MouseDown(button, p));
        self.dispatch_mouse_window_event_class(MouseWindowEvent::MouseUp(button, p));
        self.dispatch_mouse_window_event_class(MouseWindowEvent::Click(button, p));
    }

    fn on_touch_cancel(&mut self, identifier: TouchId, point: DevicePoint) {
        // Send the event to script.
        self.touch_handler.on_touch_cancel(identifier, point);
        self.send_touch_event(TouchEventType::Cancel, identifier, point);
    }

}
