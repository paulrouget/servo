/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use area::{Area, ScrollStates};
use SendableFrameTree;
use compositor_thread::{CompositorProxy, CompositorReceiver};
use compositor_thread::{InitialCompositorState, Msg};
use gfx_traits::Epoch;
#[cfg(feature = "gleam")]
use gl;
#[cfg(feature = "gleam")]
use image::{DynamicImage, ImageFormat};
use ipc_channel::ipc;
use msg::constellation_msg::PipelineId;
use net_traits::image::base::Image;
#[cfg(feature = "gleam")]
use net_traits::image::base::PixelFormat;
use profile_traits::time::{self, ProfilerCategory, profile};
use script_traits::{AnimationState, AnimationTickType, ConstellationMsg, LayoutControlMsg};
use script_traits::{TouchEventType, TouchId};
use servo_config::opts;
use std::collections::HashMap;
use std::env;
use std::fs::{File, create_dir_all};
use std::io::Write;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use time::{now, precise_time_ns};
use webrender;
use webrender_api::{self, DeviceIntPoint, DevicePoint, DocumentId};
use webrender_api::ScrollLocation;
use windowing::{self, AreaCoordinates, EmbedderCoordinates, MouseWindowEvent, WebRenderDebugOption, WindowMethods};
use CompositionPipeline;



#[derive(Debug, PartialEq)]
enum UnableToComposite {
    WindowUnprepared,
    NotReadyToPaintImage(NotReadyToPaint),
}

#[derive(Debug, PartialEq)]
enum NotReadyToPaint {
    AnimationsActive,
    JustNotifiedConstellation,
    WaitingOnConstellation,
}

/// Holds the state when running reftests that determines when it is
/// safe to save the output image.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ReadyState {
    Unknown,
    WaitingForConstellationReply,
    ReadyToSaveImage,
}

/// NB: Never block on the constellation, because sometimes the constellation blocks on us.
pub struct IOCompositor<Window: WindowMethods> {
    /// The application window.
    pub window: Rc<Window>,

    /// The port on which we receive messages.
    port: CompositorReceiver,

    embedder_coordinates: EmbedderCoordinates,

    /// Tracks details about each active pipeline that the compositor knows about.
    pipeline_details: HashMap<PipelineId, PipelineDetails>,

    /// The type of composition to perform
    composite_target: CompositeTarget,

    /// Tracks whether we should composite this frame.
    composition_request: CompositionRequest,

    /// Tracks whether we are in the process of shutting down, or have shut down and should close
    /// the compositor.
    pub shutdown_state: ShutdownState,

    /// Tracks the last composite time.
    last_composite_time: u64,

    /// The channel on which messages can be sent to the constellation.
    constellation_chan: Sender<ConstellationMsg>,

    /// The channel on which messages can be sent to the time profiler.
    time_profiler_chan: time::ProfilerChan,

    /// Used by the logic that determines when it is safe to output an
    /// image for the reftest framework.
    ready_to_save_state: ReadyState,

    /// The webrender renderer.
    webrender: webrender::Renderer,

    /// The webrender interface, if enabled.
    webrender_api: Rc<webrender_api::RenderApi>,

    /// Map of the pending paint metrics per layout thread.
    /// The layout thread for each specific pipeline expects the compositor to
    /// paint frames with specific given IDs (epoch). Once the compositor paints
    /// these frames, it records the paint time for each of them and sends the
    /// metric to the corresponding layout thread.
    pending_paint_metrics: HashMap<PipelineId, Epoch>,

    /// FIXME
    areas: Vec<Area>,
}

#[derive(Debug, PartialEq)]
enum CompositionRequest {
    NoCompositingNecessary,
    CompositeNow(CompositingReason),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShutdownState {
    NotShuttingDown,
    ShuttingDown,
    FinishedShuttingDown,
}

struct PipelineDetails {
    /// The pipeline associated with this PipelineDetails object.
    pipeline: Option<CompositionPipeline>,

    /// Whether animations are running
    animations_running: bool,

    /// Whether there are animation callbacks
    animation_callbacks_running: bool,

