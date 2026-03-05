use anyhow::{anyhow, Context, Result};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, BufWriter, IsTerminal, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{MenuBuilder, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, SubmenuBuilder},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use walkdir::WalkDir;

const MARKDOWN_EXTENSIONS: [&str; 5] = ["md", "markdown", "mdown", "mkd", "mdx"];
const MAX_RECENT_FILES: usize = 15;
const RECENTS_STORAGE_FILE: &str = "recent-files.json";

const MENU_ID_FILE_OPEN: &str = "file.open";
const MENU_ID_FILE_OPEN_RECENT_PREFIX: &str = "file.open_recent.";
const MENU_ID_FILE_NO_RECENTS: &str = "file.open_recent.none";
const MENU_ID_EDIT_FIND: &str = "edit.find";
const WINDOW_USAGE: &str = "Usage:
  basalt windows list [--json]
  basalt windows close <path>
  basalt windows close --path <path>
  basalt windows close --label <window-label>";

#[derive(Default)]
struct AppState {
    documents: Mutex<HashMap<String, PathBuf>>,
    recents: Mutex<Vec<PathBuf>>,
    next_window: AtomicU64,
    pending_open: Mutex<Vec<PathBuf>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadedDocument {
    path: String,
    file_name: String,
    content: String,
    is_markdown: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileChangedEvent {
    path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecentFileEntry {
    path: String,
    file_name: String,
    available: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowDescriptor {
    label: String,
    path: String,
    title: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
enum ControlRequest {
    ListWindows,
    CloseByPath { path: String },
    CloseByLabel { label: String },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlResponse {
    ok: bool,
    message: String,
    windows: Vec<WindowDescriptor>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlEndpoint {
    host: String,
    port: u16,
}

enum WindowCliCommand {
    List { json: bool },
    CloseByPath { path: String },
    CloseByLabel { label: String },
}

struct CloseOutcome {
    closed: Vec<WindowDescriptor>,
    failures: Vec<String>,
}

#[tauri::command]
fn load_document(
    window: WebviewWindow,
    state: tauri::State<AppState>,
) -> Result<LoadedDocument, String> {
    let path = active_document_for_window(&window, state.inner())?;
    let content = read_document_text(&path)
        .map_err(|error| format!("Failed to read '{}': {error}", path_display(&path)))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "Untitled".to_string());

    Ok(LoadedDocument {
        path: path_display(&path),
        file_name,
        content,
        is_markdown: is_markdown_file(&path),
    })
}

#[tauri::command]
fn list_recent_files(state: tauri::State<AppState>) -> Result<Vec<RecentFileEntry>, String> {
    recent_entries(state.inner())
}

#[tauri::command]
fn open_document_dialog(
    window: WebviewWindow,
    app: AppHandle,
    state: tauri::State<AppState>,
) -> Result<Option<String>, String> {
    open_document_dialog_for_window(&window, &app, state.inner())
}

#[tauri::command]
fn open_document_path(
    window: WebviewWindow,
    path: String,
    app: AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let target = normalize_document_path(PathBuf::from(path))?;
    assign_document_to_window(&app, state.inner(), window.label(), target)
}

#[tauri::command]
fn resolve_references(
    window: WebviewWindow,
    references: Vec<String>,
    state: tauri::State<AppState>,
) -> Result<HashMap<String, Option<String>>, String> {
    let document = active_document_for_window(&window, state.inner())?;
    let mut resolved = HashMap::with_capacity(references.len());

    for reference in references {
        let target =
            resolve_reference_from_document(&document, &reference).map(|path| path_display(&path));
        resolved.insert(reference, target);
    }

    Ok(resolved)
}

#[tauri::command]
fn open_reference(
    window: WebviewWindow,
    reference: String,
    app: AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let document = active_document_for_window(&window, state.inner())?;
    let Some(target) = resolve_reference_from_document(&document, &reference) else {
        return Ok(());
    };

    if target.is_dir() {
        let targets = collect_files_recursively(&target);
        if targets.is_empty() {
            return Ok(());
        }
        open_targets(&app, state.inner(), targets, false)?;
    } else {
        open_targets(&app, state.inner(), vec![target], false)?;
    }

    Ok(())
}

#[tauri::command]
fn open_in_vscode(window: WebviewWindow, state: tauri::State<AppState>) -> Result<(), String> {
    let path = active_document_for_window(&window, state.inner())?;
    Command::new("code")
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            format!(
                "Unable to launch VS Code for '{}': {error}. Ensure `code` is available in PATH.",
                path_display(&path)
            )
        })?;

    Ok(())
}

fn active_document_for_window(window: &WebviewWindow, state: &AppState) -> Result<PathBuf, String> {
    let guard = state
        .documents
        .lock()
        .map_err(|_| "App state is unavailable right now.".to_string())?;

    guard
        .get(window.label())
        .cloned()
        .ok_or_else(|| "This window is not bound to a document yet.".to_string())
}

fn open_document_dialog_for_window(
    window: &WebviewWindow,
    app: &AppHandle,
    state: &AppState,
) -> Result<Option<String>, String> {
    let current_document = active_document_for_window(window, state).ok();

    let mut dialog = rfd::FileDialog::new().add_filter("Markdown", &MARKDOWN_EXTENSIONS);

    if let Some(path) = current_document {
        if let Some(parent) = path.parent() {
            dialog = dialog.set_directory(parent);
        }

        if let Some(file_name) = path.file_name().and_then(|value| value.to_str()) {
            dialog = dialog.set_file_name(file_name);
        }
    }

    let Some(chosen) = dialog.pick_file() else {
        return Ok(None);
    };

    let normalized = normalize_document_path(chosen)?;
    assign_document_to_window(app, state, window.label(), normalized.clone())?;

    Ok(Some(path_display(&normalized)))
}

fn open_recent_file_for_window(
    app: &AppHandle,
    state: &AppState,
    window: &WebviewWindow,
    recent_index: usize,
) -> Result<(), String> {
    let candidate = {
        let recents = state
            .recents
            .lock()
            .map_err(|_| "App state is unavailable right now.".to_string())?;
        recents.get(recent_index).cloned()
    }
    .ok_or_else(|| "That recent file entry is no longer available.".to_string())?;

    match normalize_document_path(candidate.clone()) {
        Ok(normalized) => assign_document_to_window(app, state, window.label(), normalized),
        Err(error) => {
            remove_recent_file_by_path(state, &candidate)?;
            save_recent_files_to_disk(app, state)?;
            refresh_app_menu(app, state)?;
            Err(error)
        }
    }
}

fn normalize_document_path(path: PathBuf) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path_display(&path)));
    }

