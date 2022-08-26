use egui_backend::WindowBackend;
use egui_window_glfw_passthrough::GlfwWindow;

fn main() {
    let _ = GlfwWindow::new(Default::default(), egui_backend::GfxApiConfig::Vulkan {  });

}