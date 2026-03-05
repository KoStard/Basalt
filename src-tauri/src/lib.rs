use anyhow::{anyhow, Context, Result};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use walkdir::WalkDir;

const MARKDOWN_EXTENSIONS: [&str; 5] = ["md", "markdown", "mdown", "mkd", "mdx"];

#[derive(Default)]
struct AppState {
    documents: Mutex<HashMap<String, PathBuf>>,
    next_window: AtomicU64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadedDocument {
    path: String,
    file_name: String,
    markdown: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileChangedEvent {
    path: String,
}

#[tauri::command]
fn load_document(
    window: WebviewWindow,
    state: tauri::State<AppState>,
) -> Result<LoadedDocument, String> {
    let path = active_document_for_window(&window, &state)?;
    let markdown = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read '{}': {error}", path_display(&path)))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "Untitled.md".to_string());

    Ok(LoadedDocument {
        path: path_display(&path),
        file_name,
        markdown,
    })
}

#[tauri::command]
fn resolve_references(
    window: WebviewWindow,
    references: Vec<String>,
    state: tauri::State<AppState>,
) -> Result<HashMap<String, Option<String>>, String> {
    let document = active_document_for_window(&window, &state)?;
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
    let document = active_document_for_window(&window, &state)?;
    let Some(target) = resolve_reference_from_document(&document, &reference) else {
        return Ok(());
    };

    if target.is_dir() {
        let targets = collect_markdown_files(&target);
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
    let path = active_document_for_window(&window, &state)?;
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

fn active_document_for_window(
    window: &WebviewWindow,
    state: &tauri::State<AppState>,
) -> Result<PathBuf, String> {
    let guard = state
        .documents
        .lock()
        .map_err(|_| "App state is unavailable right now.".to_string())?;

    guard
        .get(window.label())
        .cloned()
        .ok_or_else(|| "This window is not bound to a document yet.".to_string())
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
        .inner_size(1200.0, 820.0)
        .min_inner_size(520.0, 420.0)
        .build()
        .map_err(|error| format!("Failed to create main window: {error}"))?;

    register_window_cleanup(&window, app.clone());
    Ok(())
}

fn spawn_document_window(app: &AppHandle, state: &AppState, target: PathBuf) -> Result<(), String> {
    let label = format!("doc-{}", state.next_window.fetch_add(1, Ordering::Relaxed));
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(&title_for_document(&target))
        .inner_size(1200.0, 820.0)
        .min_inner_size(520.0, 420.0)
        .build()
        .map_err(|error| format!("Failed to create document window: {error}"))?;

    register_window_cleanup(&window, app.clone());
    assign_document_to_window(app, state, &label, target)
}

fn assign_document_to_window(
    app: &AppHandle,
    state: &AppState,
    label: &str,
    target: PathBuf,
) -> Result<(), String> {
    {
        let mut documents = state
            .documents
            .lock()
            .map_err(|_| "App state is unavailable right now.".to_string())?;
        documents.insert(label.to_string(), target.clone());
    }

    let Some(window) = app.get_webview_window(label) else {
        return Err(format!("Window '{label}' is not available."));
    };

    window
        .set_title(&title_for_document(&target))
        .map_err(|error| format!("Failed to set window title: {error}"))?;
    window
        .emit(
            "basalt://file-changed",
            FileChangedEvent {
                path: path_display(&target),
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
            for markdown in collect_markdown_files(&absolute) {
                let key = path_key(&markdown);
                if seen.insert(key) {
                    targets.push(markdown);
                }
            }
        }
    }

    targets
}

fn collect_markdown_files(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = WalkDir::new(directory)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file() && is_markdown_file(entry.path()))
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
        .unwrap_or("Untitled.md");
    format!("{name} - Basalt")
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
    for existing in collect_markdown_files(&directory) {
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
        if !candidate.exists() || !candidate.is_file() || !is_markdown_file(&candidate) {
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

    if cli_args.first().is_some_and(|arg| arg == "watch") {
        if let Err(error) = run_watch_mode(&cli_args[1..]) {
            eprintln!("Basalt watch failed: {error:#}");
            std::process::exit(1);
        }
        return;
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

            let state = app.state::<AppState>();
            if let Err(error) = open_targets(app, state.inner(), targets, false) {
                eprintln!("Failed to open requested files: {error}");
            }
        }))
        .manage(AppState::default())
        .setup(move |app| {
            let Some(main_window) = app.get_webview_window("main") else {
                return Ok(());
            };

            register_window_cleanup(&main_window, app.handle().clone());

            if startup_targets.is_empty() {
                return Ok(());
            }

            let state = app.state::<AppState>();
            if let Err(error) =
                open_targets(app.handle(), state.inner(), startup_targets.clone(), true)
            {
                eprintln!("Failed to open startup files: {error}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_document,
            resolve_references,
            open_reference,
            open_in_vscode
        ])
        .run(tauri::generate_context!())
        .expect("error while running Basalt application");
}