    if !path.is_file() {
        return Err(format!("Path is not a file: {}", path_display(&path)));
    }

    Ok(fs::canonicalize(&path).unwrap_or(path))
}

fn remember_recent_file(state: &AppState, path: &Path) -> Result<(), String> {
    let mut recents = state
        .recents
        .lock()
        .map_err(|_| "App state is unavailable right now.".to_string())?;

    let normalized = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let normalized_key = path_key(&normalized);

    recents.retain(|entry| path_key(entry) != normalized_key);
    recents.insert(0, normalized);
    recents.truncate(MAX_RECENT_FILES);

    Ok(())
}

fn remove_recent_file_by_path(state: &AppState, path: &Path) -> Result<(), String> {
    let mut recents = state
        .recents
        .lock()
        .map_err(|_| "App state is unavailable right now.".to_string())?;

    let key = path_key(path);
    recents.retain(|entry| path_key(entry) != key);
    Ok(())
}

fn recent_paths_snapshot(state: &AppState) -> Result<Vec<PathBuf>, String> {
    let recents = state
        .recents
        .lock()
        .map_err(|_| "App state is unavailable right now.".to_string())?;
    Ok(recents.clone())
}

fn recent_entries(state: &AppState) -> Result<Vec<RecentFileEntry>, String> {
    let recents = recent_paths_snapshot(state)?;

    Ok(recents
        .into_iter()
        .map(|path| {
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| "Untitled.md".to_string());

            RecentFileEntry {
                path: path_display(&path),
                file_name,
                available: path.exists() && path.is_file(),
            }
        })
        .collect())
}

fn recents_storage_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|directory| directory.join(RECENTS_STORAGE_FILE))
}

fn load_recent_files_from_disk(app: &AppHandle) -> Vec<PathBuf> {
    let Some(path) = recents_storage_path(app) else {
        return Vec::new();
    };

    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let parsed: Vec<String> = serde_json::from_str(&content).unwrap_or_default();
    let mut seen = HashSet::new();
    let mut recents = Vec::new();

    for entry in parsed {
        if entry.trim().is_empty() {
            continue;
        }

        let candidate = PathBuf::from(entry);
        let normalized = fs::canonicalize(&candidate).unwrap_or(candidate);
        let key = path_key(&normalized);

        if seen.insert(key) {
            recents.push(normalized);
        }

        if recents.len() >= MAX_RECENT_FILES {
            break;
        }
    }

    recents
}

fn save_recent_files_to_disk(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let Some(path) = recents_storage_path(app) else {
        return Ok(());
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create config directory: {error}"))?;
    }

    let payload = recent_paths_snapshot(state)?
        .into_iter()
        .map(|entry| path_display(&entry))
        .collect::<Vec<_>>();

    let serialized =
        serde_json::to_string_pretty(&payload).map_err(|error| format!("Failed to encode recents: {error}"))?;

    fs::write(path, serialized).map_err(|error| format!("Failed to save recent files: {error}"))
}

fn recent_menu_label(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Untitled.md");

    let parent = path
        .parent()
        .map(path_display)
        .unwrap_or_else(|| "/".to_string());

    format!("{name} ({parent})")
}

fn build_open_recent_submenu(
    app: &AppHandle,
    recents: &[PathBuf],
) -> Result<Submenu<tauri::Wry>, String> {
    let mut builder = SubmenuBuilder::new(app, "Open Recent");

    if recents.is_empty() {
        let empty_item = MenuItem::with_id(
            app,
            MENU_ID_FILE_NO_RECENTS,
            "No Recent Files",
            false,
            None::<&str>,
        )
        .map_err(|error| format!("Failed to build recent menu item: {error}"))?;
        builder = builder.item(&empty_item);
    } else {
        for (index, path) in recents.iter().enumerate() {
            builder = builder.text(
                format!("{MENU_ID_FILE_OPEN_RECENT_PREFIX}{index}"),
                recent_menu_label(path),
            );
        }
    }

    builder
        .build()
        .map_err(|error| format!("Failed to build Open Recent submenu: {error}"))
}

