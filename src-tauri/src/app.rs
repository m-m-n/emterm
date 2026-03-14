/// Set the taskbar icon from the embedded ICO resource on Windows.
///
/// Works around a bug in tao's `CreateIcon()` where the AND mask is created with
/// 1 byte per pixel instead of 1 bit per pixel, causing alpha transparency to be
/// lost. By loading the icon directly from the embedded resource via `LoadImageW`,
/// Windows handles the ICO's alpha channel correctly.
#[cfg(windows)]
fn set_taskbar_icon(window: &tauri::WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        IMAGE_ICON, LR_DEFAULTSIZE, LoadImageW, SendMessageW, WM_SETICON,
    };
    use windows::core::PCWSTR;

    const ICON_BIG: usize = 1;
    const MAINICON_ID: u16 = 32512;

    let window_handle = window.window_handle()?;
    let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
        return Err("not a Win32 window".into());
    };

    unsafe {
        let hmodule = GetModuleHandleW(PCWSTR::null())?;
        let hicon = LoadImageW(
            Some(hmodule.into()),
            PCWSTR::from_raw(MAINICON_ID as *const u16),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE,
        )?;
        let hwnd = windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _);
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(windows::Win32::Foundation::WPARAM(ICON_BIG)),
            Some(windows::Win32::Foundation::LPARAM(hicon.0 as isize)),
        );
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg(not(test))]
pub fn run() {
    use crate::download_registry::DownloadRegistry;
    use crate::logging;
    use crate::pty::PtyManager;
    use std::sync::Arc;
    use crate::sftp::pool::ConcurrentUploadPool;
    use crate::sftp::upload::SftpProcessManager;
    use crate::state::{ImageProcessorState, LargeImageDataStore};
    use crate::{commands, tauri_commands};

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .manage(PtyManager::new())
        .manage(ImageProcessorState::new())
        .manage(LargeImageDataStore::new())
        .manage(SftpProcessManager::new())
        .manage(ConcurrentUploadPool::new(4))
        .manage(Arc::new(DownloadRegistry::new()))
        .invoke_handler(tauri::generate_handler![
            tauri_commands::pty_spawn,
            tauri_commands::pty_write,
            tauri_commands::pty_resize,
            tauri_commands::pty_kill,
            tauri_commands::process_image_data,
            tauri_commands::process_kitty_batch,
            tauri_commands::fetch_image_data,
            tauri_commands::console_log,
            tauri_commands::console_warn,
            tauri_commands::console_error,
            tauri_commands::console_info,
            tauri_commands::console_debug,
            tauri_commands::session_count,
            tauri_commands::tab_close_graceful,
            commands::config::io::load_settings,
            commands::config::io::save_settings,
            commands::editor::check_file_exists,
            commands::editor::open_file_in_editor,
            commands::font::list_fonts,
            commands::ssh::detect_ssh_command,
            commands::ssh::load_ssh_config_hosts,
            commands::ssh::build_ssh_args,
            commands::ssh::validate_identity_file,
            commands::sftp::sftp_check_duplicates,
            commands::sftp::sftp_upload,
            commands::sftp::sftp_cancel_upload,
            tauri_commands::set_language,
            tauri_commands::get_log_contents,
            tauri_commands::get_log_tail,
            tauri_commands::clear_log,
            tauri_commands::get_log_path,
            tauri_commands::set_log_recording,
            tauri_commands::decode_iterm2_image,
            tauri_commands::start_download_file,
            tauri_commands::append_download_chunk,
            tauri_commands::finish_download_file,
            tauri_commands::cancel_download_file,
        ])
        .setup(|app| {
            // Initialize custom logger for backend
            // Use Debug level in debug builds, Info level in release builds
            let level = if cfg!(debug_assertions) {
                log::Level::Debug
            } else {
                log::Level::Info
            };
            logging::BackendLogger::init(level);

            // Initialize log file for release builds and sync log recording flag
            {
                use tauri::Manager;

                if !cfg!(debug_assertions) {
                    if let Ok(log_dir) = app.path().app_log_dir() {
                        logging::init_log_file(&log_dir);
                    }
                }

                // Sync log recording flag from settings
                if let Ok(settings) = commands::config::io::load_settings(app.handle().clone()) {
                    logging::set_log_recording_enabled(settings.log_recording_enabled);
                }

                // On Windows, set ICON_BIG from the embedded ICO resource to fix
                // taskbar icon transparency. tao's CreateIcon() has a bug where the
                // AND mask format is incorrect, causing alpha transparency to be lost.
                // Loading directly from the resource via LoadImageW bypasses this.
                #[cfg(windows)]
                if let Some(window) = app.get_webview_window("main") {
                    if let Err(e) = set_taskbar_icon(&window) {
                        log::warn!("Failed to set taskbar icon: {e}");
                    }
                }

                #[cfg(not(windows))]
                let _ = app;
            }

            // Spawn background thread for download registry cleanup (120s idle timeout)
            {
                use tauri::Manager;
                let registry = Arc::clone(&app.state::<Arc<DownloadRegistry>>().inner());
                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(30));
                        registry.cleanup_expired();
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
