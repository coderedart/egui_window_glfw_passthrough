use egui_backend::{BackendSettings, WindowBackend};
use egui_window_glfw_passthrough::GlfwWindow;

fn main() {
    let _ = GlfwWindow::new(Default::default(), BackendSettings::default());
}