    /// Whether this pipeline is visible
    visible: bool,
}

impl PipelineDetails {
    fn new() -> PipelineDetails {
        PipelineDetails {
            pipeline: None,
            animations_running: false,
            animation_callbacks_running: false,
            visible: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CompositeTarget {
    /// Normal composition to a window
    Window,

    /// Compose as normal, but also return a PNG of the composed output
    WindowAndPng,

    /// Compose to a PNG, write it to disk, and then exit the browser (used for reftests)
    PngFile
}

#[derive(Clone)]
pub struct RenderNotifier {
    compositor_proxy: CompositorProxy,
}

impl RenderNotifier {
    pub fn new(compositor_proxy: CompositorProxy) -> RenderNotifier {
        RenderNotifier {
            compositor_proxy: compositor_proxy,
        }
    }
}

impl webrender_api::RenderNotifier for RenderNotifier {
    fn clone(&self) -> Box<webrender_api::RenderNotifier> {
        Box::new(RenderNotifier::new(self.compositor_proxy.clone()))
    }

    fn wake_up(&self) {
        self.compositor_proxy.recomposite(CompositingReason::NewWebRenderFrame);
    }

    fn new_frame_ready(
        &self,
        document_id: webrender_api::DocumentId,
        scrolled: bool,
        composite_needed: bool,
        _render_time_ns: Option<u64>,
    ) {
        if scrolled {
            self.compositor_proxy.send(Msg::NewScrollFrameReady(document_id, composite_needed));
        } else {
            self.wake_up();
        }
    }
}

impl<Window: WindowMethods> IOCompositor<Window> {
    pub fn new(window: Rc<Window>, state: InitialCompositorState) -> Self {
        let composite_target = match opts::get().output_file {
            Some(_) => CompositeTarget::PngFile,
            None => CompositeTarget::Window
        };

        let coordinates = window.get_coordinates();

        IOCompositor {
            window,
            port: state.receiver,
            pipeline_details: HashMap::new(),
            composition_request: CompositionRequest::NoCompositingNecessary,
            composite_target,
            embedder_coordinates: coordinates,
            shutdown_state: ShutdownState::NotShuttingDown,
            constellation_chan: state.constellation_chan,
            time_profiler_chan: state.time_profiler_chan,
            last_composite_time: 0,
            ready_to_save_state: ReadyState::Unknown,
            webrender: state.webrender,
            webrender_api: Rc::new(state.webrender_api),
            pending_paint_metrics: HashMap::new(),
            areas: Vec::new(),
        }
    }

    pub fn create_area(&mut self, coords: AreaCoordinates, layer: webrender_api::DocumentLayer) -> webrender_api::DocumentId {
        let area = Area::new(
            self.constellation_chan.clone(),
            coords,
            self.embedder_coordinates.clone(),
            self.webrender_api.clone(),
            layer);
        let id = area.get_id();
        self.areas.push(area);
        id
    }

    pub fn deinit(self) {
        self.webrender.deinit();
    }

    pub fn maybe_start_shutting_down(&mut self) {
        if self.shutdown_state == ShutdownState::NotShuttingDown {
            debug!("Shutting down the constellation for WindowEvent::Quit");
            self.start_shutting_down();
        }
    }

    fn start_shutting_down(&mut self) {
        debug!("Compositor sending Exit message to Constellation");
        if let Err(e) = self.constellation_chan.send(ConstellationMsg::Exit) {
            warn!("Sending exit message to constellation failed ({}).", e);
        }

        self.shutdown_state = ShutdownState::ShuttingDown;
    }

    fn finish_shutting_down(&mut self) {
        debug!("Compositor received message that constellation shutdown is complete");

        // Drain compositor port, sometimes messages contain channels that are blocking
        // another thread from finishing (i.e. SetFrameTree).
        while self.port.try_recv_compositor_msg().is_some() {}

        // Tell the profiler, memory profiler, and scrolling timer to shut down.
        if let Ok((sender, receiver)) = ipc::channel() {
            self.time_profiler_chan.send(time::ProfilerMsg::Exit(sender));
            let _ = receiver.recv();
        }

        self.shutdown_state = ShutdownState::FinishedShuttingDown;
    }

    fn get_area_for_wrdoc(&mut self, wrdoc: DocumentId) -> Option<&mut Area> {
        let area = self.areas.iter_mut().find(|area| area.get_id() == wrdoc);
        if area.is_none() {
            warn!("Can't find area for DocumentId {:?}", wrdoc);
        }
        area
    }

    fn handle_browser_message(&mut self, msg: Msg) -> bool {
        match (msg, self.shutdown_state) {
            (_, ShutdownState::FinishedShuttingDown) => {
                error!("compositor shouldn't be handling messages after shutting down");
                return false
            }

            (Msg::ShutdownComplete, _) => {
                self.finish_shutting_down();
                return false;
            }

            (Msg::ChangeRunningAnimationsState(pipeline_id, animation_state),
             ShutdownState::NotShuttingDown) => {
                self.change_running_animations_state(pipeline_id, animation_state);
            }

            (Msg::SetFrameTree(wrdoc, frame_tree), ShutdownState::NotShuttingDown) => {
                self.create_pipeline_details_for_frame_tree(&frame_tree);

                if let Some(scroll_states) = self.get_area_for_wrdoc(wrdoc).and_then(|area| area.set_frame_tree(&frame_tree)) {
                    self.update_scroll_states(scroll_states);
                }
            }

            (Msg::Recomposite(reason), ShutdownState::NotShuttingDown) => {
                self.composition_request = CompositionRequest::CompositeNow(reason)
            }


            (Msg::TouchEventProcessed(pipeline_id, result), ShutdownState::NotShuttingDown) => {
                let id = self.pipeline(pipeline_id).map(|p| p.id);
                if let Some(id) = id {
                    if let Some(area) = self.areas.iter_mut().find(|area| {
                        area.root_pipeline.as_ref().map(|root| root.id == id).unwrap_or(false)
                    }) {
                        area.on_touch_event_processed(result);
                    } else {
                        warn!("Can't find area for pipeline");
                    }
                } 
            }

            (Msg::CreatePng(reply), ShutdownState::NotShuttingDown) => {
                let res = self.composite_specific_target(CompositeTarget::WindowAndPng);
                if let Err(ref e) = res {
                    info!("Error retrieving PNG: {:?}", e);
                }
                let img = res.unwrap_or(None);
                if let Err(e) = reply.send(img) {
                    warn!("Sending reply to create png failed ({}).", e);
                }
            }

            (Msg::ViewportConstrained(pipeline_id, constraints),
             ShutdownState::NotShuttingDown) => {
                let id = self.pipeline(pipeline_id).map(|p| p.id);
                if let Some(id) = id {
                    if let Some(area) = self.areas.iter_mut().find(|area| {
                        area.root_pipeline.as_ref().map(|root| root.id == id).unwrap_or(false)
                    }) {
                        area.constrain_viewport(constraints);
                    }
                }
            }

            (Msg::IsReadyToSaveImageReply(is_ready), ShutdownState::NotShuttingDown) => {
                assert_eq!(self.ready_to_save_state, ReadyState::WaitingForConstellationReply);
                if is_ready {
                    self.ready_to_save_state = ReadyState::ReadyToSaveImage;
                    if opts::get().is_running_problem_test {
                        println!("ready to save image!");
                    }
                } else {
                    self.ready_to_save_state = ReadyState::Unknown;
                    if opts::get().is_running_problem_test {
                        println!("resetting ready_to_save_state!");
                    }
                }
                self.composite_if_necessary(CompositingReason::Headless);
            }

            (Msg::PipelineVisibilityChanged(pipeline_id, visible), ShutdownState::NotShuttingDown) => {
                self.pipeline_details(pipeline_id).visible = visible;
                if visible {
                    self.process_animations();
                }
            }

            (Msg::PipelineExited(pipeline_id, sender), _) => {
                debug!("Compositor got pipeline exited: {:?}", pipeline_id);
                self.remove_pipeline_root_layer(pipeline_id);
                let _ = sender.send(());
            }

            (Msg::NewScrollFrameReady(wrdoc, recomposite_needed), ShutdownState::NotShuttingDown) => {
                if let Some(area) = self.get_area_for_wrdoc(wrdoc) {
                    // FIXME: is it semantically correct?
                    area.composite_done();
                }
                // FIXME: should that be in Area
                if recomposite_needed {
                    self.composition_request = CompositionRequest::CompositeNow(
                        CompositingReason::NewWebRenderScrollFrame);
                }
            }

            (Msg::Dispatch(func), ShutdownState::NotShuttingDown) => {
                // The functions sent here right now are really dumb, so they can't panic.
                // But if we start running more complex code here, we should really catch panic here.
                func();
            }

            (Msg::LoadComplete(_), ShutdownState::NotShuttingDown) => {
                // If we're painting in headless mode, schedule a recomposite.
                if opts::get().output_file.is_some() || opts::get().exit_after_load {
                    self.composite_if_necessary(CompositingReason::Headless);
                }
            },

            (Msg::PendingPaintMetric(pipeline_id, epoch), _) => {
                self.pending_paint_metrics.insert(pipeline_id, epoch);
            }

            (Msg::GetClientWindow(req), ShutdownState::NotShuttingDown) => {
                if let Err(e) = req.send(self.embedder_coordinates.window) {
                    warn!("Sending response to get client window failed ({}).", e);
                }
            }

            (Msg::GetScreenSize(req), ShutdownState::NotShuttingDown) => {
                if let Err(e) = req.send(self.embedder_coordinates.screen) {
                    warn!("Sending response to get screen size failed ({}).", e);
                }
            }

            (Msg::GetScreenAvailSize(req), ShutdownState::NotShuttingDown) => {
                if let Err(e) = req.send(self.embedder_coordinates.screen_avail) {
                    warn!("Sending response to get screen avail size failed ({}).", e);
                }
            }

            // When we are shutting_down, we need to avoid performing operations
            // such as Paint that may crash because we have begun tearing down
            // the rest of our resources.
            (_, ShutdownState::ShuttingDown) => {}
        }

        true
    }

    /// Sets or unsets the animations-running flag for the given pipeline, and schedules a
    /// recomposite if necessary.
    fn change_running_animations_state(
        &mut self,
        pipeline_id: PipelineId,
        animation_state: AnimationState,
    ) {
        match animation_state {
            AnimationState::AnimationsPresent => {
                let visible = self.pipeline_details(pipeline_id).visible;
                self.pipeline_details(pipeline_id).animations_running = true;
                if visible {
                    self.composite_if_necessary(CompositingReason::Animation);
                }
            }
            AnimationState::AnimationCallbacksPresent => {
                let visible = self.pipeline_details(pipeline_id).visible;
                self.pipeline_details(pipeline_id).animation_callbacks_running = true;
                if visible {
                    self.tick_animations_for_pipeline(pipeline_id);
                }
            }
            AnimationState::NoAnimationsPresent => {
                self.pipeline_details(pipeline_id).animations_running = false;
            }
            AnimationState::NoAnimationCallbacksPresent => {
                self.pipeline_details(pipeline_id).animation_callbacks_running = false;
            }
        }
    }

    fn pipeline_details(&mut self, pipeline_id: PipelineId) -> &mut PipelineDetails {
        if !self.pipeline_details.contains_key(&pipeline_id) {
            self.pipeline_details.insert(pipeline_id, PipelineDetails::new());
        }
        self.pipeline_details.get_mut(&pipeline_id).expect("Insert then get failed!")
    }

    pub fn pipeline(&self, pipeline_id: PipelineId) -> Option<&CompositionPipeline> {
        match self.pipeline_details.get(&pipeline_id) {
            Some(ref details) => details.pipeline.as_ref(),
            None => {
                warn!("Compositor layer has an unknown pipeline ({:?}).", pipeline_id);
                None
            }
        }
    }

    fn create_pipeline_details_for_frame_tree(&mut self, frame_tree: &SendableFrameTree) {
        self.pipeline_details(frame_tree.pipeline.id).pipeline = Some(frame_tree.pipeline.clone());

        for kid in &frame_tree.children {
            self.create_pipeline_details_for_frame_tree(kid);
        }
    }

    fn remove_pipeline_root_layer(&mut self, pipeline_id: PipelineId) {
        self.pipeline_details.remove(&pipeline_id);
    }

    pub fn on_mouse_window_event_class(&mut self, wrdoc: webrender_api::DocumentId, mouse_window_event: MouseWindowEvent) {
        if let Some(area) = self.get_area_for_wrdoc(wrdoc) {
            if opts::get().convert_mouse_to_touch {
                match mouse_window_event {
                    MouseWindowEvent::Click(_, _) => {}
                    MouseWindowEvent::MouseDown(_, p) => area.on_touch_down(TouchId(0), p),
                    MouseWindowEvent::MouseUp(_, p) => area.on_touch_up(TouchId(0), p),
                }
                return
            } else {
                area.dispatch_mouse_window_event_class(mouse_window_event);
            }
        }
    }

    pub fn on_resize_window_event(&mut self) {
        debug!("compositor resize requested");

        let old_coords = self.embedder_coordinates;
        self.embedder_coordinates = self.window.get_coordinates();

        // FIXME: there's more to this. What if area coordinates changed.
        for area in &mut self.areas {
            area.on_resize_window_event(self.embedder_coordinates);
        }
    }


    pub fn on_touch_event(&mut self,
                          wrdoc: webrender_api::DocumentId,
                          event_type: TouchEventType,
                          identifier: TouchId,
                          location: DevicePoint) {
        if let Some(area) = self.get_area_for_wrdoc(wrdoc) {
            area.on_touch_event(event_type, identifier, location);
        }
    }

    pub fn on_zoom_window_event(&mut self, wrdoc: webrender_api::DocumentId, magnification: f32) {
        if let Some(area) = self.get_area_for_wrdoc(wrdoc) {
            area.on_zoom_window_event(magnification);
        }
    }

    pub fn on_zoom_reset_window_event(&mut self, wrdoc: webrender_api::DocumentId) {
        if let Some(area) = self.get_area_for_wrdoc(wrdoc) {
            area.on_zoom_reset_window_event();
        }
    }

    pub fn on_scroll_event(&mut self,
                           wrdoc: webrender_api::DocumentId,
                           delta: ScrollLocation,
                           cursor: DeviceIntPoint,
                           phase: TouchEventType) {

        if let Some(area) = self.get_area_for_wrdoc(wrdoc) {
            area.on_scroll_event(delta, cursor, phase);
        }
    }

    /// If there are any animations running, dispatches appropriate messages to the constellation.
    fn process_animations(&mut self) {
        let mut pipeline_ids = vec![];
        for (pipeline_id, pipeline_details) in &self.pipeline_details {
            if (pipeline_details.animations_running ||
                pipeline_details.animation_callbacks_running) &&
               pipeline_details.visible {
                   pipeline_ids.push(*pipeline_id);
            }
        }
        let animation_state = if pipeline_ids.is_empty() {
            windowing::AnimationState::Idle
        } else {
            windowing::AnimationState::Animating
        };
        self.window.set_animation_state(animation_state);
        for pipeline_id in &pipeline_ids {
            self.tick_animations_for_pipeline(*pipeline_id)
        }
    }

    fn tick_animations_for_pipeline(&mut self, pipeline_id: PipelineId) {
        let animation_callbacks_running = self.pipeline_details(pipeline_id).animation_callbacks_running;
        if animation_callbacks_running {
            let msg = ConstellationMsg::TickAnimation(pipeline_id, AnimationTickType::Script);
            if let Err(e) = self.constellation_chan.send(msg) {
                warn!("Sending tick to constellation failed ({}).", e);
            }
        }

        // We may need to tick animations in layout. (See #12749.)
        let animations_running = self.pipeline_details(pipeline_id).animations_running;
        if animations_running {
            let msg = ConstellationMsg::TickAnimation(pipeline_id, AnimationTickType::Layout);
            if let Err(e) = self.constellation_chan.send(msg) {
                warn!("Sending tick to constellation failed ({}).", e);
            }
        }
    }

    /// Simulate a pinch zoom
    pub fn on_pinch_zoom_window_event(&mut self, wrdoc: webrender_api::DocumentId, magnification: f32) {
        if let Some(area) = self.get_area_for_wrdoc(wrdoc) {
            area.on_pinch_zoom_window_event(magnification);
        }
    }

    pub fn on_mouse_window_move_event_class(&mut self, wrdoc: webrender_api::DocumentId, cursor: DevicePoint) {
        if let Some(area) = self.get_area_for_wrdoc(wrdoc) {
            if opts::get().convert_mouse_to_touch {
                area.on_touch_move(TouchId(0), cursor);
            } else {
                area.dispatch_mouse_window_move_event_class(cursor);
            }
        }
    }

    // Check if any pipelines currently have active animations or animation callbacks.
    fn animations_active(&self) -> bool {
        for (_, details) in &self.pipeline_details {
            // If animations are currently running, then don't bother checking
            // with the constellation if the output image is stable.
            if details.animations_running {
                return true;
            }
            if details.animation_callbacks_running {
                return true;
            }
        }

        false
    }

    /// Query the constellation to see if the current compositor
    /// output matches the current frame tree output, and if the
    /// associated script threads are idle.
    fn is_ready_to_paint_image_output(&mut self) -> Result<(), NotReadyToPaint> {
        match self.ready_to_save_state {
            ReadyState::Unknown => {
                // Unsure if the output image is stable.

                // Collect the currently painted epoch of each pipeline that is
                // complete (i.e. has *all* layers painted to the requested epoch).
                // This gets sent to the constellation for comparison with the current
                // frame tree.
                let mut pipeline_epochs = HashMap::new();
                for (id, _) in &self.pipeline_details {
                    let webrender_pipeline_id = id.to_webrender();
                    if let Some(webrender_api::Epoch(epoch)) = self.webrender
                                                                   .current_epoch(webrender_pipeline_id) {
                        let epoch = Epoch(epoch);
                        pipeline_epochs.insert(*id, epoch);
                    }
                }

                // Pass the pipeline/epoch states to the constellation and check
                // if it's safe to output the image.
                let msg = ConstellationMsg::IsReadyToSaveImage(pipeline_epochs);
                if let Err(e) = self.constellation_chan.send(msg) {
                    warn!("Sending ready to save to constellation failed ({}).", e);
                }
                self.ready_to_save_state = ReadyState::WaitingForConstellationReply;
                Err(NotReadyToPaint::JustNotifiedConstellation)
            }
            ReadyState::WaitingForConstellationReply => {
                // If waiting on a reply from the constellation to the last
                // query if the image is stable, then assume not ready yet.
                Err(NotReadyToPaint::WaitingOnConstellation)
            }
            ReadyState::ReadyToSaveImage => {
                // Constellation has replied at some point in the past
                // that the current output image is stable and ready
                // for saving.
                // Reset the flag so that we check again in the future
                // TODO: only reset this if we load a new document?
                if opts::get().is_running_problem_test {
                    println!("was ready to save, resetting ready_to_save_state");
                }
                self.ready_to_save_state = ReadyState::Unknown;
                Ok(())
            }
        }
    }

    pub fn composite(&mut self) {
        let target = self.composite_target;
        match self.composite_specific_target(target) {
            Ok(_) => if opts::get().output_file.is_some() || opts::get().exit_after_load {
                println!("Shutting down the Constellation after generating an output file or exit flag specified");
                self.start_shutting_down();
            },
            Err(e) => if opts::get().is_running_problem_test {
                if e != UnableToComposite::NotReadyToPaintImage(NotReadyToPaint::WaitingOnConstellation) {
                    println!("not ready to composite: {:?}", e);
                }
            },
        }
    }

    /// Composite either to the screen or to a png image or both.
    /// Returns Ok if composition was performed or Err if it was not possible to composite
    /// for some reason. If CompositeTarget is Window or Png no image data is returned;
    /// in the latter case the image is written directly to a file. If CompositeTarget
    /// is WindowAndPng Ok(Some(png::Image)) is returned.
    fn composite_specific_target(&mut self,
                                 target: CompositeTarget)
                                 -> Result<Option<Image>, UnableToComposite> {
        let width = self.embedder_coordinates.framebuffer.width_typed();
        let height = self.embedder_coordinates.framebuffer.height_typed();
        if !self.window.prepare_for_composite(width, height) {
            return Err(UnableToComposite::WindowUnprepared)
        }

        self.webrender.update();

        let wait_for_stable_image = match target {
            CompositeTarget::WindowAndPng | CompositeTarget::PngFile => true,
            CompositeTarget::Window => opts::get().exit_after_load,
        };

        if wait_for_stable_image {
            // The current image may be ready to output. However, if there are animations active,
            // tick those instead and continue waiting for the image output to be stable AND
            // all active animations to complete.
            if self.animations_active() {
                self.process_animations();
                return Err(UnableToComposite::NotReadyToPaintImage(NotReadyToPaint::AnimationsActive));
            }
            if let Err(result) = self.is_ready_to_paint_image_output() {
                return Err(UnableToComposite::NotReadyToPaintImage(result))
            }
        }

        let rt_info = match target {
            #[cfg(feature = "gleam")]
            CompositeTarget::Window => {
                gl::RenderTargetInfo::default()
            }
            #[cfg(feature = "gleam")]
            CompositeTarget::WindowAndPng |
            CompositeTarget::PngFile => {
                gl::initialize_png(&*self.window.gl(), width, height)
            }
            #[cfg(not(feature = "gleam"))]
            _ => ()
        };

        profile(ProfilerCategory::Compositing, None, self.time_profiler_chan.clone(), || {
            debug!("compositor: compositing");

            // Paint the scene.
            // TODO(gw): Take notice of any errors the renderer returns!
            self.webrender.render(self.embedder_coordinates.framebuffer).ok();
        });

        // If there are pending paint metrics, we check if any of the painted epochs is
        // one of the ones that the paint metrics recorder is expecting . In that case,
        // we get the current time, inform the layout thread about it and remove the
        // pending metric from the list.
        if !self.pending_paint_metrics.is_empty() {
            let paint_time = precise_time_ns();
            let mut to_remove = Vec::new();
            // For each pending paint metrics pipeline id
            for (id, pending_epoch) in &self.pending_paint_metrics {
                // we get the last painted frame id from webrender
                if let Some(webrender_api::Epoch(epoch)) = self.webrender.current_epoch(id.to_webrender()) {
                    // and check if it is the one the layout thread is expecting,
                    let epoch = Epoch(epoch);
                    if *pending_epoch != epoch {
                        continue;
                    }
                    // in which case, we remove it from the list of pending metrics,
                    to_remove.push(id.clone());
                    if let Some(pipeline) = self.pipeline(*id) {
                        // and inform the layout thread with the measured paint time.
                        let msg = LayoutControlMsg::PaintMetric(epoch, paint_time);
                        if let Err(e)  = pipeline.layout_chan.send(msg) {
                            warn!("Sending PaintMetric message to layout failed ({}).", e);
                        }
                    }
                }
            }
            for id in to_remove.iter() {
                self.pending_paint_metrics.remove(id);
            }
        }

        let rv = match target {
            CompositeTarget::Window => None,
            #[cfg(feature = "gleam")]
            CompositeTarget::WindowAndPng => {
                let img = gl::draw_img(&*self.window.gl(), rt_info, width, height);
                Some(Image {
                    width: img.width(),
                    height: img.height(),
                    format: PixelFormat::RGB8,
                    bytes: ipc::IpcSharedMemory::from_bytes(&*img),
                    id: None,
                })
            }
            #[cfg(feature = "gleam")]
            CompositeTarget::PngFile => {
                let gl = &*self.window.gl();
                profile(ProfilerCategory::ImageSaving, None, self.time_profiler_chan.clone(), || {
                    match opts::get().output_file.as_ref() {
                        Some(path) => match File::create(path) {
                            Ok(mut file) => {
                                let img = gl::draw_img(gl, rt_info, width, height);
                                let dynamic_image = DynamicImage::ImageRgb8(img);
                                if let Err(e) = dynamic_image.write_to(&mut file, ImageFormat::PNG) {
                                    error!("Failed to save {} ({}).", path, e);
                                }
                            },
                            Err(e) => error!("Failed to create {} ({}).", path, e),
                        },
                        None => error!("No file specified."),
                    }
                });
                None
            }
            #[cfg(not(feature = "gleam"))]
            _ => None,
        };

        // Perform the page flip. This will likely block for a while.
        self.window.present();

        self.last_composite_time = precise_time_ns();

        self.composition_request = CompositionRequest::NoCompositingNecessary;

        self.process_animations();

        for area in &mut self.areas {
            area.composite_done();
        }

        Ok(rv)
    }

    fn composite_if_necessary(&mut self, reason: CompositingReason) {
        if self.composition_request == CompositionRequest::NoCompositingNecessary {
            if opts::get().is_running_problem_test {
                println!("updating composition_request ({:?})", reason);
            }
            self.composition_request = CompositionRequest::CompositeNow(reason)
        } else if opts::get().is_running_problem_test {
            println!("composition_request is already {:?}", self.composition_request);
        }
    }

    pub fn receive_messages(&mut self) -> bool {
        // Check for new messages coming from the other threads in the system.
        let mut compositor_messages = vec![];
        let mut found_recomposite_msg = false;
        while let Some(msg) = self.port.try_recv_compositor_msg() {
            match msg {
                Msg::Recomposite(_) if found_recomposite_msg => {}
                Msg::Recomposite(_) => {
                    found_recomposite_msg = true;
                    compositor_messages.push(msg)
                }
                _ => compositor_messages.push(msg),
            }
        }
        for msg in compositor_messages {
            if !self.handle_browser_message(msg) {
                return false
            }
        }
        true
    }

    pub fn perform_updates(&mut self) -> bool {
        if self.shutdown_state == ShutdownState::FinishedShuttingDown {
            return false;
        }

        match self.composition_request {
            CompositionRequest::NoCompositingNecessary => {}
            CompositionRequest::CompositeNow(_) => {
                self.composite()
            }
        }

        let scroll_states: Vec<Option<ScrollStates>> = self.areas.iter_mut()
            .map(|area| area.process_pending_scroll_events_if_needed())
            .collect();

        scroll_states.into_iter().for_each(|scroll_states| {
            if let Some(scroll_states) = scroll_states {
                self.update_scroll_states(scroll_states);
            }
        });

        self.shutdown_state != ShutdownState::FinishedShuttingDown
    }

    fn update_scroll_states(&mut self, scroll_states: ScrollStates) {
        for (pipeline_id, scroll_states) in scroll_states {
            if let Some(pipeline) = self.pipeline(pipeline_id) {
                let msg = LayoutControlMsg::SetScrollStates(scroll_states);
                let _ = pipeline.layout_chan.send(msg);
            }
        }
    }

    /// Repaints and recomposites synchronously. You must be careful when calling this, as if a
    /// paint is not scheduled the compositor will hang forever.
    ///
    /// This is used when resizing the window.
    pub fn repaint_synchronously(&mut self) {
        while self.shutdown_state != ShutdownState::ShuttingDown {
            let msg = self.port.recv_compositor_msg();
            let need_recomposite = match msg {
                Msg::Recomposite(_) => true,
                _ => false,
            };
            let keep_going = self.handle_browser_message(msg);
            if need_recomposite {
                self.composite();
                break
            }
            if !keep_going {
                break
            }
        }
    }

    pub fn toggle_webrender_debug(&mut self, option: WebRenderDebugOption) {
        let mut flags = self.webrender.get_debug_flags();
        let flag = match option {
            WebRenderDebugOption::Profiler => {
                webrender::DebugFlags::PROFILER_DBG |
                webrender::DebugFlags::GPU_TIME_QUERIES |
                webrender::DebugFlags::GPU_SAMPLE_QUERIES
            }
            WebRenderDebugOption::TextureCacheDebug => {
                webrender::DebugFlags::TEXTURE_CACHE_DBG
            }
            WebRenderDebugOption::RenderTargetDebug => {
                webrender::DebugFlags::RENDER_TARGET_DBG
            }
        };
        flags.toggle(flag);
        self.webrender.set_debug_flags(flags);

        for area in &mut self.areas {
            area.generate_frame();
        }
    }

    pub fn capture_webrender(&mut self) {
        let capture_id = now().to_timespec().sec.to_string();
        let available_path = [env::current_dir(), Ok(env::temp_dir())].iter()
            .filter_map(|val| val.as_ref().map(|dir| dir.join("capture_webrender").join(&capture_id)).ok())
            .find(|val| {
                match create_dir_all(&val) {
                    Ok(_) => true,
                    Err(err) => {
                        eprintln!("Unable to create path '{:?}' for capture: {:?}", &val, err);
                        false
                    }
                }
            });

        match available_path {
            Some(capture_path) => {
                let revision_file_path = capture_path.join("wr.txt");

                debug!("Trying to save webrender capture under {:?}", &revision_file_path);
                self.webrender_api.save_capture(capture_path, webrender_api::CaptureBits::all());

                match File::create(revision_file_path) {
                    Ok(mut file) => {
                        let revision = include!(concat!(env!("OUT_DIR"), "/webrender_revision.rs"));
                        if let Err(err) = write!(&mut file, "{}", revision) {
                            eprintln!("Unable to write webrender revision: {:?}", err)
                        }
                    }
                    Err(err) => eprintln!("Capture triggered, creating webrender revision info skipped: {:?}", err)
                }
            },
            None => eprintln!("Unable to locate path to save captures")
        }
    }
}

/// Why we performed a composite. This is used for debugging.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompositingReason {
    /// We hit the delayed composition timeout. (See `delayed_composition.rs`.)
    DelayedCompositeTimeout,
    /// The window has been scrolled and we're starting the first recomposite.
    Scroll,
    /// A scroll has continued and we need to recomposite again.
    ContinueScroll,
    /// We're performing the single composite in headless mode.
    Headless,
    /// We're performing a composite to run an animation.
    Animation,
    /// A new frame tree has been loaded.
    NewFrameTree,
    /// New painted buffers have been received.
    NewPaintedBuffers,
    /// The window has been zoomed.
    Zoom,
    /// A new WebRender frame has arrived.
    NewWebRenderFrame,
    /// WebRender has processed a scroll event and has generated a new frame.
    NewWebRenderScrollFrame,
}
