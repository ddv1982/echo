use echo_core::History;
use gtk::prelude::*;

pub fn print_history(store: &History) {
    if store.rows().is_empty() {
        println!("(empty)");
        return;
    }
    for row in store.rows() {
        println!("{}  {}  {}  {}", row.id, row.engine, row.infer_ms, row.text);
    }
}

pub fn build_window() -> (gtk::Window, gtk::ListBox) {
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Echo History");
    window.set_default_size(560, 400);
    window.connect_delete_event(|window, _| {
        window.hide();
        glib::Propagation::Stop
    });

    let list = gtk::ListBox::new();
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.add(&list);
    window.add(&scroll);

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
    match History::load() {
        Ok(store) if store.rows().is_empty() => {
            list.add(&gtk::Label::new(Some("(empty)")));
        }
        Ok(store) => {
            for row in store.rows().iter().rev() {
                let text = format!("{}  {}  {}", row.id, row.engine, row.text);
                let label = gtk::Label::new(Some(&text));
                label.set_xalign(0.0);
                label.set_line_wrap(true);
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
