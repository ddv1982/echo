use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use echo_desktop::ipc::{
    LanguageMode, LanguageOption, SettingSource, SettingsChange, SettingsSnapshot,
};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Emitter, Manager, Wry};

static NEXT_LANGUAGE_REQUEST: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LanguageMenuRequest(u64);

pub(crate) struct TrayMenu {
    // Tauri's Linux AppIndicator backend requires the menu owner to outlive setup.
    _menu: Menu<Wry>,
    language_menu: Submenu<Wry>,
    language_items: Vec<(String, CheckMenuItem<Wry>)>,
    language_state: Mutex<LanguageMenuState>,
    language_writes: Mutex<LanguageWriteQueue>,
}

pub(crate) fn build(app: &mut App) -> tauri::Result<TrayMenu> {
    let open = MenuItem::with_id(app, "open", "Open Echo", true, None::<&str>)?;
    let record = MenuItem::with_id(app, "record", "Start / stop recording", true, None::<&str>)?;
    let auto = CheckMenuItem::with_id(
        app,
        "language:auto",
        "Auto detect",
        false,
        false,
        None::<&str>,
    )?;
    let common_languages = ["en", "de", "es", "fr"]
        .into_iter()
        .filter_map(echo_core::Language::from_code)
        .map(|language| language_item(app, "language", language))
        .collect::<tauri::Result<Vec<_>>>()?;
    let mut languages = echo_core::Language::all().collect::<Vec<_>>();
    languages.sort_by_key(|language| language.english_name());
    let all_languages = languages
        .into_iter()
        .map(|language| language_item(app, "language-all", language))
        .collect::<tauri::Result<Vec<_>>>()?;
    let all_refs = all_languages
        .iter()
        .map(|(_, item)| item as &dyn IsMenuItem<Wry>)
        .collect::<Vec<_>>();
    let all_menu = Submenu::with_items(app, "All languages", true, &all_refs)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let mut language_refs: Vec<&dyn IsMenuItem<Wry>> = vec![&auto];
    language_refs.extend(
        common_languages
            .iter()
            .map(|(_, item)| item as &dyn IsMenuItem<Wry>),
    );
    language_refs.extend([&separator as &dyn IsMenuItem<Wry>, &all_menu]);
    let language_menu = Submenu::with_items(app, "Language", false, &language_refs)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &record, &language_menu, &quit])?;
    let icon = Image::from_bytes(include_bytes!("../icons/tray-24.png"))
        .expect("tray-24.png decodes as RGBA");
    let mut language_items = vec![("auto".to_string(), auto)];
    language_items.extend(common_languages);
    language_items.extend(all_languages);
    let tray_menu = TrayMenu {
        _menu: menu,
        language_menu,
        language_items,
        language_state: Mutex::new(LanguageMenuState {
            revision: LanguageMenuRevision::default(),
            projection: None,
        }),
        language_writes: Mutex::new(LanguageWriteQueue::default()),
    };
    TrayIconBuilder::new()
        .menu(&tray_menu._menu)
        .icon(icon)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            match id {
                "open" => crate::show_main_window(app),
                "record" => {
                    let _ = crate::commands::start_recording_thread();
                }
                "quit" => app.exit(0),
                _ => {
                    if let Some(value) = language_event_value(id) {
                        select_language(app, value);
                    }
                }
            }
        })
        .build(app)?;
    Ok(tray_menu)
}

fn language_item(
    app: &App,
    id_prefix: &str,
    language: echo_core::Language,
) -> tauri::Result<(String, CheckMenuItem<Wry>)> {
    let mut label = language.english_name().to_string();
    if let Some(first) = label.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    let code = language.code().to_string();
    let item = CheckMenuItem::with_id(
        app,
        format!("{id_prefix}:{code}"),
        label,
        false,
        false,
        None::<&str>,
    )?;
    Ok((code, item))
}

#[derive(Clone)]
struct LanguageMenuProjection {
    enabled: bool,
    selected: String,
    selectable: HashSet<String>,
}

