//! Smart Agent 桌面客户端（品牌锁定）。
//!
//! 铁律：站点地址是编译期常量（site.rs），前端一律通过 Tauri command 间接访问，
//! 任何 command 都不接收/传递 baseUrl；UI 与本地配置永不出现可改的站点地址。

mod api;
mod codex;
mod connectivity;
mod error;
mod site;
mod store;

use std::time::Duration;

use store::LocalStore;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

pub struct AppState {
    /// 共享 HTTP 客户端（rustls TLS）。
    pub http: reqwest::Client,
    /// 本地持久化状态（PAT / sk-key / 向导标记）。
    pub store: tokio::sync::Mutex<LocalStore>,
    /// 登录返回的短期 JWT（约 15 分钟），仅存内存，不落盘。
    pub session_jwt: tokio::sync::Mutex<Option<String>>,
}

pub fn apply_codex_config_headless() -> Result<(), String> {
    codex::apply_codex_config_from_local_store_blocking()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn minimize_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
fn toggle_maximize_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    if window.is_maximized().map_err(|error| error.to_string())? {
        window.unmaximize().map_err(|error| error.to_string())
    } else {
        window.maximize().map_err(|error| error.to_string())
    }
}

#[tauri::command]
fn hide_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window.hide().map_err(|error| error.to_string())
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("show", "显示主窗口")
        .separator()
        .text("quit", "退出 Smart Agent")
        .build()?;
    let icon = app.default_window_icon().cloned();
    let mut tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Smart Agent")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main_window(&tray.app_handle()),
            _ => {}
        });
    if let Some(icon) = icon {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
        .expect("failed to build http client");

    let state = AppState {
        http,
        store: tokio::sync::Mutex::new(LocalStore::load()),
        session_jwt: tokio::sync::Mutex::new(None),
    };

    tauri::Builder::default()
        .manage(state)
        .setup(|app| setup_tray(app).map_err(Into::into))
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // api.rs
            api::get_status,
            api::get_client_config,
            api::get_app_version,
            api::open_external_url,
            api::check_client_update,
            minimize_main_window,
            toggle_maximize_main_window,
            hide_main_window,
            api::get_model_settings,
            api::save_model_settings,
            api::register_account,
            api::login,
            api::login_2fa,
            api::ensure_pat,
            api::get_self,
            api::update_profile,
            api::change_password,
            api::get_usage,
            api::get_topup_info,
            api::get_topups,
            api::get_invite_info,
            api::claim_invite_reward,
            api::redeem_topup,
            api::start_topup_checkout,
            api::get_open_platform_overview,
            api::get_open_id_items,
            api::reveal_open_id_item,
            api::list_tokens,
            api::create_token,
            api::fetch_token_key,
            api::auto_provision,
            api::check_device_status,
            // codex.rs
            codex::detect_codex,
            codex::install_codex_desktop,
            codex::plan_codex_config,
            codex::apply_codex_config,
            codex::ensure_codex_config,
            codex::disconnect_codex,
            codex::restore_official_codex_login,
            codex::codex_desktop_running,
            // connectivity.rs
            connectivity::test_connection,
            // store.rs
            store::get_local_state,
            store::set_wizard_done,
            store::logout,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
