use std::env;

/// Get the renderer type from environment variable at runtime.
///
/// This allows the frontend to check `EMTERM_RENDERER` environment variable
/// at runtime, enabling E2E tests to verify renderer switching.
#[tauri::command]
pub fn get_renderer_type() -> String {
    env::var("EMTERM_RENDERER").unwrap_or_else(|_| "dom".to_string())
}