struct LanguageMenuState {
    revision: LanguageMenuRevision,
    projection: Option<LanguageMenuProjection>,
}

struct LanguageWrite {
    request: LanguageMenuRequest,
    value: String,
}

#[derive(Default)]
struct LanguageWriteQueue {
    active: bool,
    pending: VecDeque<LanguageWrite>,
}

impl LanguageWriteQueue {
    fn enqueue(&mut self, write: LanguageWrite) -> bool {
        self.pending.push_back(write);
        if self.active {
            false
        } else {
            self.active = true;
            true
        }
    }

    fn next(&mut self) -> Option<LanguageWrite> {
        let next = self.pending.pop_front();
        if next.is_none() {
            self.active = false;
        }
        next
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct LanguageMenuRevision {
    settings: u64,
    request: u64,
}

impl LanguageMenuState {
    fn publish(
        &mut self,
        revision: LanguageMenuRevision,
        projection: &LanguageMenuProjection,
    ) -> bool {
        if revision < self.revision {
            return false;
        }
        self.revision = revision;
        self.projection = Some(projection.clone());
        true
    }
}

impl LanguageMenuProjection {
    fn item_enabled(&self, code: &str) -> bool {
        self.enabled && self.selectable.contains(code)
    }

    fn item_checked(&self, code: &str) -> bool {
        self.selected == code
    }
}

fn language_menu_projection(
    mode: LanguageMode,
    options: &[LanguageOption],
    effective: &str,
    source: SettingSource,
) -> LanguageMenuProjection {
    let mut selectable = HashSet::new();
    if mode == LanguageMode::Multilingual {
        selectable.insert("auto".to_string());
        selectable.extend(options.iter().map(|option| option.code.clone()));
    }
    let locked = source == SettingSource::Env;
    let selected = match mode {
        LanguageMode::English => "en",
        LanguageMode::Parakeet => "auto",
        LanguageMode::Multilingual => effective,
    };
    LanguageMenuProjection {
        enabled: mode == LanguageMode::Multilingual && !locked,
        selected: selected.to_string(),
        selectable,
    }
}

impl TrayMenu {
    fn apply(
        &self,
        revision: LanguageMenuRevision,
        projection: LanguageMenuProjection,
    ) -> Result<(), String> {
        {
            let mut state = self
                .language_state
                .lock()
                .map_err(|_| "tray language selection state is unavailable".to_string())?;
            if !state.publish(revision, &projection) {
                return Ok(());
            }
        }
        self.apply_projection(&projection)
            .map_err(|error| error.to_string())
    }

    fn apply_projection(&self, projection: &LanguageMenuProjection) -> tauri::Result<()> {
        self.language_menu.set_enabled(projection.enabled)?;
        for (code, item) in &self.language_items {
            item.set_enabled(projection.item_enabled(code))?;
            item.set_checked(projection.item_checked(code))?;
        }
        Ok(())
    }

    fn validate_selection(&self, value: &str) -> Result<(), String> {
        let current = self
            .language_state
            .lock()
            .map_err(|_| "tray language selection state is unavailable".to_string())?;
        let projection = current
            .projection
            .as_ref()
            .ok_or_else(|| "language settings are still loading".to_string())?;
        if !projection.item_enabled(value) {
            return Err(format!(
                "language {value} is unavailable for the active transcription engine"
            ));
        }
        Ok(())
    }

    fn restore(&self) {
        let projection = self
            .language_state
            .lock()
            .ok()
            .and_then(|current| current.projection.clone());
        if let Some(projection) = projection {
            if let Err(error) = self.apply_projection(&projection) {
                eprintln!("tray language: failed to restore menu: {error}");
            }
        }
    }

    fn enqueue_language_write(&self, write: LanguageWrite) -> Result<bool, String> {
        self.language_writes
            .lock()
            .map(|mut queue| queue.enqueue(write))
            .map_err(|_| "tray language write queue is unavailable".to_string())
    }

    fn next_language_write(&self) -> Result<Option<LanguageWrite>, String> {
        self.language_writes
            .lock()
            .map(|mut queue| queue.next())
            .map_err(|_| "tray language write queue is unavailable".to_string())
    }
}

pub(crate) fn request() -> LanguageMenuRequest {
    LanguageMenuRequest(NEXT_LANGUAGE_REQUEST.fetch_add(1, Ordering::SeqCst))
}

pub(crate) fn sync(
    app: &AppHandle,
    request: LanguageMenuRequest,
    revision: u64,
    snapshot: &SettingsSnapshot,
) {
    sync_requested(
        app,
        LanguageMenuRevision {
            settings: revision,
            request: request.0,
        },
        snapshot,
    );
}

fn sync_requested(app: &AppHandle, revision: LanguageMenuRevision, snapshot: &SettingsSnapshot) {
    let projection = language_menu_projection(
        snapshot.transcription.languages.mode,
        &snapshot.transcription.languages.options,
        &snapshot.preferences.language.effective,
        snapshot.preferences.language.source,
    );
    let app_for_update = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        let Some(menu) = app_for_update.try_state::<TrayMenu>() else {
            return;
        };
        if let Err(error) = menu.apply(revision, projection) {
            eprintln!("tray language: failed to update menu: {error}");
        }
    }) {
        eprintln!("tray language: failed to dispatch menu update: {error}");
    }
}

