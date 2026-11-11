//! A track in a menu: a radio button whose arrow keys only move the focus,
//! on to the menu's next item. GTK's own, in a group, kept to the group and
//! picked each track on the way, switching subtitles at each key.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Choice;

    #[glib::object_subclass]
    impl ObjectSubclass for Choice {
        const NAME: &'static str = "JellyChoice";
        type Type = super::Choice;
        type ParentType = gtk::CheckButton;
    }

    impl ObjectImpl for Choice {}

    impl WidgetImpl for Choice {
        /// Take the focus, or let it go on (as most widgets do)
        fn focus(&self, _direction: gtk::DirectionType) -> bool {
            let choice = self.obj();
            !choice.is_focus() && choice.grab_focus()
        }
    }

    impl CheckButtonImpl for Choice {}
}

glib::wrapper! {
    pub struct Choice(ObjectSubclass<imp::Choice>)
        @extends gtk::CheckButton, gtk::Widget,
        @implements gtk::Accessible, gtk::Actionable, gtk::Buildable, gtk::ConstraintTarget;
}

impl Choice {
    pub fn new(label: &str, active: bool) -> Choice {
        glib::Object::builder()
            .property("label", label)
            .property("active", active)
            .build()
    }
}