fn build_file_submenu(app: &AppHandle, recents: &[PathBuf]) -> Result<Submenu<tauri::Wry>, String> {
    let open_item = MenuItem::with_id(app, MENU_ID_FILE_OPEN, "Open...", true, Some("CmdOrCtrl+O"))
        .map_err(|error| format!("Failed to build Open menu item: {error}"))?;
    let open_recent_submenu = build_open_recent_submenu(app, recents)?;
    let close_window_item = PredefinedMenuItem::close_window(app, None)
        .map_err(|error| format!("Failed to build Close Window menu item: {error}"))?;

    let base_builder = SubmenuBuilder::new(app, "File")
        .item(&open_item)
        .item(&open_recent_submenu)
        .separator()
        .item(&close_window_item);

    #[cfg(not(target_os = "macos"))]
    let builder = {
        let quit_item = PredefinedMenuItem::quit(app, None)
            .map_err(|error| format!("Failed to build Quit menu item: {error}"))?;
        base_builder.separator().item(&quit_item)
    };

    #[cfg(target_os = "macos")]
    let builder = base_builder;

    builder
        .build()
        .map_err(|error| format!("Failed to build File submenu: {error}"))
}

fn build_edit_submenu(app: &AppHandle) -> Result<Submenu<tauri::Wry>, String> {
    let find_item = MenuItem::with_id(app, MENU_ID_EDIT_FIND, "Find...", true, Some("CmdOrCtrl+F"))
        .map_err(|error| format!("Failed to build Find menu item: {error}"))?;

    SubmenuBuilder::new(app, "Edit")
        .item(&find_item)
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()
        .map_err(|error| format!("Failed to build Edit submenu: {error}"))
}

#[cfg(target_os = "macos")]
fn build_macos_app_submenu(app: &AppHandle) -> Result<Submenu<tauri::Wry>, String> {
    let package_name = app.package_info().name.clone();

    let about_item = PredefinedMenuItem::about(app, None, None)
        .map_err(|error| format!("Failed to build About menu item: {error}"))?;
    let services_item = PredefinedMenuItem::services(app, None)
        .map_err(|error| format!("Failed to build Services menu item: {error}"))?;
    let hide_item = PredefinedMenuItem::hide(app, None)
        .map_err(|error| format!("Failed to build Hide menu item: {error}"))?;
    let hide_others_item = PredefinedMenuItem::hide_others(app, None)
        .map_err(|error| format!("Failed to build Hide Others menu item: {error}"))?;
    let quit_item = PredefinedMenuItem::quit(app, None)
        .map_err(|error| format!("Failed to build Quit menu item: {error}"))?;

    SubmenuBuilder::new(app, package_name)
        .item(&about_item)
        .separator()
        .item(&services_item)
        .separator()
        .item(&hide_item)
        .item(&hide_others_item)
        .separator()
        .item(&quit_item)
        .build()
        .map_err(|error| format!("Failed to build app submenu: {error}"))
}

fn build_app_menu(app: &AppHandle, state: &AppState) -> Result<tauri::menu::Menu<tauri::Wry>, String> {
    let recents = recent_paths_snapshot(state)?;
    let file_submenu = build_file_submenu(app, &recents)?;
    let edit_submenu = build_edit_submenu(app)?;

    let mut builder = MenuBuilder::new(app);

    #[cfg(target_os = "macos")]
    {
        let app_submenu = build_macos_app_submenu(app)?;
        let window_submenu = SubmenuBuilder::with_id(app, tauri::menu::WINDOW_SUBMENU_ID, "Window")
            .minimize()
            .maximize()
            .separator()
            .close_window()
            .build()
            .map_err(|error| format!("Failed to build Window submenu: {error}"))?;

        builder = builder
            .item(&app_submenu)
            .item(&file_submenu)
            .item(&edit_submenu)
            .item(&window_submenu);
    }

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.item(&file_submenu).item(&edit_submenu);
    }

    builder
        .build()
        .map_err(|error| format!("Failed to build application menu: {error}"))
}

fn refresh_app_menu(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let menu = build_app_menu(app, state)?;
    app.set_menu(menu)
        .map_err(|error| format!("Failed to apply menu: {error}"))?;
    Ok(())
}

fn window_for_menu_action(app: &AppHandle) -> Option<WebviewWindow> {
    let windows = app.webview_windows();

    if let Some(window) = windows
        .values()
        .find(|window| window.is_focused().ok().is_some_and(|focused| focused))
    {
        return Some(window.clone());
    }

    app.get_webview_window("main")
        .or_else(|| windows.into_values().next())
}

fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    let menu_id = event.id().as_ref();
    let Some(window) = window_for_menu_action(app) else {
        return;
    };

    let state = app.state::<AppState>();

    if menu_id == MENU_ID_FILE_OPEN {
        if let Err(error) = open_document_dialog_for_window(&window, app, state.inner()) {
            eprintln!("Failed to open document from menu: {error}");
        }
        return;
    }

    if menu_id == MENU_ID_EDIT_FIND {
        if let Err(error) = window.emit("basalt://focus-search", ()) {
            eprintln!("Failed to dispatch Find action to frontend: {error}");
        }
        return;
    }

    if let Some(raw_index) = menu_id.strip_prefix(MENU_ID_FILE_OPEN_RECENT_PREFIX) {
        if let Ok(index) = raw_index.parse::<usize>() {
            if let Err(error) = open_recent_file_for_window(app, state.inner(), &window, index) {
                eprintln!("Failed to open recent file: {error}");
            }
        }
    }
}

