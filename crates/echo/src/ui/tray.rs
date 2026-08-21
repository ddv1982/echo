use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use echo_core::{status_path, AppCommand, SessionState};
use gtk::prelude::*;
use libappindicator::{AppIndicator, AppIndicatorStatus};

use crate::ui::{dictionary, history};

pub fn write_status(state: SessionState, last: Option<&str>) -> Result<(), String> {
    let path = status_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let name = match state {
        SessionState::Idle => "Idle",
        SessionState::Recording { .. } => "Recording",
        SessionState::Transcribing => "Transcribing",
        SessionState::Cleaning => "Cleaning",
        SessionState::Injecting => "Injecting",
        SessionState::Failed { reason } => {
            return write(&format!("Failed {}", reason.as_str()), last)
        }
    };
    write(name, last)
}

fn write(state: &str, last: Option<&str>) -> Result<(), String> {
    let mut body = format!("state={state}\n");
    if let Some(text) = last {
        body.push_str("last=");
        body.push_str(text);
        body.push('\n');
    }
    fs::write(status_path(), body).map_err(|err| err.to_string())
}

pub fn read_status() -> Result<String, String> {
    fs::read_to_string(status_path()).map_err(|err| err.to_string())
}

pub fn status_summary() -> String {
    match read_status() {
        Ok(text) => {
            let state = text
                .lines()
                .find_map(|line| line.strip_prefix("state="))
                .unwrap_or("Idle");
            format!("Echo: {state}")
        }
        Err(_) => "Echo: Idle".to_string(),
    }
}

pub fn load_icon() -> Result<gdk_pixbuf::Pixbuf, String> {
    let loader = gdk_pixbuf::PixbufLoader::with_type("png").map_err(|err| err.to_string())?;
    loader
        .write(crate::icon::PNG)
        .map_err(|err| err.to_string())?;
    loader.close().map_err(|err| err.to_string())?;
    loader
        .pixbuf()
        .ok_or_else(|| "embedded echo.png produced no pixbuf".to_string())
}

pub fn run_app(smoke: bool) -> Result<(), String> {
    gtk::init().map_err(|err| err.to_string())?;
    glib::set_prgname(Some("echo-app"));
    glib::set_application_name("Echo");
    let pixbuf = load_icon()?;
    gtk::Window::set_default_icon(&pixbuf);
    if smoke {
        return Ok(());
    }

    let _ = write_status(SessionState::Idle, None);
    let icon_file = install_runtime_icon()?;
    let theme_dir = icon_file
        .parent()
        .ok_or_else(|| "icon path has no parent directory".to_string())?;
    let theme_dir = theme_dir
        .to_str()
        .ok_or_else(|| "icon path is not utf-8".to_string())?;

    let (history_win, history_list) = history::build_window();
    let (dict_win, dict_list) = dictionary::build_window();

    let mut menu = gtk::Menu::new();
    let busy = Arc::new(AtomicBool::new(false));
    let rec_done = Arc::new(AtomicBool::new(false));
    for cmd in AppCommand::TRAY_MENU {
        let item = gtk::MenuItem::with_label(cmd.tray_label());
        let history_win = history_win.clone();
        let history_list = history_list.clone();
        let dict_win = dict_win.clone();
        let dict_list = dict_list.clone();
        let busy = Arc::clone(&busy);
        let rec_done = Arc::clone(&rec_done);
        item.connect_activate(move |_| match cmd {
            AppCommand::Quit => gtk::main_quit(),
            AppCommand::OpenHistory => {
                history::refresh_list(&history_list);
                history_win.present();
            }
            AppCommand::OpenDictionary => {
                dictionary::refresh_list(&dict_list);
                dict_win.present();
            }
            AppCommand::RecordOnce => start_record(&busy, &rec_done),
        });
        menu.append(&item);
    }

    let mut indicator = AppIndicator::new("echo-app", "echo");
    indicator.set_status(AppIndicatorStatus::Active);
    indicator.set_icon_theme_path(theme_dir);
    indicator.set_icon_full("echo", &status_summary());
    indicator.set_menu(&mut menu);
    menu.show_all();

    let indicator = Rc::new(RefCell::new(indicator));
    {
        let indicator = Rc::clone(&indicator);
        let last = Rc::new(RefCell::new(String::new()));
        glib::timeout_add_local(Duration::from_millis(400), move || {
            if rec_done.swap(false, Ordering::SeqCst) {
                busy.store(false, Ordering::SeqCst);
            }
            let tip = status_summary();
            if last.borrow().as_str() != tip {
                indicator.borrow_mut().set_icon_full("echo", &tip);
                *last.borrow_mut() = tip;
            }
            glib::ControlFlow::Continue
        });
    }

    gtk::main();
    Ok(())
}

fn start_record(busy: &Arc<AtomicBool>, rec_done: &Arc<AtomicBool>) {
    if busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let rec_done = Arc::clone(rec_done);
    std::thread::spawn(move || {
        let _ = crate::rec::run_rec_once();
        rec_done.store(true, Ordering::SeqCst);
    });
}

fn install_runtime_icon() -> Result<PathBuf, String> {
    let dir = echo_core::data_dir();
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join("echo.png");
    if fs::read(&path).ok().as_deref() != Some(crate::icon::PNG) {
        fs::write(&path, crate::icon::PNG).map_err(|err| err.to_string())?;
    }
    Ok(path)
}
