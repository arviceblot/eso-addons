mod imp;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gio::Settings;
use glib::{clone, Object};
use gtk::glib::BindingFlags;
use gtk::{
    gio, glib, pango, Align, CheckButton, CustomFilter, Dialog, DialogFlags, Entry,
    FilterListModel, Label, ListBoxRow, NoSelection, ResponseType, SelectionMode,
};

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    pub fn new(app: &adw::Application) -> Self {
        // Create new window
        Object::builder().property("application", app).build()
    }

    pub fn content_box(&self) -> gtk::Box {
        self.imp().content_box.get()
    }

    pub fn view_stack(&self) -> adw::ViewStack {
        self.imp().view_stack.get()
    }

    // pub fn installed_view(&self) -> InstalledView {
    //     self.view_stack()
    //         .child_by_name("snapshot")
    //         .unwrap()
    //         .downcast()
    //         .unwrap()
    // }

    pub fn switcher_bar(&self) -> adw::ViewSwitcherBar {
        self.imp().switcher_bar.get()
    }

    // pub fn header_bar(&self) -> AppHeaderBar {
    //     self.imp().header_bar.get()
    // }

    fn setup_callbacks(&self) {
        self.set_stack();
    }

    fn set_stack(&self) {
        // self.imp().stack.set_visible_child_name("main");
        // self.imp().stack.set_visible_child_name("placeholder");
    }

    fn setup_installed_addons(&self) {}
}
