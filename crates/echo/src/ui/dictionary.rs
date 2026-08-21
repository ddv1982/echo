use echo_core::Dictionary;
use gtk::prelude::*;

pub fn build_window() -> (gtk::Window, gtk::ListBox) {
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Echo Dictionary");
    window.set_default_size(480, 360);
    window.connect_delete_event(|window, _| {
        window.hide();
        glib::Propagation::Stop
    });

    let list = gtk::ListBox::new();
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.add(&list);

    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Claude Code"));
    let add = gtk::Button::with_label("Add");
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    controls.pack_start(&entry, true, true, 0);
    controls.pack_start(&add, false, false, 0);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_top(8);
    root.set_margin_bottom(8);
    root.set_margin_start(8);
    root.set_margin_end(8);
    root.pack_start(&controls, false, false, 0);
    root.pack_start(&scroll, true, true, 0);
    window.add(&root);

    let entry_add = entry.clone();
    let list_add = list.clone();
    add.connect_clicked(move |_| {
        add_from_field(&entry_add, &list_add);
    });
    let list_enter = list.clone();
    entry.connect_activate(move |entry| {
        add_from_field(entry, &list_enter);
    });

    let list_refresh = list.clone();
    window.connect_show(move |_| {
        refresh_list(&list_refresh);
    });

    (window, list)
}

pub fn refresh_list(list: &gtk::ListBox) {
    for child in list.children() {
        list.remove(&child);
    }
    match Dictionary::load() {
        Ok(store) if store.entries().is_empty() => {
            list.add(&gtk::Label::new(Some(
                "No dictionary entries yet. Add a preferred spelling above.",
            )));
        }
        Ok(store) => {
            for entry in store.entries() {
                let text = format!("{} -> {}", entry.spoken, entry.written);
                let label = gtk::Label::new(Some(&text));
                label.set_xalign(0.0);
                label.set_selectable(true);
                list.add(&label);
            }
        }
        Err(err) => {
            list.add(&gtk::Label::new(Some(&err)));
        }
    }
    list.show_all();
}

fn add_from_field(entry: &gtk::Entry, list: &gtk::ListBox) {
    let written = entry.text().trim().to_string();
    if written.is_empty() {
        return;
    }
    let spoken = written.to_ascii_lowercase();
    match Dictionary::load().and_then(|mut store| store.add(spoken, written)) {
        Ok(_) => {
            entry.set_text("");
            refresh_list(list);
        }
        Err(err) => eprintln!("dict: {err}"),
    }
}
