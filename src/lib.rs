use egui::{Event, Key, PointerButton, Pos2, RawInput};
use egui_backend::raw_window_handle::HasRawWindowHandle;
use egui_backend::GfxApiConfig;
use egui_backend::WindowBackend;
use egui_backend::*;
use glfw::Action;
use glfw::ClientApiHint;
use glfw::Context;
use glfw::Glfw;
use glfw::RenderContext;
use glfw::WindowEvent;
use std::sync::mpsc::Receiver;

pub use glfw;

pub struct GlfwWindow {
    pub glfw: glfw::Glfw,
    pub events_receiver: Receiver<(f64, WindowEvent)>,
    pub window: glfw::Window,
    pub size_physical_pixels: [u32; 2],
    pub scale: f32,
    pub cursor_pos_physical_pixels: [f32; 2],
    pub raw_input: RawInput,
    pub frame_events: Vec<WindowEvent>,
    pub resized_event_pending: bool,
}

unsafe impl HasRawWindowHandle for GlfwWindow {
    fn raw_window_handle(&self) -> raw_window_handle::RawWindowHandle {
        self.window.raw_window_handle()
    }
}

pub struct GlfwOpenGLWindowContext {
    pub render_context: RenderContext,
    pub glfw_context: Glfw,
}

impl OpenGLWindowContext for GlfwOpenGLWindowContext {
    fn swap_buffers(&mut self) {
        self.render_context.swap_buffers()
    }

    fn make_context_current(&mut self) {
        self.render_context.make_current();
    }

    fn is_current(&mut self) -> bool {
        self.render_context.is_current()
    }

    fn get_proc_address(&mut self, symbol: &str) -> *const core::ffi::c_void {
        self.glfw_context.get_proc_address_raw(symbol)
    }
}

/// The configuration struct for Glfw Backend
///
#[derive(Default)]
pub struct GlfwConfig {
    /// This callback is called with `&mut Glfw` right after `Glfw` is created
    pub glfw_callback: Option<Box<dyn FnOnce(&mut Glfw)>>,
}
impl WindowBackend for GlfwWindow {
    type Configuration = GlfwConfig;

