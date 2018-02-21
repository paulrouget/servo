/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! A windowing implementation using glutin.

use compositing::compositor_thread::EventLoopWaker;
use compositing::windowing::{AnimationState, MouseWindowEvent, WindowEvent};
use compositing::windowing::{EmbedderCoordinates, WindowMethods};
use euclid::{Point2D, TypedPoint2D, TypedVector2D, TypedScale, TypedSize2D};
#[cfg(target_os = "windows")]
use gdi32;
use gleam::gl;
use glutin;
use glutin::{Api, ElementState, Event, GlContext, GlRequest, MouseButton, MouseScrollDelta, VirtualKeyCode};
use glutin::TouchPhase;
#[cfg(target_os = "macos")]
use glutin::os::macos::{ActivationPolicy, WindowBuilderExt};
use msg::constellation_msg::{Key, KeyState};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use osmesa_sys;
use script_traits::TouchEventType;
use servo_config::opts;
use servo_geometry::DeviceIndependentPixel;
use std::cell::{Cell, RefCell};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::ffi::CString;
use std::mem;
use std::os::raw::c_void;
use std::ptr;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time;
use style_traits::DevicePixel;
use style_traits::cursor::CursorKind;
#[cfg(target_os = "windows")]
use user32;
use webrender_api::{DeviceUintRect, ScrollLocation};
#[cfg(target_os = "windows")]
use winapi;

use super::keyutils::{self, GlutinKeyModifiers};

// This should vary by zoom level and maybe actual text size (focused or under cursor)
const LINE_HEIGHT: f32 = 38.0;

const MULTISAMPLES: u16 = 16;

#[cfg(target_os = "macos")]
fn builder_with_platform_options(mut builder: glutin::WindowBuilder) -> glutin::WindowBuilder {
    if opts::get().headless || opts::get().output_file.is_some() {
        // Prevent the window from showing in Dock.app, stealing focus,
        // or appearing at all when running in headless mode or generating an
        // output file.
        builder = builder.with_activation_policy(ActivationPolicy::Prohibited)
    }
    builder
}