fn open_targets(
    app: &AppHandle,
    state: &AppState,
    targets: Vec<PathBuf>,
    use_main_window: bool,
) -> Result<(), String> {
    if targets.is_empty() {
        return Ok(());
    }

    let mut pending = targets;

    if use_main_window {
        if let Some(first) = pending.first().cloned() {
            ensure_main_window(app)?;
            assign_document_to_window(app, state, "main", first)?;
            pending.remove(0);
        }
    }

    for target in pending {
        spawn_document_window(app, state, target)?;
    }

    Ok(())
}

fn ensure_main_window(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("main").is_some() {
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Basalt")
        .inner_size(320.0, 720.0)
        .min_inner_size(320.0, 240.0)
        .build()
        .map_err(|error| format!("Failed to create main window: {error}"))?;

    register_window_cleanup(&window, app.clone());
    Ok(())
}

fn spawn_document_window(app: &AppHandle, state: &AppState, target: PathBuf) -> Result<(), String> {
    let normalized = fs::canonicalize(&target).unwrap_or(target.clone());

    // If this file is already open, focus that window instead of spawning a new one.
    {
        let documents = state
            .documents
            .lock()
            .map_err(|_| "App state is unavailable right now.".to_string())?;
        for (label, path) in documents.iter() {
            if *path == normalized {
                if let Some(window) = app.get_webview_window(label) {
                    let _ = window.show();
                    let _ = window.set_focus();
                    return Ok(());
                }
            }
        }
    }

    let label = format!("doc-{}", state.next_window.fetch_add(1, Ordering::Relaxed));
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(&title_for_document(&normalized))
        .inner_size(320.0, 720.0)
        .min_inner_size(320.0, 240.0)
        .build()
        .map_err(|error| format!("Failed to create document window: {error}"))?;

    register_window_cleanup(&window, app.clone());
    assign_document_to_window(app, state, &label, normalized)
}

fn assign_document_to_window(
    app: &AppHandle,
    state: &AppState,
    label: &str,
    target: PathBuf,
) -> Result<(), String> {
    let normalized = fs::canonicalize(&target).unwrap_or(target);

    {
        let mut documents = state
            .documents
            .lock()
            .map_err(|_| "App state is unavailable right now.".to_string())?;
        documents.insert(label.to_string(), normalized.clone());
    }

    remember_recent_file(state, &normalized)?;
    if let Err(error) = save_recent_files_to_disk(app, state) {
        eprintln!("Failed to persist recent files: {error}");
    }
    if let Err(error) = refresh_app_menu(app, state) {
        eprintln!("Failed to refresh app menu: {error}");
    }

    let Some(window) = app.get_webview_window(label) else {
        return Err(format!("Window '{label}' is not available."));
    };

    window
        .set_title(&title_for_document(&normalized))
        .map_err(|error| format!("Failed to set window title: {error}"))?;
    window
        .emit(
            "basalt://file-changed",
            FileChangedEvent {
                path: path_display(&normalized),
            },
        )
        .map_err(|error| format!("Failed to update window content: {error}"))?;

    let _ = window.show();
    let _ = window.set_focus();

    Ok(())
}

fn register_window_cleanup(window: &WebviewWindow, app: AppHandle) {
    let label = window.label().to_string();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            if let Ok(mut documents) = app.state::<AppState>().documents.lock() {
                documents.remove(&label);
            }
        }
    });
}

fn list_open_windows(state: &AppState) -> Result<Vec<WindowDescriptor>, String> {
    let documents = state
        .documents
        .lock()
        .map_err(|_| "App state is unavailable right now.".to_string())?;

    let mut windows: Vec<WindowDescriptor> = documents
        .iter()
        .map(|(label, path)| WindowDescriptor {
            label: label.to_string(),
            path: path_display(path),
            title: title_for_document(path),
        })
        .collect();

    windows.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.label.cmp(&right.label))
    });
    Ok(windows)
}

fn close_windows_by_path_key(
    app: &AppHandle,
    state: &AppState,
    target_key: &str,
) -> Result<CloseOutcome, String> {
    let matches = {
        let documents = state
            .documents
            .lock()
            .map_err(|_| "App state is unavailable right now.".to_string())?;
        documents
            .iter()
            .filter(|(_label, path)| path_key(path) == target_key)
            .map(|(label, path)| WindowDescriptor {
                label: label.to_string(),
                path: path_display(path),
                title: title_for_document(path),
            })
            .collect::<Vec<_>>()
    };

    close_windows_by_descriptors(app, state, matches)
}

fn close_window_by_label(
    app: &AppHandle,
    state: &AppState,
    label: &str,
) -> Result<CloseOutcome, String> {
    let matches = {
        let documents = state
            .documents
            .lock()
            .map_err(|_| "App state is unavailable right now.".to_string())?;
        documents
            .get(label)
            .map(|path| {
                vec![WindowDescriptor {
                    label: label.to_string(),
                    path: path_display(path),
                    title: title_for_document(path),
                }]
            })
            .unwrap_or_default()
    };

    close_windows_by_descriptors(app, state, matches)
}