    fn new(config: Self::Configuration, gfx_api_config: GfxApiConfig) -> (Self, WindowInfoForGfx) {
        let mut glfw_context =
            glfw::init(glfw::FAIL_ON_ERRORS).expect("failed to create glfw context");
        if let Some(glfw_callback) = config.glfw_callback {
            glfw_callback(&mut glfw_context);
        }
        // set hints based on gfx api config
        match gfx_api_config {
            GfxApiConfig::OpenGL {
                version,
                samples,
                srgb,
                transparent,
                debug,
            } => {
                if let Some(version) = version {
                    glfw_context.window_hint(glfw::WindowHint::ContextVersion(
                        version.0.into(),
                        version.1.into(),
                    ));
                }

                if let Some(transparent) = transparent {
                    glfw_context.window_hint(glfw::WindowHint::TransparentFramebuffer(transparent));
                }
                if let Some(debug) = debug {
                    glfw_context.window_hint(glfw::WindowHint::OpenGlDebugContext(debug));
                }
                if let Some(srgb) = srgb {
                    glfw_context.window_hint(glfw::WindowHint::SRgbCapable(srgb));
                }
                if let Some(samples) = samples {
                    glfw_context.window_hint(glfw::WindowHint::Samples(Some(samples as u32)));
                }
            }
            GfxApiConfig::Vulkan { .. } => {
                glfw_context.window_hint(glfw::WindowHint::ClientApi(ClientApiHint::NoApi));
            }
            _ => panic!("gfx api type not recognized"),
        }
        // hints for passthrough and overlay features
        glfw_context.window_hint(glfw::WindowHint::TransparentFramebuffer(true));
        glfw_context.window_hint(glfw::WindowHint::MousePassthrough(true));
        glfw_context.window_hint(glfw::WindowHint::Floating(true));

        // create a window
        let (mut window, events_receiver) = glfw_context
            .create_window(800, 600, "Overlay Window", glfw::WindowMode::Windowed)
            .expect("failed to create glfw window");
        // set which events you care about
        window.set_all_polling(true);
        window.set_store_lock_key_mods(true);
        // collect details and keep them updated
        let (width, height) = window.get_framebuffer_size();
        let scale = window.get_content_scale().0;
        let cursor_position = window.get_cursor_pos();
        let size_physical_pixels = [
            width.try_into().expect("width not fit in u32"),
            height.try_into().expect("height not fit in u32"),
        ];
        let raw_input = RawInput {
            // set raw input screen rect details so that first frame
            // will have correct size even without any resize event
            screen_rect: Some(egui::Rect::from_points(&[
                Default::default(),
                [width as f32, height as f32].into(),
            ])),
            pixels_per_point: Some(scale),
            ..Default::default()
        };
        // collect opengl window context stuff to give to renderers
        let opengl_context: Option<Box<dyn OpenGLWindowContext>> = match gfx_api_config {
            GfxApiConfig::OpenGL { .. } => Some(Box::new(GlfwOpenGLWindowContext {
                render_context: window.render_context(),
                glfw_context: glfw_context.clone(),
            })),
            GfxApiConfig::Vulkan {} => None,
            _ => unimplemented!("don't have this gfx api backend implemented yet in glfw"),
        };
        // create info needed for gfx renderers
        let window_info_for_gfx = WindowInfoForGfx {
            gfx_api_config,
            window_handle: window.raw_window_handle(),
            opengl_context,
        };
        (
            Self {
                glfw: glfw_context,
                events_receiver,
                window,
                size_physical_pixels,
                scale,
                cursor_pos_physical_pixels: [cursor_position.0 as f32, cursor_position.1 as f32],
                raw_input,
                frame_events: vec![],
                resized_event_pending: true, // provide so that on first prepare frame, renderers can set their viewport sizes
            },
            window_info_for_gfx,
        )
    }

    fn take_raw_input(&mut self) -> RawInput {
        self.raw_input.take()
    }

    fn take_latest_size_update(&mut self) -> Option<[u32; 2]> {
        if self.resized_event_pending {
            self.resized_event_pending = false;
            let (w, h) = self.window.get_framebuffer_size();
            self.size_physical_pixels = [w as u32, h as u32];
            Some(self.size_physical_pixels)
        } else {
            None
        }
    }

    fn run_event_loop<G: GfxBackend, U: UserApp<Self, G>>(
        mut self,
        mut gfx_backend: G,
        mut user_app: U,
    ) {
        let egui_context = egui::Context::default();
        while !self.window.should_close() {
            // gather events
            self.tick();
            // take egui input
            let input = self.take_raw_input();
            // take any frambuffer resize events
            let fb_size_update = self.take_latest_size_update();
            // prepare surface for drawing
            gfx_backend.prepare_frame(fb_size_update, &mut self);
            // begin egui with input
            egui_context.begin_frame(input);
            // run userapp gui function. let user do anything he wants with window or gfx backends
            user_app.run(&egui_context, &mut self, &mut gfx_backend);
            // end frame
            let output = egui_context.end_frame();
            // prepare egui render data for gfx backend
            let gfx_output = EguiGfxOutput {
                meshes: egui_context.tessellate(output.shapes),
                textures_delta: output.textures_delta,
                screen_size_logical: [
                    self.size_physical_pixels[0] as f32 / self.scale,
                    self.size_physical_pixels[1] as f32 / self.scale,
                ],
                framebuffer_size_physical: self.size_physical_pixels,
                scale: self.scale,
            };
            // render egui with gfx backend
            gfx_backend.prepare_render(gfx_output);
            gfx_backend.render();
            // present the frame and loop back
            gfx_backend.present();
        }
    }

    fn get_live_physical_size_framebuffer(&mut self) -> [u32; 2] {
        let physical_fb_size = self.window.get_framebuffer_size();
        self.size_physical_pixels = [physical_fb_size.0 as u32, physical_fb_size.1 as u32];
        self.size_physical_pixels
    }
}

