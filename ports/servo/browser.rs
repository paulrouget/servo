/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use euclid::{TypedPoint2D, TypedVector2D};
use compositing::compositor_thread::EmbedderMsg;
use compositing::windowing::{WebRenderDebugOption, WindowEvent};
use glutin_app::keyutils::{CMD_OR_CONTROL, CMD_OR_ALT};
use glutin_app::window::Window;
use msg::constellation_msg::{Key, TopLevelBrowsingContextId as BrowserId};
use msg::constellation_msg::{KeyModifiers, KeyState, TraversalDirection};
use servo::servo_url::ServoUrl;
use script_traits::TouchEventType;
use servo_config::prefs::PREFS;
use std::mem;
use std::rc::Rc;
use webrender_api::ScrollLocation;

// FIXME: share with window.rs
const LINE_HEIGHT: f32 = 38.0;

pub struct Browser {
    current_url: Option<ServoUrl>,
    /// id of the top level browsing context. It is unique as tabs
    /// are not supported yet. None until created.
    browser_id: Option<BrowserId>,

    title: Option<String>,
    status: Option<String>,
    favicon: Option<ServoUrl>,
    loading_state: Option<LoadingState>,
    window: Rc<Window>,
    event_queue: Vec<WindowEvent>,
    shutdown_requested: bool,
}

enum LoadingState {
    Connecting,
    Loading,
    Loaded,
}

impl Browser {
    pub fn new(window: Rc<Window>) -> Browser {
        Browser {
            title: None,
            current_url: None,
            browser_id: None,
            status: None,
            favicon: None,
            loading_state: None,
            window: window,
            event_queue: Vec::new(),
            shutdown_requested: false,
        }
    }

    pub fn get_events(&mut self) -> Vec<WindowEvent> {
        mem::replace(&mut self.event_queue, Vec::new())
    }

    pub fn set_browser_id(&mut self, browser_id: BrowserId) {
        self.browser_id = Some(browser_id);
    }