pub(crate) fn refresh(app: &AppHandle) {
    let request = request();
    refresh_requested(app, request);
}

pub(crate) fn refresh_requested(app: &AppHandle, request: LanguageMenuRequest) {
    let app = app.clone();
    let service = app.state::<crate::setup::SetupService>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let result = crate::blocking::run_blocking("tray language refresh", move || {
            crate::settings::snapshot_with_revision(|| service.snapshot())
        })
        .await
        .and_then(|result| result);
        match result {
            Ok((settings, snapshot)) => sync_requested(
                &app,
                LanguageMenuRevision {
                    settings,
                    request: request.0,
                },
                &snapshot,
            ),
            Err(error) => eprintln!("tray language: failed to read settings: {error}"),
        }
    });
}

fn language_event_value(id: &str) -> Option<String> {
    id.strip_prefix("language:")
        .or_else(|| id.strip_prefix("language-all:"))
        .filter(|value| echo_core::LanguageChoice::parse(value).is_some())
        .map(str::to_string)
}

fn select_language(app: &AppHandle, value: String) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };
    if let Err(error) = menu.validate_selection(&value) {
        eprintln!("tray language: {error}");
        restore(app);
        return;
    }
    let request = request();
    let start_worker = match menu.enqueue_language_write(LanguageWrite { request, value }) {
        Ok(start_worker) => start_worker,
        Err(error) => {
            eprintln!("tray language: {error}");
            restore(app);
            return;
        }
    };
    if start_worker {
        tauri::async_runtime::spawn(process_language_writes(app.clone()));
    }
}

async fn process_language_writes(app: AppHandle) {
    loop {
        let write = {
            let Some(menu) = app.try_state::<TrayMenu>() else {
                return;
            };
            match menu.next_language_write() {
                Ok(write) => write,
                Err(error) => {
                    eprintln!("tray language: {error}");
                    return;
                }
            }
        };
        let Some(write) = write else {
            return;
        };
        let service = app.state::<crate::setup::SetupService>().inner().clone();
        let value = write.value;
        let outcome = crate::blocking::run_blocking("tray language change", move || {
            crate::settings::change(SettingsChange::Language { value: Some(value) })?;
            crate::settings::snapshot_with_revision(|| service.snapshot())
        })
        .await
        .and_then(|result| result);
        match outcome {
            Ok((revision, snapshot)) => {
                sync(&app, write.request, revision, &snapshot);
                let _ = app.emit("settings-event", ());
            }
            Err(error) => {
                eprintln!("tray language: {error}");
                restore(&app);
            }
        }
    }
}