fn close_windows_by_descriptors(
    app: &AppHandle,
    state: &AppState,
    windows: Vec<WindowDescriptor>,
) -> Result<CloseOutcome, String> {
    let mut closed = Vec::new();
    let mut failures = Vec::new();

    for window_descriptor in windows {
        if let Some(window) = app.get_webview_window(&window_descriptor.label) {
            if let Err(error) = window.close() {
                failures.push(format!("{}: {error}", window_descriptor.label));
                continue;
            }
        }
        closed.push(window_descriptor);
    }

    if !closed.is_empty() {
        let mut documents = state
            .documents
            .lock()
            .map_err(|_| "App state is unavailable right now.".to_string())?;
        for window in &closed {
            documents.remove(&window.label);
        }
    }

    Ok(CloseOutcome { closed, failures })
}

fn run_on_main_thread_sync<T, F>(app: &AppHandle, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let app_handle = app.clone();

    app.run_on_main_thread(move || {
        let result = task(&app_handle);
        let _ = sender.send(result);
    })
    .map_err(|error| format!("Unable to schedule window operation: {error}"))?;

    receiver
        .recv()
        .map_err(|_| "Window operation failed before returning a result.".to_string())?
}

fn control_endpoint_path() -> PathBuf {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("basalt-control.json");
    }

    if let Some(home_dir) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return PathBuf::from(home_dir).join(".basalt").join("control.json");
    }

    std::env::temp_dir().join("basalt-control.json")
}

fn start_control_server(app: AppHandle) -> Result<()> {
    let endpoint_path = control_endpoint_path();
    if let Some(parent) = endpoint_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Unable to create control directory {}",
                path_display(parent)
            )
        })?;
    }

    let listener = TcpListener::bind("127.0.0.1:0").context("Unable to bind control socket")?;
    let endpoint = ControlEndpoint {
        host: "127.0.0.1".to_string(),
        port: listener
            .local_addr()
            .context("Unable to determine control port")?
            .port(),
    };

    fs::write(
        &endpoint_path,
        serde_json::to_vec(&endpoint).context("Unable to encode control endpoint")?,
    )
    .with_context(|| {
        format!(
            "Unable to write control endpoint {}",
            path_display(&endpoint_path)
        )
    })?;

    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            match incoming {
                Ok(stream) => {
                    if let Err(error) = handle_control_connection(stream, &app) {
                        eprintln!("Control command failed: {error:#}");
                    }
                }
                Err(error) => {
                    eprintln!("Control socket error: {error}");
                }
            }
        }
    });

    Ok(())
}

fn handle_control_connection(mut stream: TcpStream, app: &AppHandle) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(2))).ok();

    let request: ControlRequest =
        serde_json::from_reader(&mut stream).context("Invalid control request payload")?;
    let response = execute_control_request(app, request);

    let mut writer = BufWriter::new(stream);
    serde_json::to_writer(&mut writer, &response).context("Unable to write control response")?;
    writer
        .flush()
        .context("Unable to flush control response to client")
}

fn execute_control_request(app: &AppHandle, request: ControlRequest) -> ControlResponse {
    match request {
        ControlRequest::ListWindows => {
            let state = app.state::<AppState>();
            match list_open_windows(state.inner()) {
                Ok(windows) => ControlResponse {
                    ok: true,
                    message: format!("{} window(s) open.", windows.len()),
                    windows,
                },
                Err(message) => ControlResponse {
                    ok: false,
                    message,
                    windows: Vec::new(),
                },
            }
        }
        ControlRequest::CloseByPath { path } => {
            let target_key = path_key(Path::new(&path));
            match run_on_main_thread_sync(app, move |handle| {
                let state = handle.state::<AppState>();
                close_windows_by_path_key(handle, state.inner(), &target_key)
            }) {
                Ok(outcome) if outcome.closed.is_empty() => ControlResponse {
                    ok: false,
                    message: format!("No open window is currently bound to '{}'.", path),
                    windows: Vec::new(),
                },
                Ok(outcome) if outcome.failures.is_empty() => {
                    let closed_count = outcome.closed.len();
                    ControlResponse {
                        ok: true,
                        message: format!("Closed {closed_count} window(s)."),
                        windows: outcome.closed,
                    }
                }
                Ok(outcome) => {
                    let closed_count = outcome.closed.len();
                    ControlResponse {
                        ok: false,
                        message: format!(
                            "Closed {closed_count} window(s), but some failed: {}",
                            outcome.failures.join("; ")
                        ),
                        windows: outcome.closed,
                    }
                }
                Err(message) => ControlResponse {
                    ok: false,
                    message,
                    windows: Vec::new(),
                },
            }
        }
        ControlRequest::CloseByLabel { label } => {
            let label_for_error = label.clone();
            match run_on_main_thread_sync(app, move |handle| {
                let state = handle.state::<AppState>();
                close_window_by_label(handle, state.inner(), &label)
            }) {
                Ok(outcome) if outcome.closed.is_empty() => ControlResponse {
                    ok: false,
                    message: format!("No open window found with label '{}'.", label_for_error),
                    windows: Vec::new(),
                },
                Ok(outcome) if outcome.failures.is_empty() => ControlResponse {
                    ok: true,
                    message: "Closed 1 window.".to_string(),
                    windows: outcome.closed,
                },
                Ok(outcome) => ControlResponse {
                    ok: false,
                    message: format!("Window close failed: {}", outcome.failures.join("; ")),
                    windows: outcome.closed,
                },
                Err(message) => ControlResponse {
                    ok: false,
                    message,
                    windows: Vec::new(),
                },
            }
        }
    }
}

