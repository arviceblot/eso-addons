use std::cell::RefCell;
use std::fs::File;

use adw::subclass::prelude::*;

use gio::Settings;
use glib::signal::Inhibit;
use glib::subclass::InitializingObject;

use adw::prelude::*;
use gtk::{gio, glib, CompositeTemplate, Entry, ListBox, Stack};

use crate::config::APP_ID;

#[derive(CompositeTemplate)]
#[template(resource = "/com/arviceblot/ueam/window.ui")]
pub struct Window {
    #[template_child]
    pub content_box: TemplateChild<gtk::Box>,
    #[template_child]
    pub view_stack: TemplateChild<adw::ViewStack>,
    #[template_child]
    pub switcher_bar: TemplateChild<adw::ViewSwitcherBar>,
    // #[template_child]
    // pub header_bar: TemplateChild<AppHeaderBar>,
    pub settings: gio::Settings,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            content_box: TemplateChild::default(),
            view_stack: TemplateChild::default(),
            switcher_bar: TemplateChild::default(),
            // header_bar: TemplateChild::default(),
            settings: gio::Settings::new(APP_ID),
        }
    }
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for Window {
    // `NAME` needs to match `class` attribute of template
    const NAME: &'static str = "UeamWindow";
    type Type = super::Window;
    type ParentType = adw::ApplicationWindow;

    fn class_init(klass: &mut Self::Class) {
        klass.bind_template();
    }

    fn instance_init(obj: &InitializingObject<Self>) {
        obj.init_template();
    }
}

// Trait shared by all GObjects
impl ObjectImpl for Window {
    fn constructed(&self) {
        // Call "constructed" on parent
        self.parent_constructed();

        // Setup
        let obj = self.obj();
        // obj.setup_settings();
        obj.setup_callbacks();
        obj.setup_installed_addons();

        let new_action = gio::SimpleAction::new("new", None);

        let win = obj.clone();
        new_action.connect_activate(move |_, _| {
            let imp = win.imp();
            let cur_view = imp.view_stack.visible_child_name().unwrap();
            if cur_view == "installed" {
                let view = win.installed_view();
                view.present_creation_window();
            }
        });

        obj.add_action(&new_action);

        // let header_bar = self.header_bar.get();
        // self.view_stack.connect_visible_child_name_notify(
        //     glib::clone!(@weak header_bar => move |vs| {
        //         if let Some(view) = vs.visible_child_name() {
        //             match view.as_str() {
        //                 "installed" => {
        //                     header_bar.set_property("title-start", "add");
        //                     header_bar.set_property("title-end", "fs");
        //                 }
        //                 "search" => {
        //                     header_bar.set_property("title-start", "none");
        //                     header_bar.set_property("title-end", "switch");
        //                 }
        //                 _ => unimplemented!(),
        //             }
        //         }
        //     }),
        // );
    }
}

// Trait shared by all widgets
impl WidgetImpl for Window {}

// Trait shared by all windows
impl WindowImpl for Window {
    fn close_request(&self) -> Inhibit {
        // Pass close request on to the parent
        self.parent_close_request()
    }
}

// Trait shared by all application windows
impl ApplicationWindowImpl for Window {}

// Trait shared by all adwaita application windows
impl AdwApplicationWindowImpl for Window {}
