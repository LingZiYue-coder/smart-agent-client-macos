//! 本地状态存储：PAT、用户信息、当前 sk-key、向导完成标记。
//! 存放于 app data 目录（Windows: %APPDATA%\com.smartai.agent\store.json）。
//!
//! TODO(安全): 本期 PAT 与 sk-key 以明文 JSON 落盘，后续必须换成
//! Windows DPAPI / macOS Keychain 加密存储（keyring 或 stronghold 方案）。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const APP_DIR_NAME: &str = "com.smartai.agent";

/// app data 根目录：%APPDATA%\com.smartai.agent
pub fn app_data_dir() -> AppResult<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| AppError::msg("无法定位系统应用数据目录"))?;
    Ok(base.join(APP_DIR_NAME))
}

fn store_path() -> AppResult<PathBuf> {
    Ok(app_data_dir()?.join("store.json"))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalStore {
    /// 系统访问令牌 PAT（每用户仅一个，重新生成会作废旧的）。
    #[serde(default)]
    pub pat: Option<String>,
    #[serde(default)]
    pub user_id: Option<i64>,
    #[serde(default)]
    pub username: Option<String>,
    /// 当前使用的 API 令牌（sk- 开头完整 key）。
    #[serde(default)]
    pub sk_key: Option<String>,
    #[serde(default)]
    pub token_id: Option<i64>,
    #[serde(default)]
    pub token_name: Option<String>,
    /// 本设备稳定 UUID；同时作为服务端设备令牌名称。
    #[serde(default)]
    pub device_id: Option<String>,
    /// 首次向导是否已完成。
    #[serde(default)]
    pub wizard_done: bool,
    /// 用户是否希望保持 Codex 接入。
    /// - true：一键接入成功后；磁盘配置被 Desktop 洗掉时允许静默回写
    /// - false：用户点了断开，或从未接入；**禁止**自动回写（否则会「断开又连上」）
    #[serde(default)]
    pub codex_desired: bool,
    /// 用户在 Smart Agent 中允许 Codex 使用的模型。
    #[serde(default)]
    pub selected_models: Vec<String>,
    /// Codex 启动时默认使用的模型。
    #[serde(default)]
    pub default_model: Option<String>,
}

impl LocalStore {
    pub fn load() -> Self {
        let Ok(path) = store_path() else {
            return LocalStore::default();
        };
        let Ok(text) = fs::read_to_string(&path) else {
            return LocalStore::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) -> AppResult<()> {
        let path = store_path()?;
        let json = serde_json::to_string_pretty(self)?;
        crate::codex::atomic_write(&path, json.as_bytes())
    }

    pub fn ensure_device_id(&mut self) -> AppResult<String> {
        if let Some(device_id) = &self.device_id {
            if uuid::Uuid::parse_str(device_id).is_ok() {
                return Ok(device_id.clone());
            }
        }
        let device_id = uuid::Uuid::new_v4().to_string();
        self.device_id = Some(device_id.clone());
        self.save()?;
        Ok(device_id)
    }
}

/// 给前端看的本地状态摘要（不含任何完整凭据）。
#[derive(Debug, Clone, Serialize)]
pub struct FrontState {
    pub wizard_done: bool,
    pub has_pat: bool,
    pub has_key: bool,
    /// 用户意图：是否保持 Codex 接入（与磁盘是否已配置可能不一致）。
    pub codex_desired: bool,
    pub username: Option<String>,
    pub user_id: Option<i64>,
    pub token_name: Option<String>,
}

impl From<&LocalStore> for FrontState {
    fn from(s: &LocalStore) -> Self {
        FrontState {
            wizard_done: s.wizard_done,
            has_pat: s.pat.is_some(),
            has_key: s.sk_key.is_some(),
            codex_desired: s.codex_desired,
            username: s.username.clone(),
            user_id: s.user_id,
            token_name: s.token_name.clone(),
        }
    }
}

// ---------------- Tauri commands ----------------

#[tauri::command]
pub async fn get_local_state(
    state: tauri::State<'_, crate::AppState>,
) -> Result<FrontState, AppError> {
    let store = state.store.lock().await;
    Ok(FrontState::from(&*store))
}

#[tauri::command]
pub async fn set_wizard_done(
    state: tauri::State<'_, crate::AppState>,
    done: bool,
) -> Result<(), AppError> {
    let mut store = state.store.lock().await;
    store.wizard_done = done;
    store.save()
}

/// 退出登录：清空本地凭据（不触碰服务端 PAT，避免影响用户其他设备）。
#[tauri::command]
pub async fn logout(state: tauri::State<'_, crate::AppState>) -> Result<(), AppError> {
    {
        let mut jwt = state.session_jwt.lock().await;
        *jwt = None;
    }
    let mut store = state.store.lock().await;
    store.pat = None;
    store.sk_key = None;
    store.token_id = None;
    store.token_name = None;
    store.selected_models.clear();
    store.default_model = None;
    store.codex_desired = false;
    // device_id 代表物理设备，退出账号后仍保留。
    store.user_id = None;
    store.username = None;
    store.save()
}