fn parse_window_cli_command(args: &[String]) -> Result<Option<WindowCliCommand>> {
    let Some(scope) = args.first() else {
        return Ok(None);
    };

    if scope != "windows" && scope != "window" {
        return Ok(None);
    }

    let Some(subcommand) = args.get(1) else {
        return Err(anyhow!(WINDOW_USAGE));
    };

    match subcommand.as_str() {
        "list" => {
            let mut json = false;
            for arg in &args[2..] {
                if arg == "--json" {
                    json = true;
                } else {
                    return Err(anyhow!(WINDOW_USAGE));
                }
            }
            Ok(Some(WindowCliCommand::List { json }))
        }
        "close" => {
            let rest = &args[2..];
            if rest.is_empty() {
                return Err(anyhow!(WINDOW_USAGE));
            }

            if rest[0] == "--path" {
                if rest.len() != 2 {
                    return Err(anyhow!(WINDOW_USAGE));
                }
                let normalized_path = normalize_cli_path(&rest[1])?;
                return Ok(Some(WindowCliCommand::CloseByPath {
                    path: normalized_path,
                }));
            }

            if rest[0] == "--label" {
                if rest.len() != 2 || rest[1].trim().is_empty() {
                    return Err(anyhow!(WINDOW_USAGE));
                }
                return Ok(Some(WindowCliCommand::CloseByLabel {
                    label: rest[1].to_string(),
                }));
            }

            if rest.len() == 1 {
                let normalized_path = normalize_cli_path(&rest[0])?;
                return Ok(Some(WindowCliCommand::CloseByPath {
                    path: normalized_path,
                }));
            }

            Err(anyhow!(WINDOW_USAGE))
        }
        _ => Err(anyhow!(WINDOW_USAGE)),
    }
}

fn normalize_cli_path(path: &str) -> Result<String> {
    if path.trim().is_empty() {
        return Err(anyhow!("Path cannot be empty."));
    }

    let raw = PathBuf::from(path);
    let cwd = std::env::current_dir().context("Unable to determine current directory")?;
    let absolute = if raw.is_absolute() {
        raw
    } else {
        cwd.join(raw)
    };

    let normalized = if absolute.exists() {
        fs::canonicalize(&absolute).unwrap_or(absolute)
    } else {
        absolute
    };

    Ok(path_display(&normalized))
}

fn run_window_cli(args: &[String]) -> Result<bool> {
    let Some(command) = parse_window_cli_command(args)? else {
        return Ok(false);
    };

    match command {
        WindowCliCommand::List { json } => {
            let response = send_control_request(ControlRequest::ListWindows)?;
            if !response.ok {
                return Err(anyhow!(response.message));
            }

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&response.windows)
                        .context("Unable to encode windows as JSON")?
                );
            } else if response.windows.is_empty() {
                println!("No document windows are open.");
            } else {
                for window in response.windows {
                    println!("{}\t{}", window.label, window.path);
                }
            }
        }
        WindowCliCommand::CloseByPath { path } => {
            let response = send_control_request(ControlRequest::CloseByPath { path })?;
            if response.windows.is_empty() {
                return Err(anyhow!(response.message));
            }

            for window in &response.windows {
                println!("{}\t{}", window.label, window.path);
            }

            if !response.ok {
                return Err(anyhow!(response.message));
            }
        }
        WindowCliCommand::CloseByLabel { label } => {
            let response = send_control_request(ControlRequest::CloseByLabel { label })?;
            if response.windows.is_empty() {
                return Err(anyhow!(response.message));
            }

            for window in &response.windows {
                println!("{}\t{}", window.label, window.path);
            }

            if !response.ok {
                return Err(anyhow!(response.message));
            }
        }
    }

    Ok(true)
}

fn send_control_request(request: ControlRequest) -> Result<ControlResponse> {
    let endpoint_path = control_endpoint_path();
    let endpoint_data = fs::read(&endpoint_path).with_context(|| {
        format!(
            "Basalt is not running. Start it first, then retry. Missing {}",
            path_display(&endpoint_path)
        )
    })?;
    let endpoint: ControlEndpoint =
        serde_json::from_slice(&endpoint_data).context("Control endpoint file is invalid")?;

    let address: SocketAddr = format!("{}:{}", endpoint.host, endpoint.port)
        .parse()
        .context("Control endpoint address is invalid")?;

    let mut stream = None;
    let mut last_error = None;
    for _attempt in 0..5 {
        match TcpStream::connect_timeout(&address, Duration::from_millis(500)) {
            Ok(connected) => {
                stream = Some(connected);
                break;
            }
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(120));
            }
        }
    }

    let mut stream = match stream {
        Some(stream) => stream,
        None => {
            let _ = fs::remove_file(&endpoint_path);
            let error = last_error
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown connection failure".to_string());
            return Err(anyhow!(
                "Unable to reach the running Basalt instance ({error}). Start Basalt and retry."
            ));
        }
    };

    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(2))).ok();

    serde_json::to_writer(&mut stream, &request).context("Failed to send control request")?;
    stream.shutdown(Shutdown::Write).ok();

    serde_json::from_reader(&mut stream).context("Failed to decode control response")
}