fn restore(app: &AppHandle) {
    let app_for_update = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        if let Some(menu) = app_for_update.try_state::<TrayMenu>() {
            menu.restore();
        }
    }) {
        eprintln!("tray language: failed to dispatch menu restoration: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use echo_desktop::ipc::{LanguageMode, LanguageOption, SettingSource};

    fn option(code: &str) -> LanguageOption {
        let language = echo_core::Language::from_code(code).unwrap();
        LanguageOption {
            code: code.to_string(),
            english_name: language.english_name().to_string(),
            group: echo_desktop::ipc::LanguageGroup::All,
        }
    }

    #[test]
    fn stale_revision_cannot_replace_the_newest_projection() {
        let mut state = LanguageMenuState {
            revision: LanguageMenuRevision::default(),
            projection: None,
        };
        let newer = language_menu_projection(
            LanguageMode::Multilingual,
            &[option("en"), option("de")],
            "de",
            SettingSource::File,
        );
        let stale = language_menu_projection(
            LanguageMode::Multilingual,
            &[option("en"), option("fr")],
            "fr",
            SettingSource::File,
        );

        assert!(state.publish(
            LanguageMenuRevision {
                settings: 4,
                request: 2,
            },
            &newer,
        ));
        assert!(!state.publish(
            LanguageMenuRevision {
                settings: 4,
                request: 1,
            },
            &stale,
        ));
        assert_eq!(state.revision.settings, 4);
        assert_eq!(state.revision.request, 2);
        assert!(state.projection.unwrap().item_checked("de"));
    }

    #[test]
    fn language_writes_are_dequeued_in_click_order() {
        let mut queue = LanguageWriteQueue::default();
        assert!(queue.enqueue(LanguageWrite {
            request: LanguageMenuRequest(1),
            value: "fr".to_string(),
        }));
        assert!(!queue.enqueue(LanguageWrite {
            request: LanguageMenuRequest(2),
            value: "de".to_string(),
        }));

        let first = queue.next().unwrap();
        let second = queue.next().unwrap();
        assert_eq!(
            (first.request, first.value.as_str()),
            (LanguageMenuRequest(1), "fr")
        );
        assert_eq!(
            (second.request, second.value.as_str()),
            (LanguageMenuRequest(2), "de")
        );
        assert!(queue.next().is_none());
        assert!(queue.enqueue(LanguageWrite {
            request: LanguageMenuRequest(3),
            value: "es".to_string(),
        }));
    }

    #[test]
    fn multilingual_projection_selects_and_enables_available_languages() {
        let projection = language_menu_projection(
            LanguageMode::Multilingual,
            &[option("en"), option("de")],
            "de",
            SettingSource::File,
        );

        assert!(projection.enabled);
        assert!(projection.item_enabled("auto"));
        assert!(projection.item_enabled("en"));
        assert!(projection.item_enabled("de"));
        assert!(!projection.item_enabled("fr"));
        assert!(projection.item_checked("de"));
        assert!(!projection.item_checked("auto"));
    }

    #[test]
    fn fixed_engine_modes_are_visible_but_not_selectable() {
        let english = language_menu_projection(
            LanguageMode::English,
            &[option("en")],
            "de",
            SettingSource::Default,
        );
        assert!(!english.enabled);
        assert!(english.item_checked("en"));
        assert!(!english.item_enabled("en"));

        let parakeet = language_menu_projection(
            LanguageMode::Parakeet,
            &[option("en"), option("de")],
            "de",
            SettingSource::Default,
        );
        assert!(!parakeet.enabled);
        assert!(parakeet.item_checked("auto"));
        assert!(!parakeet.item_enabled("auto"));
    }

    #[test]
    fn environment_language_is_shown_as_locked() {
        let projection = language_menu_projection(
            LanguageMode::Multilingual,
            &[option("en"), option("de")],
            "de",
            SettingSource::Env,
        );

        assert!(!projection.enabled);
        assert!(projection.item_checked("de"));
        assert!(!projection.item_enabled("de"));
    }
}