    pub fn handle_window_events(&mut self, events: Vec<WindowEvent>) {
        for event in events {
            match event {
                WindowEvent::KeyEvent(..) => {
                    // FIXME. Do stuff
                    self.event_queue.push(event);
                },
                event => {
                    self.event_queue.push(event);
                }
            }
        }
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    /// Helper function to handle keyboard events.
    fn handle_key(&mut self, _: Option<BrowserId>, ch: Option<char>, key: Key, _: KeyState, mods: KeyModifiers) {
        let browser_id = match self.browser_id {
            Some(id) => id,
            None => { unreachable!("Can't get keys without a browser"); }
        };
        match (mods, ch, key) {
            (_, Some('+'), _) => {
                if mods & !KeyModifiers::SHIFT == CMD_OR_CONTROL {
                    self.event_queue.push(WindowEvent::Zoom(1.1));
                } else if mods & !KeyModifiers::SHIFT == CMD_OR_CONTROL | KeyModifiers::ALT {
                    self.event_queue.push(WindowEvent::PinchZoom(1.1));
                }
            }
            (CMD_OR_CONTROL, Some('-'), _) => {
                self.event_queue.push(WindowEvent::Zoom(1.0 / 1.1));
            }
            (_, Some('-'), _) if mods == CMD_OR_CONTROL | KeyModifiers::ALT => {
                self.event_queue.push(WindowEvent::PinchZoom(1.0 / 1.1));
            }
            (CMD_OR_CONTROL, Some('0'), _) => {
                self.event_queue.push(WindowEvent::ResetZoom);
            }

            (KeyModifiers::NONE, None, Key::NavigateForward) => {
                let event = WindowEvent::Navigation(browser_id, TraversalDirection::Forward(1));
                self.event_queue.push(event);
            }
            (KeyModifiers::NONE, None, Key::NavigateBackward) => {
                let event = WindowEvent::Navigation(browser_id, TraversalDirection::Back(1));
                self.event_queue.push(event);
            }

            (KeyModifiers::NONE, None, Key::Escape) => {
                if let Some(true) = PREFS.get("shell.builtin-key-shortcuts.enabled").as_boolean() {
                    self.event_queue.push(WindowEvent::Quit);
                }
            }

            (CMD_OR_ALT, None, Key::Right) => {
                let event = WindowEvent::Navigation(browser_id, TraversalDirection::Forward(1));
                self.event_queue.push(event);
            }
            (CMD_OR_ALT, None, Key::Left) => {
                let event = WindowEvent::Navigation(browser_id, TraversalDirection::Back(1));
                self.event_queue.push(event);
            }

            (KeyModifiers::NONE, None, Key::PageDown) => {
               let scroll_location = ScrollLocation::Delta(TypedVector2D::new(0.0,
                                   -self.window.page_height() + 2.0 * LINE_HEIGHT));
                self.scroll_window_from_key(scroll_location, TouchEventType::Move);
            }
            (KeyModifiers::NONE, None, Key::PageUp) => {
                let scroll_location = ScrollLocation::Delta(TypedVector2D::new(0.0,
                                   self.window.page_height() - 2.0 * LINE_HEIGHT));
                self.scroll_window_from_key(scroll_location, TouchEventType::Move);
            }

            (KeyModifiers::NONE, None, Key::Home) => {
                self.scroll_window_from_key(ScrollLocation::Start, TouchEventType::Move);
            }

            (KeyModifiers::NONE, None, Key::End) => {
                self.scroll_window_from_key(ScrollLocation::End, TouchEventType::Move);
            }

            (KeyModifiers::NONE, None, Key::Up) => {
                self.scroll_window_from_key(ScrollLocation::Delta(TypedVector2D::new(0.0, 3.0 * LINE_HEIGHT)),
                                            TouchEventType::Move);
            }
            (KeyModifiers::NONE, None, Key::Down) => {
                self.scroll_window_from_key(ScrollLocation::Delta(TypedVector2D::new(0.0, -3.0 * LINE_HEIGHT)),
                                            TouchEventType::Move);
            }
            (KeyModifiers::NONE, None, Key::Left) => {
                self.scroll_window_from_key(ScrollLocation::Delta(TypedVector2D::new(LINE_HEIGHT, 0.0)), TouchEventType::Move);
            }
            (KeyModifiers::NONE, None, Key::Right) => {
                self.scroll_window_from_key(ScrollLocation::Delta(TypedVector2D::new(-LINE_HEIGHT, 0.0)), TouchEventType::Move);
            }
            (CMD_OR_CONTROL, Some('r'), _) => {
                if let Some(true) = PREFS.get("shell.builtin-key-shortcuts.enabled").as_boolean() {
                    self.event_queue.push(WindowEvent::Reload(browser_id));
                }
            }
            (CMD_OR_CONTROL, Some('q'), _) => {
                if let Some(true) = PREFS.get("shell.builtin-key-shortcuts.enabled").as_boolean() {
                    self.event_queue.push(WindowEvent::Quit);
                }
            }
            (KeyModifiers::CONTROL, None, Key::F10) => {
                let event = WindowEvent::ToggleWebRenderDebug(WebRenderDebugOption::RenderTargetDebug);
                self.event_queue.push(event);
            }
            (KeyModifiers::CONTROL, None, Key::F11) => {
                let event = WindowEvent::ToggleWebRenderDebug(WebRenderDebugOption::TextureCacheDebug);
                self.event_queue.push(event);
            }
            (KeyModifiers::CONTROL, None, Key::F12) => {
                let event = WindowEvent::ToggleWebRenderDebug(WebRenderDebugOption::Profiler);
                self.event_queue.push(event);
            }

            _ => {
                self.platform_handle_key(key, mods, browser_id);
            }
        }
    }

    #[cfg(not(target_os = "win"))]
    fn platform_handle_key(&mut self, key: Key, mods: KeyModifiers, browser_id: BrowserId) {
        match (mods, key) {
            (CMD_OR_CONTROL, Key::LeftBracket) => {
                let event = WindowEvent::Navigation(browser_id, TraversalDirection::Back(1));
                self.event_queue.push(event);
            }
            (CMD_OR_CONTROL, Key::RightBracket) => {
                let event = WindowEvent::Navigation(browser_id, TraversalDirection::Forward(1));
                self.event_queue.push(event);
            }
            _ => {}
        }
    }

    #[cfg(target_os = "win")]
    fn platform_handle_key(&self, key: Key, mods: KeyModifiers, browser_id: BrowserId) {
    }

    fn scroll_window_from_key(&mut self, scroll_location: ScrollLocation, phase: TouchEventType) {
        let event = WindowEvent::Scroll(scroll_location, TypedPoint2D::zero(), phase);
        self.event_queue.push(event);
    }

    pub fn handle_servo_events(&mut self, events: Vec<EmbedderMsg>) {
        for event in events {
            match event {
                EmbedderMsg::Status(_browser_id, status) => {
                    self.status = status;
                },
                EmbedderMsg::ChangePageTitle(_browser_id, title) => {
                    self.title = title;

                    let fallback_title: String = if let Some(ref current_url) = self.current_url {
                        current_url.to_string()
                    } else {
                        String::from("Untitled")
                    };
                    let title = match self.title {
                        Some(ref title) if title.len() > 0 => &**title,
                        _ => &fallback_title,
                    };
                    let title = format!("{} - Servo", title);
                    self.window.set_title(&title);
                }
                EmbedderMsg::MoveTo(_browser_id, point) => {
                    self.window.set_position(point);
                }
                EmbedderMsg::ResizeTo(_browser_id, size) => {
                    self.window.set_inner_size(size);
                }
                EmbedderMsg::AllowNavigation(_browser_id, _url, response_chan) => {
                    if let Err(e) = response_chan.send(true) {
                        warn!("Failed to send allow_navigation() response: {}", e);
                    };
                }
                EmbedderMsg::KeyEvent(_browser_id, ch, key, state, modified) => {
                    self.handle_key(_browser_id, ch, key, state, modified);
                }
                EmbedderMsg::SetCursor(cursor) => {
                    self.window.set_cursor(cursor);
                }
                EmbedderMsg::NewFavicon(_browser_id, url) => {
                    self.favicon = Some(url);
                }
                EmbedderMsg::HeadParsed(_browser_id, ) => {
                    self.loading_state = Some(LoadingState::Loading);
                }
                EmbedderMsg::HistoryChanged(_browser_id, entries, current) => {
                    self.current_url = Some(entries[current].url.clone());
                }
                EmbedderMsg::SetFullscreenState(_browser_id, state) => {
                    // FIXME
                    // match self.kind {
                    //     WindowKind::Window(ref window, ..) => {
                    //         if self.fullscreen.get() != state {
                    //             window.set_fullscreen(None);
                    //         }
                    //     },
                    //     WindowKind::Headless(..) => {}
                    // }
                    // self.fullscreen.set(state);
                }
                EmbedderMsg::LoadStart(_browser_id) => {
                    self.loading_state = Some(LoadingState::Connecting);
                }
                EmbedderMsg::LoadComplete(_browser_id) => {
                    self.loading_state = Some(LoadingState::Loaded);
                }
                EmbedderMsg::Shutdown => {
                    self.shutdown_requested = true;
                },
                EmbedderMsg::Panic(_browser_id, _reason, _backtrace) => {
                }
            }
        }
    }

}