fn collect_targets_from_args(args: &[String]) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    for arg in args {
        let input = PathBuf::from(arg);
        let absolute = if input.is_absolute() {
            input
        } else {
            cwd.join(input)
        };

        if !absolute.exists() {
            eprintln!("Ignoring missing path: {}", path_display(&absolute));
            continue;
        }

        if absolute.is_file() {
            let normalized = fs::canonicalize(&absolute).unwrap_or(absolute);
            let key = path_key(&normalized);
            if seen.insert(key) {
                targets.push(normalized);
            }
            continue;
        }

        if absolute.is_dir() {
            for file in collect_files_recursively(&absolute) {
                let key = path_key(&file);
                if seen.insert(key) {
                    targets.push(file);
                }
            }
        }
    }

    targets
}

fn collect_piped_input_target() -> Result<Option<PathBuf>> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }

    let mut input = Vec::new();
    stdin
        .lock()
        .read_to_end(&mut input)
        .context("Failed to read piped input from stdin")?;

    if input.is_empty() {
        return Ok(None);
    }

    let markdown = String::from_utf8_lossy(&input);
    let path = write_piped_markdown_file(&markdown)?;
    Ok(Some(path))
}

fn write_piped_markdown_file(markdown: &str) -> Result<PathBuf> {
    let directory = std::env::temp_dir().join("basalt-stdin");
    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "Unable to create temporary directory '{}'",
            path_display(&directory)
        )
    })?;

    let pid = std::process::id();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    for attempt in 0..128 {
        let file_name = if attempt == 0 {
            format!("stdin-{pid}-{timestamp}.md")
        } else {
            format!("stdin-{pid}-{timestamp}-{attempt}.md")
        };
        let path = directory.join(file_name);

        if path.exists() {
            continue;
        }

        fs::write(&path, markdown)
            .with_context(|| format!("Unable to write piped input to '{}'", path_display(&path)))?;
        return Ok(path);
    }

    Err(anyhow!(
        "Unable to allocate a temporary file for piped input in '{}'",
        path_display(&directory)
    ))
}

fn relay_target_to_instance(path: &Path) -> Result<()> {
    let executable = std::env::current_exe().context("Unable to locate current executable")?;

    Command::new(executable)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "Unable to launch Basalt for piped input '{}'",
                path_display(path)
            )
        })?;

    Ok(())
}

fn collect_files_recursively(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = WalkDir::new(directory)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| fs::canonicalize(entry.path()).unwrap_or_else(|_| entry.path().to_path_buf()))
        .collect();

    files.sort_by(|left, right| path_display(left).cmp(&path_display(right)));
    files
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lowered = ext.to_ascii_lowercase();
            MARKDOWN_EXTENSIONS.contains(&lowered.as_str())
        })
        .unwrap_or(false)
}

fn resolve_reference_from_document(current_document: &Path, reference: &str) -> Option<PathBuf> {
    let target = strip_reference(reference)?;
    if target.is_empty() || looks_external_reference(target) {
        return None;
    }

    let decoded = urlencoding::decode(target)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| target.to_string());
    if decoded.is_empty() {
        return None;
    }

    let resolved = if decoded.starts_with("file://") {
        PathBuf::from(decoded.trim_start_matches("file://"))
    } else {
        let raw = PathBuf::from(decoded);
        if raw.is_absolute() {
            raw
        } else {
            current_document.parent()?.join(raw)
        }
    };

    let normalized = if resolved.exists() {
        fs::canonicalize(&resolved).unwrap_or(resolved)
    } else {
        resolved
    };

    if normalized.exists() {
        Some(normalized)
    } else {
        None
    }
}

fn strip_reference(reference: &str) -> Option<&str> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let candidate = without_query.trim();

    if candidate.is_empty() {
        None
    } else {
        Some(candidate)
    }
}

