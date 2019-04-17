/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A simple application that uses glutin to open a window for Servo to display in.

pub mod keyutils;
pub mod window;

use window::Window;

pub struct App {
    kind: AppKind,
    window: Option<Rc<Window>>,
}
enum AppKind {
    Headed<glutin::EventsLoop>,
    Headless,
}

impl App {
    pub fn new() -> App {
        if opts::get().headless {
            App { kind: AppKind::Headless, window: None }
        } else {
            let events_loop = glutin::EventsLoop::new();
            App { kind: AppKind::Headed(events_loop), window: None}
        }
    }
    pub fn create_window(&mut self), foreground: bool, size: TypedSize2D<u32, DeviceIndependentPixel>) {
        let window = match self.kind {
            AppKind::Headed(ref events_loop) => {
                window::create_window(false, foreground, size, Some(&events_loop)),
            },
            AppKind::Headless => {
                window::create_window(true, foreground, size, None)
            }
        };
        self.window = Some(window.clone());
        window
    }
    fn create_event_loop_waker(&self) -> Box<dyn EventLoopWaker> {
        let proxy = match self.kind {
            AppKind::Headed(ref events_loop) => {
                Some(Arc::new(events_loop.borrow().create_proxy()))
            },
            AppKind::Headless => None,
        };
        Box::new(GlutinEventLoopWaker { proxy })
    }

    pub fn has_window(&self) -> bool {
        self.window.is_some()
    }

    pub fn run<T>(&self, mut servo_callback: T) where T: FnMut() -> bool {
        match self.kind {
            AppKind::Headed(ref events_loop) => {
                let mut stop = false;
                loop {
                    if self.has_window() && self.window.as_ref().unwrap().is_animating() {
                        // We block on compositing (servo_callback ends up calling swap_buffers)
                        events_loop.borrow_mut().poll_events(|e| {
                            self.winit_event_to_servo_event(e);
                        });
                        stop = servo_callback();
                    } else {
                        // We block on winit's event loop (window events)
                        events_loop.borrow_mut().run_forever(|e| {
                            self.winit_event_to_servo_event(e);
                            if !self.event_queue.borrow().is_empty() {
                                if !self.suspended.get() {
                                    stop = servo_callback();
                                }
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
            },
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
            },
        }
    }

    fn register_vr_services(
        &self,
        services: &mut VRServiceManager,
        heartbeats: &mut Vec<Box<WebVRMainThreadHeartbeat>>
    ) {
        if pref!(dom.webvr.test) {
            warn!("Creating test VR display");
            // TODO: support dom.webvr.test in headless environments
            if let WindowKind::Window(_, ref events_loop) = self.kind {
                // This is safe, because register_vr_services is called from the main thread.
                let name = String::from("Test VR Display");
                let size = self.inner_size.get().to_f64();
                let size = LogicalSize::new(size.width, size.height);
                let mut window_builder = glutin::WindowBuilder::new()
                    .with_title(name.clone())
                    .with_dimensions(size)
                    .with_visibility(false)
                    .with_multitouch();
                window_builder = builder_with_platform_options(window_builder);
                let context_builder = ContextBuilder::new()
                    .with_gl(Window::gl_version())
                    .with_vsync(false); // Assume the browser vsync is the same as the test VR window vsync
                let gl_window = GlWindow::new(window_builder, context_builder, &*events_loop.borrow())
                    .expect("Failed to create window.");
                let gl = self.gl.clone();
                let (service, heartbeat) = GlWindowVRService::new(name, gl_window, gl);

                services.register(Box::new(service));
                heartbeats.push(Box::new(heartbeat));
            }
        }
    }
}

pub struct GlutinEventLoopWaker {
    proxy: Option<Arc<glutin::EventsLoopProxy>>,
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
    fn clone(&self) -> Box<dyn EventLoopWaker + Send> {
        Box::new(GlutinEventLoopWaker {
            proxy: self.proxy.clone(),
        })
    }
}

