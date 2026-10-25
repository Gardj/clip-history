use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
};

use arboard::{Clipboard, ImageData};
use serde::Serialize;
use tauri::{Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow};
#[cfg(target_os = "macos")]
use tauri_nspanel::{
    CollectionBehavior, ManagerExt as PanelManagerExt, PanelLevel, StyleMask,
    WebviewWindowExt as PanelWindowExt,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const HOTKEY: &str = "CmdOrCtrl+Shift+V";
const DEFAULT_WINDOW_POSITION: &str = "center";
const WINDOW_POSITIONS: [&str; 9] = [
    "top-left",
    "top-center",
    "top-right",
    "center-left",
    "center",
    "center-right",
    "bottom-left",
    "bottom-center",
    "bottom-right",
];

#[cfg(target_os = "macos")]
mod clipboard_panel {
    tauri_nspanel::tauri_panel! {
        panel!(ClipboardPanel {
            config: {
                can_become_key_window: true,
                can_become_main_window: false,
                is_floating_panel: true
            }
        })
    }
}

#[tauri::command]
fn hide_panel(window: WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())
}

#[tauri::command]
fn set_monitor_paused(paused: bool, state: tauri::State<'_, Arc<AtomicBool>>) {
    state.store(paused, Ordering::Relaxed);
}

#[tauri::command]
fn set_window_size(window: WebviewWindow, width: f64, height: f64) -> Result<(), String> {
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_window_position(
    window: WebviewWindow,
    position: String,
    state: tauri::State<'_, Arc<Mutex<String>>>,
) -> Result<(), String> {
    if !WINDOW_POSITIONS.contains(&position.as_str()) {
        return Err("Unknown window position".into());
    }
    *state.lock().map_err(|error| error.to_string())? = position.clone();
    position_window(&window, &position).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_autostart(_app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::{fs, process::Command};
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        let agents = PathBuf::from(home).join("Library/LaunchAgents");
        fs::create_dir_all(&agents).map_err(|error| error.to_string())?;
        let label = "dev.codex.clip-history";
        let plist = agents.join(format!("{label}.plist"));
        let uid = Command::new("id")
            .arg("-u")
            .output()
            .map_err(|error| error.to_string())?;
        if !uid.status.success() {
            return Err("Could not determine macOS user id".into());
        }
        let domain = format!("gui/{}", String::from_utf8_lossy(&uid.stdout).trim());

        if enabled {
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            let executable = executable
                .to_string_lossy()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let contents = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>{executable}</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><false/></dict></plist>"
            );
            fs::write(&plist, contents).map_err(|error| error.to_string())?;
            let output = Command::new("launchctl")
                .args(["bootstrap", &domain])
                .arg(&plist)
                .output()
                .map_err(|error| error.to_string())?;
            if !output.status.success() {
                // The item can already be loaded after its plist was updated.
                let message = String::from_utf8_lossy(&output.stderr);
                if !message.contains("already bootstrapped") {
                    return Err(message.trim().to_owned());
                }
            }
        } else {
            let _ = Command::new("launchctl")
                .args(["bootout", &domain])
                .arg(&plist)
                .output();
            if plist.exists() {
                fs::remove_file(plist).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (_app, enabled);
        Err("Autostart is currently supported on macOS only".into())
    }
}

#[tauri::command]
fn get_autostart() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        Ok(PathBuf::from(home)
            .join("Library/LaunchAgents/dev.codex.clip-history.plist")
            .exists())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(false)
    }
}

#[tauri::command]
fn write_clipboard_files(paths: Vec<String>) -> Result<(), String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    Clipboard::new()
        .map_err(|error| error.to_string())?
        .set()
        .file_list(&paths)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_history_assets(paths: Vec<String>, app: tauri::AppHandle) -> Result<(), String> {
    let archive_root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("clips");
    let mut folders = std::collections::HashSet::new();
    for path in paths {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            if parent.parent() == Some(archive_root.as_path()) && parent.starts_with(&archive_root)
            {
                folders.insert(parent.to_path_buf());
            }
        }
    }
    for folder in folders {
        if folder.exists() {
            fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn copy_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_path(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
    }
    Ok(())
}

fn path_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if metadata.is_dir() {
        return fs::read_dir(path)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| path_size(&entry.path()))
                    .sum()
            })
            .unwrap_or(0);
    }
    0
}

