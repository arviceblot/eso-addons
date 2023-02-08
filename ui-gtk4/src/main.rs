mod config;
mod window;

use adw::prelude::*;
use gtk::gio;
use window::Window;

use config::APP_ID;

fn build_ui(app: &adw::Application) {
    // Create a new custom window and show it
    let window = Window::new(app);
    window.show();
}

fn main() {
    gio::resources_register_include!("ueam.gresource").expect("Failed to register resources.");

    // Create a new application
    let app = adw::Application::builder().application_id(APP_ID).build();

    // Connect to signals
    // app.connect_startup(setup_shortcuts);
    app.connect_activate(build_ui);

    // Run the application
    app.run();
}