#[cfg(not(target_os = "macos"))]
fn builder_with_platform_options(builder: glutin::WindowBuilder) -> glutin::WindowBuilder {
    builder
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct HeadlessContext {
    width: u32,
    height: u32,
    _context: osmesa_sys::OSMesaContext,
    _buffer: Vec<u32>,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
struct HeadlessContext {
    width: u32,
    height: u32,
}

impl HeadlessContext {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn new(width: u32, height: u32) -> HeadlessContext {
        let mut attribs = Vec::new();

        attribs.push(osmesa_sys::OSMESA_PROFILE);
        attribs.push(osmesa_sys::OSMESA_CORE_PROFILE);
        attribs.push(osmesa_sys::OSMESA_CONTEXT_MAJOR_VERSION);
        attribs.push(3);
        attribs.push(osmesa_sys::OSMESA_CONTEXT_MINOR_VERSION);
        attribs.push(3);
        attribs.push(0);

        let context = unsafe {
            osmesa_sys::OSMesaCreateContextAttribs(attribs.as_ptr(), ptr::null_mut())
        };

        assert!(!context.is_null());

        let mut buffer = vec![0; (width * height) as usize];

        unsafe {
            let ret = osmesa_sys::OSMesaMakeCurrent(context,
                                                    buffer.as_mut_ptr() as *mut _,
                                                    gl::UNSIGNED_BYTE,
                                                    width as i32,
                                                    height as i32);
            assert_ne!(ret, 0);
        };

        HeadlessContext {
            width: width,
            height: height,
            _context: context,
            _buffer: buffer,
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn new(width: u32, height: u32) -> HeadlessContext {
        HeadlessContext {
            width: width,
            height: height,
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn get_proc_address(s: &str) -> *const c_void {
        let c_str = CString::new(s).expect("Unable to create CString");
        unsafe {
            mem::transmute(osmesa_sys::OSMesaGetProcAddress(c_str.as_ptr()))
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn get_proc_address(_: &str) -> *const c_void {
        ptr::null() as *const _
    }
}

enum WindowKind {
    Window(glutin::GlWindow, RefCell<glutin::EventsLoop>),
    Headless(HeadlessContext),
}

/// The type of a window.
pub struct Window {
    kind: WindowKind,
    screen: TypedSize2D<u32, DeviceIndependentPixel>,

    mouse_down_button: Cell<Option<glutin::MouseButton>>,
    mouse_down_point: Cell<Point2D<i32>>,
    event_queue: RefCell<Vec<WindowEvent>>,

    mouse_pos: Cell<Point2D<i32>>,
    key_modifiers: Cell<GlutinKeyModifiers>,

    last_pressed_key: Cell<Option<Key>>,

    animation_state: Cell<AnimationState>,

    fullscreen: Cell<bool>,

    gl: Rc<gl::Gl>,
}

#[cfg(not(target_os = "windows"))]
fn window_creation_scale_factor() -> TypedScale<f32, DeviceIndependentPixel, DevicePixel> {
    TypedScale::new(1.0)
}

#[cfg(target_os = "windows")]
fn window_creation_scale_factor() -> TypedScale<f32, DeviceIndependentPixel, DevicePixel> {
        let hdc = unsafe { user32::GetDC(::std::ptr::null_mut()) };
        let ppi = unsafe { gdi32::GetDeviceCaps(hdc, winapi::wingdi::LOGPIXELSY) };
        TypedScale::new(ppi as f32 / 96.0)
}


impl Window {
    pub fn new(is_foreground: bool,
               window_size: TypedSize2D<u32, DeviceIndependentPixel>) -> Rc<Window> {
        let win_size: TypedSize2D<u32, DevicePixel> =
            (window_size.to_f32() * window_creation_scale_factor())
                .to_usize().cast().expect("Window size should fit in u32");
        let width = win_size.to_untyped().width;
        let height = win_size.to_untyped().height;

        // If there's no chrome, start off with the window invisible. It will be set to visible in
        // `load_end()`. This avoids an ugly flash of unstyled content (especially important since
        // unstyled content is white and chrome often has a transparent background). See issue
        // #9996.
        let visible = is_foreground && !opts::get().no_native_titlebar;

        let screen;
        let window_kind = if opts::get().headless {
            screen = TypedSize2D::new(width, height);
            WindowKind::Headless(HeadlessContext::new(width, height))
        } else {
            let events_loop = glutin::EventsLoop::new();
            let (screen_width, screen_height) = events_loop.get_primary_monitor().get_dimensions();
            screen = TypedSize2D::new(screen_width, screen_height);
            let mut window_builder = glutin::WindowBuilder::new()
                .with_title("Servo".to_string())
                .with_decorations(!opts::get().no_native_titlebar)
                .with_transparency(opts::get().no_native_titlebar)
                .with_dimensions(width, height)
                .with_visibility(visible)
                .with_multitouch();

            window_builder = builder_with_platform_options(window_builder);

            let mut context_builder = glutin::ContextBuilder::new()
                .with_gl(Window::gl_version())
                .with_vsync(opts::get().enable_vsync);

            if opts::get().use_msaa {
                context_builder = context_builder.with_multisampling(MULTISAMPLES)
            }

            let glutin_window = glutin::GlWindow::new(window_builder, context_builder, &events_loop)
                .expect("Failed to create window.");

            unsafe {
                glutin_window.context().make_current().expect("Couldn't make window current");
            }


            glutin_window.show();

            WindowKind::Window(glutin_window, RefCell::new(events_loop))
        };

        let gl = match window_kind {
            WindowKind::Window(ref window, ..) => {
                match gl::GlType::default() {
                    gl::GlType::Gl => {
                        unsafe {
                            gl::GlFns::load_with(|s| window.get_proc_address(s) as *const _)
                        }
                    }
                    gl::GlType::Gles => {
                        unsafe {
                            gl::GlesFns::load_with(|s| window.get_proc_address(s) as *const _)
                        }
                    }
                }
            }
            WindowKind::Headless(..) => {
                unsafe {
                    gl::GlFns::load_with(|s| HeadlessContext::get_proc_address(s))
                }
            }
        };

        if opts::get().headless {
            // Print some information about the headless renderer that
            // can be useful in diagnosing CI failures on build machines.
            println!("{}", gl.get_string(gl::VENDOR));
            println!("{}", gl.get_string(gl::RENDERER));
            println!("{}", gl.get_string(gl::VERSION));
        }

        gl.clear_color(0.6, 0.6, 0.6, 1.0);
        gl.clear(gl::COLOR_BUFFER_BIT);
        gl.finish();

        let window = Window {
            kind: window_kind,
            event_queue: RefCell::new(vec!()),
            mouse_down_button: Cell::new(None),
            mouse_down_point: Cell::new(Point2D::new(0, 0)),

            mouse_pos: Cell::new(Point2D::new(0, 0)),
            key_modifiers: Cell::new(GlutinKeyModifiers::empty()),

            last_pressed_key: Cell::new(None),
            gl: gl.clone(),
            animation_state: Cell::new(AnimationState::Idle),
            fullscreen: Cell::new(false),
            screen,
        };

        window.present();

        Rc::new(window)
    }

    pub fn get_events(&self) -> Vec<WindowEvent> {
        mem::replace(&mut *self.event_queue.borrow_mut(), Vec::new())
    }

    pub fn page_height(&self) -> f32 {
        let dpr = self.hidpi_factor();
        match self.kind {
            WindowKind::Window(ref window, _) => {
                let (_, height) = window.get_inner_size().expect("Failed to get window inner size.");
                height as f32 * dpr.get()
            },
            WindowKind::Headless(ref context) => {
                context.height as f32 * dpr.get()
            }
        }
    }

    pub fn set_title(&self, title: &str) {
        if let WindowKind::Window(ref window, _) = self.kind {
            window.set_title(title);
        }
    }

    pub fn set_inner_size(&self, size: TypedSize2D<usize, DevicePixel>) {
        if let WindowKind::Window(ref window, _) = self.kind {
            let size = size.to_f32() / self.hidpi_factor();
            window.set_inner_size(size.width as u32, size.height as u32)
        }
    }

    pub fn set_position(&self, point: TypedPoint2D<i32, DevicePixel>) {
        if let WindowKind::Window(ref window, _) = self.kind {
            let point = point.to_f32() / self.hidpi_factor();
            window.set_position(point.x as i32, point.y as i32)
        }
    }

    fn is_animating(&self) -> bool {
        self.animation_state.get() == AnimationState::Animating
    }

    pub fn run<T>(&self, mut servo_callback: T) where T: FnMut() -> bool {
        match self.kind {
            WindowKind::Window(_, ref events_loop) => {
                let mut stop = false;
                loop {
                    if self.is_animating() {
                        // We block on compositing (servo_callback ends up calling swap_buffers)
                        events_loop.borrow_mut().poll_events(|e| {
                            self.glutin_event_to_servo_event(e);
                        });
                        stop = servo_callback();
                    } else {
                        // We block on glutin's event loop (window events)
                        events_loop.borrow_mut().run_forever(|e| {
                            self.glutin_event_to_servo_event(e);
                            if !self.event_queue.borrow().is_empty() {
                                stop = servo_callback();
                            }
                            if stop || self.is_animating() {
                                glutin::ControlFlow::Break
                            } else {
                                glutin::ControlFlow::Continue
                            }
                        });
                    }
                    if stop {
                        break;
                    }
                }
            }
            WindowKind::Headless(..) => {
                loop {
                    // Sleep the main thread to avoid using 100% CPU
                    // This can be done better, see comments in #18777
                    if self.event_queue.borrow().is_empty() {
                        thread::sleep(time::Duration::from_millis(5));
                    }
                    let stop = servo_callback();
                    if stop {
                        break;
                    }
                }
            }
        }
    }

    #[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
    fn gl_version() -> GlRequest {
        return GlRequest::Specific(Api::OpenGl, (3, 2));
    }

    #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
    fn gl_version() -> GlRequest {
        GlRequest::Specific(Api::OpenGlEs, (3, 0))
    }

    fn handle_received_character(&self, ch: char) {
        let modifiers = keyutils::glutin_mods_to_script_mods(self.key_modifiers.get());
        if let Some(last_pressed_key) = self.last_pressed_key.get() {
            let event = WindowEvent::KeyEvent(Some(ch), last_pressed_key, KeyState::Pressed, modifiers);
            self.event_queue.borrow_mut().push(event);
        } else {
            // Only send the character if we can print it (by ignoring characters like backspace)
            if !ch.is_control() {
                match keyutils::char_to_script_key(ch) {
                    Some(key) => {
                        let event = WindowEvent::KeyEvent(Some(ch),
                                                          key,
                                                          KeyState::Pressed,
                                                          modifiers);
                        self.event_queue.borrow_mut().push(event);
                    }
                    None => {}
                }
            }
        }
        self.last_pressed_key.set(None);
    }

    fn toggle_keyboard_modifiers(&self, virtual_key_code: VirtualKeyCode) {
        match virtual_key_code {
            VirtualKeyCode::LControl => self.toggle_modifier(GlutinKeyModifiers::LEFT_CONTROL),
            VirtualKeyCode::RControl => self.toggle_modifier(GlutinKeyModifiers::RIGHT_CONTROL),
            VirtualKeyCode::LShift => self.toggle_modifier(GlutinKeyModifiers::LEFT_SHIFT),
            VirtualKeyCode::RShift => self.toggle_modifier(GlutinKeyModifiers::RIGHT_SHIFT),
            VirtualKeyCode::LAlt => self.toggle_modifier(GlutinKeyModifiers::LEFT_ALT),
            VirtualKeyCode::RAlt => self.toggle_modifier(GlutinKeyModifiers::RIGHT_ALT),
            VirtualKeyCode::LWin => self.toggle_modifier(GlutinKeyModifiers::LEFT_SUPER),
            VirtualKeyCode::RWin => self.toggle_modifier(GlutinKeyModifiers::RIGHT_SUPER),
            _ => {}
        }
    }

    fn handle_keyboard_input(&self, element_state: ElementState, virtual_key_code: VirtualKeyCode) {
        self.toggle_keyboard_modifiers(virtual_key_code);

        if let Ok(key) = keyutils::glutin_key_to_script_key(virtual_key_code) {
            let state = match element_state {
                ElementState::Pressed => KeyState::Pressed,
                ElementState::Released => KeyState::Released,
            };
            if element_state == ElementState::Pressed {
                if keyutils::is_printable(virtual_key_code) {
                    self.last_pressed_key.set(Some(key));
                }
            }
            let modifiers = keyutils::glutin_mods_to_script_mods(self.key_modifiers.get());
            self.event_queue.borrow_mut().push(WindowEvent::KeyEvent(None, key, state, modifiers));
        }
    }

    fn glutin_event_to_servo_event(&self, event: glutin::Event) {
        match event {
            Event::WindowEvent {
                event: glutin::WindowEvent::ReceivedCharacter(ch),
                ..
            } => self.handle_received_character(ch),
            Event::WindowEvent {
                event: glutin::WindowEvent::KeyboardInput {
                    input: glutin::KeyboardInput {
                        state, virtual_keycode: Some(virtual_keycode), ..
                    }, ..
                }, ..
            } => self.handle_keyboard_input(state, virtual_keycode),
            Event::WindowEvent {
                event: glutin::WindowEvent::MouseInput {
                    state, button, ..
                }, ..
            } => {
                if button == MouseButton::Left || button == MouseButton::Right {
                    let mouse_pos = self.mouse_pos.get();
                    self.handle_mouse(button, state, mouse_pos.x, mouse_pos.y);
                }
            },
            Event::WindowEvent {
                event: glutin::WindowEvent::CursorMoved {
                    position: (x, y),
                    ..
                },
                ..
            } => {
                self.mouse_pos.set(Point2D::new(x as i32, y as i32));
                self.event_queue.borrow_mut().push(
                    WindowEvent::MouseWindowMoveEventClass(TypedPoint2D::new(x as f32, y as f32)));
            }
            Event::WindowEvent {
                event: glutin::WindowEvent::MouseWheel { delta, phase, .. },
                ..
            } => {
                let (mut dx, mut dy) = match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => (dx, dy * LINE_HEIGHT),
                    MouseScrollDelta::PixelDelta(dx, dy) => (dx, dy),
                };
                // Scroll events snap to the major axis of movement, with vertical
                // preferred over horizontal.
                if dy.abs() >= dx.abs() {
                    dx = 0.0;
                } else {
                    dy = 0.0;
                }

                let scroll_location = ScrollLocation::Delta(TypedVector2D::new(dx, dy));
                let phase = glutin_phase_to_touch_event_type(phase);
                let mouse_pos = self.mouse_pos.get();
                let event = WindowEvent::Scroll(scroll_location,
                                                TypedPoint2D::new(mouse_pos.x as i32, mouse_pos.y as i32),
                                                phase);
                self.event_queue.borrow_mut().push(event);
            },
            Event::WindowEvent {
                event: glutin::WindowEvent::Touch(touch),
                ..
            } => {
                use script_traits::TouchId;

                let phase = glutin_phase_to_touch_event_type(touch.phase);
                let id = TouchId(touch.id as i32);
                let point = TypedPoint2D::new(touch.location.0 as f32, touch.location.1 as f32);
                self.event_queue.borrow_mut().push(WindowEvent::Touch(phase, id, point));
            }
            Event::WindowEvent {
                event: glutin::WindowEvent::Refresh,
                ..
            } => self.event_queue.borrow_mut().push(WindowEvent::Refresh),
            Event::WindowEvent {
                event: glutin::WindowEvent::Closed,
                ..
            } => {
                self.event_queue.borrow_mut().push(WindowEvent::Quit);
            }
            Event::WindowEvent {
                event: glutin::WindowEvent::Resized(x, y),
                ..
            } => {
                if let WindowKind::Window(ref window, _) = self.kind {
                    window.resize(x, y);
                }
                self.event_queue.borrow_mut().push(WindowEvent::Resize);
            }
            Event::Awakened => {
                self.event_queue.borrow_mut().push(WindowEvent::Idle);
            }
            _ => {}
        }
    }

    fn toggle_modifier(&self, modifier: GlutinKeyModifiers) {
        let mut modifiers = self.key_modifiers.get();
        modifiers.toggle(modifier);
        self.key_modifiers.set(modifiers);
    }

    /// Helper function to handle a click
    fn handle_mouse(&self, button: glutin::MouseButton, action: glutin::ElementState, x: i32, y: i32) {
        use script_traits::MouseButton;

        // FIXME(tkuehn): max pixel dist should be based on pixel density
        let max_pixel_dist = 10f64;
        let event = match action {
            ElementState::Pressed => {
                self.mouse_down_point.set(Point2D::new(x, y));
                self.mouse_down_button.set(Some(button));
                MouseWindowEvent::MouseDown(MouseButton::Left, TypedPoint2D::new(x as f32, y as f32))
            }
            ElementState::Released => {
                let mouse_up_event = MouseWindowEvent::MouseUp(MouseButton::Left,
                                                               TypedPoint2D::new(x as f32, y as f32));
                match self.mouse_down_button.get() {
                    None => mouse_up_event,
                    Some(but) if button == but => {
                        let pixel_dist = self.mouse_down_point.get() - Point2D::new(x, y);
                        let pixel_dist = ((pixel_dist.x * pixel_dist.x +
                                           pixel_dist.y * pixel_dist.y) as f64).sqrt();
                        if pixel_dist < max_pixel_dist {
                            self.event_queue.borrow_mut().push(WindowEvent::MouseWindowEventClass(mouse_up_event));
                            MouseWindowEvent::Click(MouseButton::Left, TypedPoint2D::new(x as f32, y as f32))
                        } else {
                            mouse_up_event
                        }
                    },
                    Some(_) => mouse_up_event,
                }
            }
        };
        self.event_queue.borrow_mut().push(WindowEvent::MouseWindowEventClass(event));
    }

    #[cfg(not(target_os = "windows"))]
    fn hidpi_factor(&self) -> TypedScale<f32, DeviceIndependentPixel, DevicePixel> {
        match self.kind {
            WindowKind::Window(ref window, ..) => {
                TypedScale::new(window.hidpi_factor())
            }
            WindowKind::Headless(..) => {
                TypedScale::new(1.0)
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn hidpi_factor(&self) -> TypedScale<f32, DeviceIndependentPixel, DevicePixel> {
        let hdc = unsafe { user32::GetDC(::std::ptr::null_mut()) };
        let ppi = unsafe { gdi32::GetDeviceCaps(hdc, winapi::wingdi::LOGPIXELSY) };
        TypedScale::new(ppi as f32 / 96.0)
    }

    /// Has no effect on Android.
    pub fn set_cursor(&self, cursor: CursorKind) {
        match self.kind {
            WindowKind::Window(ref window, ..) => {
                use glutin::MouseCursor;

                let glutin_cursor = match cursor {
                    CursorKind::Auto => MouseCursor::Default,
                    CursorKind::None => MouseCursor::NoneCursor,
                    CursorKind::Default => MouseCursor::Default,
                    CursorKind::Pointer => MouseCursor::Hand,
                    CursorKind::ContextMenu => MouseCursor::ContextMenu,
                    CursorKind::Help => MouseCursor::Help,
                    CursorKind::Progress => MouseCursor::Progress,
                    CursorKind::Wait => MouseCursor::Wait,
                    CursorKind::Cell => MouseCursor::Cell,
                    CursorKind::Crosshair => MouseCursor::Crosshair,
                    CursorKind::Text => MouseCursor::Text,
                    CursorKind::VerticalText => MouseCursor::VerticalText,
                    CursorKind::Alias => MouseCursor::Alias,
                    CursorKind::Copy => MouseCursor::Copy,
                    CursorKind::Move => MouseCursor::Move,
                    CursorKind::NoDrop => MouseCursor::NoDrop,
                    CursorKind::NotAllowed => MouseCursor::NotAllowed,
                    CursorKind::Grab => MouseCursor::Grab,
                    CursorKind::Grabbing => MouseCursor::Grabbing,
                    CursorKind::EResize => MouseCursor::EResize,
                    CursorKind::NResize => MouseCursor::NResize,
                    CursorKind::NeResize => MouseCursor::NeResize,
                    CursorKind::NwResize => MouseCursor::NwResize,
                    CursorKind::SResize => MouseCursor::SResize,
                    CursorKind::SeResize => MouseCursor::SeResize,
                    CursorKind::SwResize => MouseCursor::SwResize,
                    CursorKind::WResize => MouseCursor::WResize,
                    CursorKind::EwResize => MouseCursor::EwResize,
                    CursorKind::NsResize => MouseCursor::NsResize,
                    CursorKind::NeswResize => MouseCursor::NeswResize,
                    CursorKind::NwseResize => MouseCursor::NwseResize,
                    CursorKind::ColResize => MouseCursor::ColResize,
                    CursorKind::RowResize => MouseCursor::RowResize,
                    CursorKind::AllScroll => MouseCursor::AllScroll,
                    CursorKind::ZoomIn => MouseCursor::ZoomIn,
                    CursorKind::ZoomOut => MouseCursor::ZoomOut,
                };
                window.set_cursor(glutin_cursor);
            }
            WindowKind::Headless(..) => {}
        }
    }
}

impl WindowMethods for Window {
    fn gl(&self) -> Rc<gl::Gl> {
        self.gl.clone()
    }

    fn get_coordinates(&self) -> EmbedderCoordinates {
        let dpr = self.hidpi_factor();
        match self.kind {
            WindowKind::Window(ref window, _) => {
                // TODO(ajeffrey): can this fail?
                let (width, height) = window.get_outer_size().expect("Failed to get window outer size.");
                let (x, y) = window.get_position().expect("Failed to get window position.");
                let win_size = (TypedSize2D::new(width as f32, height as f32) * dpr).to_usize();
                let win_origin = (TypedPoint2D::new(x as f32, y as f32) * dpr).to_i32();
                let screen = (self.screen.to_f32() * dpr).to_usize();

                let (width, height) = window.get_inner_size().expect("Failed to get window inner size.");
                let (width, height) = ((width as f32 * dpr.get()) as u32, (height as f32 * dpr.get()) as u32);
                let inner_size = TypedSize2D::new(width, height);

                let viewport = DeviceUintRect::new(TypedPoint2D::zero(), inner_size);

                EmbedderCoordinates {
                    viewport: viewport,
                    framebuffer: inner_size,
                    window: (win_size, win_origin),
                    screen: screen,
                    // FIXME: Glutin doesn't have API for available size. Fallback to screen size
                    screen_avail: screen,
                    hidpi_factor: dpr,
                }
            },
            WindowKind::Headless(ref context) => {
                let size = (TypedSize2D::new(context.width as f32, context.height as f32) * dpr).to_usize();
                let u32size = TypedSize2D::new(size.width as u32, size.height as u32);
                EmbedderCoordinates {
                    viewport: DeviceUintRect::new(TypedPoint2D::zero(), u32size),
                    framebuffer: u32size,
                    window: (size, TypedPoint2D::zero()),
                    screen: size,
                    screen_avail: size,
                    hidpi_factor: dpr,
                }
            }
        }
    }

    fn present(&self) {
        match self.kind {
            WindowKind::Window(ref window, ..) => {
                if let Err(err) = window.swap_buffers() {
                    warn!("Failed to swap window buffers ({}).", err);
                }
            }
            WindowKind::Headless(..) => {}
        }
    }

    fn create_event_loop_waker(&self) -> Box<EventLoopWaker> {
        struct GlutinEventLoopWaker {
            proxy: Option<Arc<glutin::EventsLoopProxy>>,
        }
        impl GlutinEventLoopWaker {
            fn new(window: &Window) -> GlutinEventLoopWaker {
                let proxy = match window.kind {
                    WindowKind::Window(_, ref events_loop) => {
                        Some(Arc::new(events_loop.borrow().create_proxy()))
                    },
                    WindowKind::Headless(..) => {
                        None
                    }
                };
                GlutinEventLoopWaker { proxy }
            }
        }
        impl EventLoopWaker for GlutinEventLoopWaker {
            fn wake(&self) {
                // kick the OS event loop awake.
                if let Some(ref proxy) = self.proxy {
                    if let Err(err) = proxy.wakeup() {
                        warn!("Failed to wake up event loop ({}).", err);
                    }
                }
            }
            fn clone(&self) -> Box<EventLoopWaker + Send> {
                Box::new(GlutinEventLoopWaker {
                    proxy: self.proxy.clone(),
                })
            }
        }

        Box::new(GlutinEventLoopWaker::new(&self))
    }

    fn set_animation_state(&self, state: AnimationState) {
        self.animation_state.set(state);
    }

    fn prepare_for_composite(&self, _width: usize, _height: usize) -> bool {
        true
    }

    fn supports_clipboard(&self) -> bool {
        true
    }
}

fn glutin_phase_to_touch_event_type(phase: TouchPhase) -> TouchEventType {
    match phase {
        TouchPhase::Started => TouchEventType::Down,
        TouchPhase::Moved => TouchEventType::Move,
        TouchPhase::Ended => TouchEventType::Up,
        TouchPhase::Cancelled => TouchEventType::Cancel,
    }
}