impl GlfwWindow {
    pub fn tick(&mut self) {
        self.glfw.poll_events();
        self.frame_events.clear();
        let cursor_position = self.window.get_cursor_pos();
        let cursor_position = [cursor_position.0 as f32, cursor_position.1 as f32];
        self.cursor_pos_physical_pixels = cursor_position;
        // when we are passthorugh, we use this to get latest position
        if cursor_position != self.cursor_pos_physical_pixels && self.window.is_mouse_passthrough()
        {
            self.cursor_pos_physical_pixels = cursor_position;
            self.raw_input.events.push(Event::PointerMoved(
                [
                    cursor_position[0] / self.scale,
                    cursor_position[1] / self.scale,
                ]
                .into(),
            ))
        }

        for (_, event) in glfw::flush_messages(&self.events_receiver) {
            // if let &glfw::WindowEvent::CursorPos(..) = &event {
            //     continue;
            // }
            self.frame_events.push(event.clone());
            if let Some(ev) = match event {
                glfw::WindowEvent::FramebufferSize(w, h) => {
                    self.size_physical_pixels = [w as u32, h as u32];
                    self.resized_event_pending = true;
                    self.raw_input.screen_rect = Some(egui::Rect::from_two_pos(
                        Default::default(),
                        [w as f32 / self.scale, h as f32 / self.scale].into(),
                    ));

                    None
                }
                glfw::WindowEvent::MouseButton(mb, a, m) => {
                    let emb = Event::PointerButton {
                        pos: Pos2 {
                            x: cursor_position[0] / self.scale,
                            y: cursor_position[1] / self.scale,
                        },
                        button: glfw_to_egui_pointer_button(mb),
                        pressed: glfw_to_egui_action(a),
                        modifiers: glfw_to_egui_modifers(m),
                    };
                    Some(emb)
                }
                // we scroll 25 pixels at a time
                glfw::WindowEvent::Scroll(x, y) => {
                    Some(Event::Scroll([x as f32 * 25.0, y as f32 * 25.0].into()))
                }
                glfw::WindowEvent::Key(k, _, a, m) => match k {
                    glfw::Key::C => {
                        if glfw_to_egui_action(a) && m.contains(glfw::Modifiers::Control) {
                            Some(Event::Copy)
                        } else {
                            None
                        }
                    }
                    glfw::Key::X => {
                        if glfw_to_egui_action(a) && m.contains(glfw::Modifiers::Control) {
                            Some(Event::Cut)
                        } else {
                            None
                        }
                    }
                    glfw::Key::V => {
                        if glfw_to_egui_action(a) && m.contains(glfw::Modifiers::Control) {
                            Some(Event::Text(
                                self.window.get_clipboard_string().unwrap_or_default(),
                            ))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
                .or_else(|| {
                    glfw_to_egui_key(k).map(|key| Event::Key {
                        key,
                        pressed: glfw_to_egui_action(a),
                        modifiers: glfw_to_egui_modifers(m),
                    })
                }),
                glfw::WindowEvent::Char(c) => Some(Event::Text(c.to_string())),
                glfw::WindowEvent::ContentScale(x, _) => {
                    self.raw_input.pixels_per_point = Some(x);
                    self.scale = x;
                    None
                }
                glfw::WindowEvent::Close => {
                    self.window.set_should_close(true);
                    None
                }

                glfw::WindowEvent::FileDrop(f) => {
                    self.raw_input
                        .dropped_files
                        .extend(f.into_iter().map(|p| egui::DroppedFile {
                            path: Some(p),
                            name: "".to_string(),
                            last_modified: None,
                            bytes: None,
                        }));
                    None
                }
                glfw::WindowEvent::CursorPos(x, y) => {
                    Some(egui::Event::PointerMoved([x as f32, y as f32].into()))
                }
                _rest => None,
            } {
                self.raw_input.events.push(ev);
            }
        }
    }
}

/// a function to get the matching egui key event for a given glfw key. egui does not support all the keys provided here.
fn glfw_to_egui_key(key: glfw::Key) -> Option<Key> {
    match key {
        glfw::Key::Space => Some(Key::Space),
        glfw::Key::Num0 => Some(Key::Num0),
        glfw::Key::Num1 => Some(Key::Num1),
        glfw::Key::Num2 => Some(Key::Num2),
        glfw::Key::Num3 => Some(Key::Num3),
        glfw::Key::Num4 => Some(Key::Num4),
        glfw::Key::Num5 => Some(Key::Num5),
        glfw::Key::Num6 => Some(Key::Num6),
        glfw::Key::Num7 => Some(Key::Num7),
        glfw::Key::Num8 => Some(Key::Num8),
        glfw::Key::Num9 => Some(Key::Num9),
        glfw::Key::A => Some(Key::A),
        glfw::Key::B => Some(Key::B),
        glfw::Key::C => Some(Key::C),
        glfw::Key::D => Some(Key::D),
        glfw::Key::E => Some(Key::E),
        glfw::Key::F => Some(Key::F),
        glfw::Key::G => Some(Key::G),
        glfw::Key::H => Some(Key::H),
        glfw::Key::I => Some(Key::I),
        glfw::Key::J => Some(Key::J),
        glfw::Key::K => Some(Key::K),
        glfw::Key::L => Some(Key::L),
        glfw::Key::M => Some(Key::M),
        glfw::Key::N => Some(Key::N),
        glfw::Key::O => Some(Key::O),
        glfw::Key::P => Some(Key::P),
        glfw::Key::Q => Some(Key::Q),
        glfw::Key::R => Some(Key::R),
        glfw::Key::S => Some(Key::S),
        glfw::Key::T => Some(Key::T),
        glfw::Key::U => Some(Key::U),
        glfw::Key::V => Some(Key::V),
        glfw::Key::W => Some(Key::W),
        glfw::Key::X => Some(Key::X),
        glfw::Key::Y => Some(Key::Y),
        glfw::Key::Z => Some(Key::Z),
        glfw::Key::Escape => Some(Key::Escape),
        glfw::Key::Enter => Some(Key::Enter),
        glfw::Key::Tab => Some(Key::Tab),
        glfw::Key::Backspace => Some(Key::Backspace),
        glfw::Key::Insert => Some(Key::Insert),
        glfw::Key::Delete => Some(Key::Delete),
        glfw::Key::Right => Some(Key::ArrowRight),
        glfw::Key::Left => Some(Key::ArrowLeft),
        glfw::Key::Down => Some(Key::ArrowDown),
        glfw::Key::Up => Some(Key::ArrowUp),
        glfw::Key::PageUp => Some(Key::PageUp),
        glfw::Key::PageDown => Some(Key::PageDown),
        glfw::Key::Home => Some(Key::Home),
        glfw::Key::End => Some(Key::End),
        _ => None,
    }
}

pub fn glfw_to_egui_modifers(modifiers: glfw::Modifiers) -> egui::Modifiers {
    egui::Modifiers {
        alt: modifiers.contains(glfw::Modifiers::Alt),
        ctrl: modifiers.contains(glfw::Modifiers::Control),
        shift: modifiers.contains(glfw::Modifiers::Shift),
        mac_cmd: false,
        command: modifiers.contains(glfw::Modifiers::Control),
    }
}

pub fn glfw_to_egui_pointer_button(mb: glfw::MouseButton) -> PointerButton {
    match mb {
        glfw::MouseButton::Button1 => PointerButton::Primary,
        glfw::MouseButton::Button2 => PointerButton::Secondary,
        glfw::MouseButton::Button3 => PointerButton::Middle,
        _ => PointerButton::Secondary,
    }
}

pub fn glfw_to_egui_action(a: glfw::Action) -> bool {
    match a {
        Action::Release => false,
        Action::Press => true,
        Action::Repeat => true,
    }
}