fn looks_external_reference(reference: &str) -> bool {
    if reference.starts_with("//") {
        return true;
    }

    let Some((scheme, _rest)) = reference.split_once(':') else {
        return false;
    };

    if scheme.len() == 1 {
        return false;
    }

    scheme
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn title_for_document(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Untitled");
    format!("{name} - Basalt")
}

fn read_document_text(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn path_key(path: &Path) -> String {
    path_display(path).to_ascii_lowercase()
}

fn run_watch_mode(args: &[String]) -> Result<()> {
    if args.len() != 1 {
        return Err(anyhow!("Usage: basalt watch <directory>"));
    }

    let raw = PathBuf::from(&args[0]);
    let cwd = std::env::current_dir().context("Unable to determine current directory")?;
    let mut directory = if raw.is_absolute() {
        raw
    } else {
        cwd.join(raw)
    };

    if !directory.exists() {
        return Err(anyhow!(
            "Watch directory does not exist: {}",
            path_display(&directory)
        ));
    }

    directory = fs::canonicalize(&directory)
        .with_context(|| format!("Unable to canonicalize {}", path_display(&directory)))?;

    if !directory.is_dir() {
        return Err(anyhow!(
            "Watch target is not a directory: {}",
            path_display(&directory)
        ));
    }

    let mut known = HashSet::new();
    for existing in collect_files_recursively(&directory) {
        known.insert(path_key(&existing));
    }

    println!("Basalt watch active on {}", path_display(&directory));

    let (sender, receiver) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(sender, Config::default())
        .context("Unable to create file watcher")?;

    watcher
        .watch(&directory, RecursiveMode::Recursive)
        .with_context(|| format!("Unable to watch {}", path_display(&directory)))?;

    loop {
        match receiver.recv() {
            Ok(Ok(event)) => handle_watch_event(event.kind, event.paths, &mut known)?,
            Ok(Err(error)) => eprintln!("Watch event error: {error}"),
            Err(error) => return Err(anyhow!(error).context("Watch channel disconnected")),
        }
    }
}

fn handle_watch_event(
    kind: EventKind,
    paths: Vec<PathBuf>,
    known: &mut HashSet<String>,
) -> Result<()> {
    if !matches!(kind, EventKind::Create(_) | EventKind::Modify(_)) {
        return Ok(());
    }

    for candidate in paths {
        if !candidate.exists() || !candidate.is_file() {
            continue;
        }

        let normalized = fs::canonicalize(&candidate).unwrap_or(candidate.clone());
        let key = path_key(&normalized);
        if !known.insert(key) {
            continue;
        }

        launch_document_instance(&normalized)?;
    }

    Ok(())
}

fn launch_document_instance(path: &Path) -> Result<()> {
    let executable = std::env::current_exe().context("Unable to locate current executable")?;

    Command::new(executable)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("Unable to open '{}'.", path_display(path)))?;

    Ok(())
}

fn maybe_strip_executable(argv: Vec<String>) -> Vec<String> {
    if argv.is_empty() {
        return argv;
    }

    let first = argv[0].to_ascii_lowercase();
    if first.ends_with("basalt")
        || first.ends_with("basalt.exe")
        || first.contains("/basalt.app/")
        || first.contains("\\basalt.exe")
    {
        argv.into_iter().skip(1).collect()
    } else {
        argv
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cli_args: Vec<String> = std::env::args().skip(1).collect();

    match run_window_cli(&cli_args) {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("Basalt windows command failed: {error:#}");
            std::process::exit(1);
        }
    }

    if cli_args.first().is_some_and(|arg| arg == "watch") {
        if let Err(error) = run_watch_mode(&cli_args[1..]) {
            eprintln!("Basalt watch failed: {error:#}");
            std::process::exit(1);
        }
        return;
    }

    if cli_args.is_empty() {
        match collect_piped_input_target() {
            Ok(Some(path)) => {
                if let Err(error) = relay_target_to_instance(&path) {
                    eprintln!("Basalt stdin handoff failed: {error:#}");
                    std::process::exit(1);
                }
                return;
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("Basalt stdin processing failed: {error:#}");
                std::process::exit(1);
            }
        }
    }

    let startup_targets = collect_targets_from_args(&cli_args);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let args = maybe_strip_executable(argv);
            let targets = collect_targets_from_args(&args);
            if targets.is_empty() {
                return;
            }

            // Window creation must happen on the main thread.
            let app = app.clone();
            if let Err(error) = app.clone().run_on_main_thread(move || {
                let state = app.state::<AppState>();
                if let Err(error) = open_targets(&app, state.inner(), targets, false) {
                    eprintln!("Failed to open requested files: {error}");
                }
            }) {
                eprintln!("Failed to dispatch open to main thread: {error}");
            }
        }))
        .manage(AppState::default())
        .on_menu_event(handle_menu_event)
        .setup(move |app| {
            let state = app.state::<AppState>();

            if let Ok(mut recents) = state.recents.lock() {
                *recents = load_recent_files_from_disk(app.handle());
            }

            if let Err(error) = refresh_app_menu(app.handle(), state.inner()) {
                eprintln!("Failed to initialize app menu: {error}");
            }

            if let Err(error) = start_control_server(app.handle().clone()) {
                eprintln!("Basalt control server failed to start: {error:#}");
            }

            if let Err(error) = ensure_main_window(app.handle()) {
                eprintln!("Failed to create main window: {error}");
                return Ok(());
            }

            let main_window = app.get_webview_window("main").unwrap();
            register_window_cleanup(&main_window, app.handle().clone());

            // Merge CLI targets with any paths received via macOS Open With (RunEvent::Opened)
            let mut all_targets = startup_targets.clone();
            if let Ok(mut pending) = state.pending_open.lock() {
                all_targets.extend(pending.drain(..));
            }

            if !all_targets.is_empty() {
                if let Err(error) = open_targets(app.handle(), state.inner(), all_targets, true) {
                    eprintln!("Failed to open startup files: {error}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_document,
            list_recent_files,
            open_document_dialog,
            open_document_path,
            resolve_references,
            open_reference,
            open_in_vscode
        ])
        .build(tauri::generate_context!())
        .expect("error while building Basalt application")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = event {
                let paths: Vec<PathBuf> = urls
                    .iter()
                    .filter_map(|url| url.to_file_path().ok())
                    .collect();
                if paths.is_empty() {
                    return;
                }
                let state = app.state::<AppState>();
                // If setup has already run, open immediately; otherwise stash for setup to pick up
                let already_open = app.get_webview_window("main").is_some();
                if already_open {
                    if let Err(error) = open_targets(app, state.inner(), paths, false) {
                        eprintln!("Failed to open files from Open With: {error}");
                    }
                } else if let Ok(mut pending) = state.pending_open.lock() {
                    pending.extend(paths);
                }
            }
        });
}