fn archive_clipboard_files(
    archive_root: &Path,
    id: &str,
    sources: &[PathBuf],
) -> (Vec<String>, u64) {
    let folder = archive_root.join(id);
    if fs::create_dir_all(&folder).is_err() {
        let paths: Vec<String> = sources
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let size = sources.iter().map(|path| path_size(path)).sum();
        return (paths, size);
    }
    let paths: Vec<String> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let name = source.file_name().unwrap_or_default().to_string_lossy();
            let destination = folder.join(format!("{index}-{name}"));
            if copy_path(source, &destination).is_ok() {
                destination.to_string_lossy().into_owned()
            } else {
                source.to_string_lossy().into_owned()
            }
        })
        .collect();
    let size = paths.iter().map(|path| path_size(Path::new(path))).sum();
    (paths, size)
}

#[tauri::command]
fn write_clipboard_image(width: u32, height: u32, bytes: Vec<u8>) -> Result<(), String> {
    Clipboard::new()
        .map_err(|error| error.to_string())?
        .set()
        .image(ImageData {
            width: width as usize,
            height: height as usize,
            bytes: Cow::Owned(bytes),
        })
        .map_err(|error| error.to_string())
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum ClipboardItem {
    Text {
        id: String,
        text: String,
    },
    Image {
        id: String,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    },
    Files {
        id: String,
        paths: Vec<String>,
        #[serde(rename = "storageBytes")]
        storage_bytes: u64,
    },
}

fn fingerprint<T: std::hash::Hash>(value: &T) -> String {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn current_clipboard_fingerprint(clipboard: &mut Clipboard) -> String {
    if let Ok(paths) = clipboard.get().file_list() {
        if !paths.is_empty() {
            return fingerprint(
                &paths
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>(),
            );
        }
    }
    if let Ok(image) = clipboard.get().image() {
        return fingerprint(&(image.width, image.height, image.bytes.as_ref()));
    }
    clipboard
        .get()
        .text()
        .map(|text| fingerprint(&text))
        .unwrap_or_default()
}

fn position_window(window: &WebviewWindow, preset: &str) -> tauri::Result<()> {
    let monitor = window.current_monitor()?.or(window.primary_monitor()?);

    if let Some(monitor) = monitor {
        let work_area = monitor.work_area();
        let window_size = window.outer_size()?;
        let max_x =
            work_area.position.x + work_area.size.width.saturating_sub(window_size.width) as i32;
        let max_y =
            work_area.position.y + work_area.size.height.saturating_sub(window_size.height) as i32;
        let margin = (24.0 * monitor.scale_factor()) as i32;
        let center_x =
            work_area.position.x + (work_area.size.width as i32 - window_size.width as i32) / 2;
        let center_y =
            work_area.position.y + (work_area.size.height as i32 - window_size.height as i32) / 2;
        let x = match preset {
            "top-left" | "center-left" | "bottom-left" => work_area.position.x + margin,
            "top-right" | "center-right" | "bottom-right" => max_x - margin,
            _ => center_x,
        }
        .clamp(work_area.position.x, max_x.max(work_area.position.x));
        let y = match preset {
            "top-left" | "top-center" | "top-right" => work_area.position.y + margin,
            "bottom-left" | "bottom-center" | "bottom-right" => max_y - margin,
            _ => center_y,
        }
        .clamp(work_area.position.y, max_y.max(work_area.position.y));

        window.set_position(PhysicalPosition::new(x, y))?;
    }

    Ok(())
}

fn main() {
    let monitor_paused = Arc::new(AtomicBool::new(false));
    let window_position = Arc::new(Mutex::new(DEFAULT_WINDOW_POSITION.to_string()));

    let builder = tauri::Builder::default()
        .manage(monitor_paused.clone())
        .manage(window_position)
        .plugin(tauri_plugin_clipboard_manager::init());

    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }

                    if let Some(window) = app.get_webview_window("main") {
                        if window.is_visible().unwrap_or(false) {
                            let _ = window.hide();
                        } else {
                            let preset = app
                                .state::<Arc<Mutex<String>>>()
                                .lock()
                                .map(|position| position.clone())
                                .unwrap_or_else(|_| DEFAULT_WINDOW_POSITION.to_string());
                            if let Err(error) = position_window(&window, &preset) {
                                eprintln!("Could not position clipboard window: {error}");
                            }
                            #[cfg(target_os = "macos")]
                            {
                                if let Ok(panel) = app.get_webview_panel("main") {
                                    panel.show_and_make_key();
                                } else {
                                    eprintln!("Clipboard panel is not initialized");
                                    return;
                                }
                            }
                            #[cfg(not(target_os = "macos"))]
                            {
                                if let Err(error) = window.show() {
                                    eprintln!("Could not show clipboard window: {error}");
                                    return;
                                }
                                if let Err(error) = window.set_focus() {
                                    eprintln!("Could not focus clipboard window: {error}");
                                }
                            }
                            let _ = window.emit("panel-opened", ());
                        }
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            hide_panel,
            set_monitor_paused,
            set_window_size,
            set_window_position,
            set_autostart,
            get_autostart,
            delete_history_assets,
            write_clipboard_files,
            write_clipboard_image
        ])
        .setup(move |app| {
            app.global_shortcut().register(HOTKEY)?;

            if let Some(window) = app.get_webview_window("main") {
                if let Err(error) = position_window(&window, DEFAULT_WINDOW_POSITION) {
                    eprintln!("Could not position clipboard window: {error}");
                }

                #[cfg(target_os = "macos")]
                {
                    use clipboard_panel::ClipboardPanel;

                    let panel = window.to_panel::<ClipboardPanel>()?;
                    panel.set_level(PanelLevel::Dock.value());
                    panel.set_floating_panel(true);
                    panel.set_has_shadow(false);
                    panel.set_hides_on_deactivate(false);
                    panel.set_style_mask(
                        StyleMask::empty()
                            .borderless()
                            .nonactivating_panel()
                            .value(),
                    )?;
                    panel.set_corner_radius(22.0);
                    panel.set_opaque(false);
                    panel.set_transparent(true);
                    panel.set_collection_behavior(
                        CollectionBehavior::new()
                            .stationary()
                            .can_join_all_spaces()
                            .full_screen_auxiliary()
                            .ignores_cycle()
                            .value(),
                    );
                }
            }

            // Seed the value without adding the clipboard contents from before
            // launch to the newly created history.
            let handle = app.handle().clone();
            let monitor_paused = monitor_paused.clone();
            let archive_root = app.path().app_data_dir()?.join("clips");
            fs::create_dir_all(&archive_root)?;
            let mut clipboard = Clipboard::new().ok();
            let mut last_item_id = clipboard
                .as_mut()
                .map(current_clipboard_fingerprint)
                .unwrap_or_default();

            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(800));

                    if monitor_paused.load(Ordering::Relaxed) {
                        continue;
                    }

                    let Some(clipboard) = clipboard.as_mut() else {
                        continue;
                    };
                    let item = match clipboard.get().file_list() {
                        Ok(paths) if !paths.is_empty() => {
                            let id = fingerprint(
                                &paths
                                    .iter()
                                    .map(|path| path.to_string_lossy())
                                    .collect::<Vec<_>>(),
                            );
                            let (paths, storage_bytes) =
                                archive_clipboard_files(&archive_root, &id, &paths);
                            Some((
                                id.clone(),
                                ClipboardItem::Files {
                                    id,
                                    paths,
                                    storage_bytes,
                                },
                            ))
                        }
                        _ => match clipboard.get().image() {
                            Ok(image) => {
                                let id =
                                    fingerprint(&(image.width, image.height, image.bytes.as_ref()));
                                Some((
                                    id.clone(),
                                    ClipboardItem::Image {
                                        id,
                                        width: image.width as u32,
                                        height: image.height as u32,
                                        bytes: image.bytes.to_vec(),
                                    },
                                ))
                            }
                            Err(_) => clipboard
                                .get()
                                .text()
                                .ok()
                                .filter(|text| !text.is_empty())
                                .map(|text| {
                                    let id = fingerprint(&text);
                                    (id.clone(), ClipboardItem::Text { id, text })
                                }),
                        },
                    };

                    if let Some((id, item)) = item {
                        if id != last_item_id {
                            last_item_id = id;
                            let _ = handle.emit("clipboard-item", item);
                        }
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Clip History");
}
