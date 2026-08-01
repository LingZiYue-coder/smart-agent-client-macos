//! New API（v1.0.0-rc.22）管理面 HTTP 客户端。
//!
//! 鉴权策略（两代通吃）：
//! - 长期凭据用 PAT（系统访问令牌），Authorization: Bearer <PAT>；
//! - 登录后短期使用 JWT（约 15 分钟）完成 PAT 生成等一次性操作；
//! - 无条件附带 New-Api-User: <userId> 请求头（旧版 v0.x 必需，rc 新版忽略，
//!   已核实 rc.22 middleware/auth.go 不读取该头）。
//!
//! 站点地址是编译期常量（见 site.rs），本模块所有函数都不接收 baseUrl 参数。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::site::api_url;
use crate::AppState;

const UA: &str = concat!("SmartAgentClient/", env!("CARGO_PKG_VERSION"));

/// 解析 New API 统一响应包 {success, message, data}，success=false 时报中文错误。
fn parse_envelope(body: Value) -> AppResult<Value> {
    let success = body
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if success {
        Ok(body.get("data").cloned().unwrap_or(Value::Null))
    } else {
        let msg = body
            .get("message")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .unwrap_or("服务器返回未知错误");
        Err(AppError::msg(msg.to_string()))
    }
}

async fn read_json(resp: reqwest::Response) -> AppResult<Value> {
    let status = resp.status();
    let text = resp.text().await?;
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => Ok(v),
        Err(_) => Err(AppError::Http(format!(
            "服务器响应异常（HTTP {}）",
            status.as_u16()
        ))),
    }
}

/// 组装一个带鉴权头的请求。credential 为 PAT 或 JWT；user_id 用于 New-Api-User 头。
fn with_auth(
    req: reqwest::RequestBuilder,
    credential: &str,
    user_id: Option<i64>,
) -> reqwest::RequestBuilder {
    let mut req = req
        .header("Authorization", format!("Bearer {credential}"))
        .header("User-Agent", UA);
    if let Some(id) = user_id {
        // 旧版 v0.x 强制要求；rc 新版忽略该头，无条件带上以两代通吃。
        req = req.header("New-Api-User", id.to_string());
    }
    req
}

/// 取当前可用凭据：优先 PAT，其次登录后的短期 JWT。
async fn current_credential(state: &AppState) -> AppResult<(String, Option<i64>)> {
    let store = state.store.lock().await;
    if let Some(pat) = &store.pat {
        return Ok((pat.clone(), store.user_id));
    }
    let user_id = store.user_id;
    drop(store);
    let jwt = state.session_jwt.lock().await;
    if let Some(jwt) = &*jwt {
        return Ok((jwt.clone(), user_id));
    }
    Err(AppError::msg("尚未登录，请先完成登录"))
}

/// GET 一个需要鉴权的管理面接口并解包 data。
async fn authed_get(state: &AppState, path: &str) -> AppResult<Value> {
    let (cred, uid) = current_credential(state).await?;
    let resp = with_auth(state.http.get(api_url(path)), &cred, uid)
        .send()
        .await?;
    parse_envelope(read_json(resp).await?)
}

async fn authed_post(state: &AppState, path: &str, body: &Value) -> AppResult<Value> {
    let (cred, uid) = current_credential(state).await?;
    let resp = with_auth(state.http.post(api_url(path)), &cred, uid)
        .json(body)
        .send()
        .await?;
    parse_envelope(read_json(resp).await?)
}

// ---------------- 数据结构 ----------------

#[derive(Debug, Clone, Serialize)]
pub struct LoginOutcome {
    pub require_2fa: bool,
    pub flow_token: Option<String>,
    pub user_id: Option<i64>,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatResult {
    /// 本地已有可用 PAT（未重新生成）。
    pub reused: bool,
    /// 本次新生成了 PAT（旧 PAT 已被服务端作废）。
    pub generated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelfInfo {
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub email: String,
    /// 剩余额度（quota，500000 = 1 美元，以 /api/status 的 quota_per_unit 为准）。
    pub quota: i64,
    pub used_quota: i64,
    pub request_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    /// 今日消耗 quota 合计（来自 GET /api/data/self 日聚合）。
    pub today_quota: i64,
    pub today_requests: i64,
    pub today_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopUpMethod {
    pub id: String,
    pub label: String,
    pub min_amount: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopUpInfo {
    pub online_enabled: bool,
    pub redemption_enabled: bool,
    pub min_amount: i64,
    pub amount_options: Vec<i64>,
    pub methods: Vec<TopUpMethod>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InviteInfo {
    pub aff_code: String,
    pub inviter_id: i64,
    pub aff_count: i64,
    pub aff_quota: i64,
    pub aff_history_quota: i64,
    pub inviter_reward: i64,
    pub invitee_reward: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InviteClaimResult {
    pub rewarded: bool,
    pub already_done: bool,
    pub inviter_quota: i64,
    pub invitee_quota: i64,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPlatformOverview {
    pub enabled: bool,
    pub id_enabled: bool,
    pub id_unlocked: bool,
    pub unlock_label: String,
    pub unlock_hint: String,
    pub key_enabled: bool,
    pub docs_url: String,
    pub one_models: Vec<String>,
    pub last_sync_time: String,
    pub id_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenIdItem {
    pub id: String,
    pub label: String,
    pub masked_username: String,
    pub region: String,
    pub status: String,
    pub last_check: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenIdReveal {
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProvisionResult {
    pub token_id: i64,
    pub token_name: String,
    /// 仅用于展示的脱敏 key（完整 key 只存本地 store 并写入 auth.json）。
    pub masked_key: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub default_model: String,
    pub models: Vec<String>,
    pub min_client_version: String,
    /// 最新版本（展示/可选更新）；缺省与 min 相同语义由前端处理。
    #[serde(default)]
    pub latest_version: String,
    /// 更新弹窗「前往官网下载」目标 URL。
    #[serde(default)]
    pub download_url: String,
    /// 官网首页（可选）。
    #[serde(default)]
    pub website_url: String,
    pub announcement: String,
    pub user_agreement: String,
    pub privacy_policy: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSettings {
    pub available_models: Vec<String>,
    /// 当前写入 Codex 的唯一模型（Desktop 列表不可靠，只靠 config.model）
    pub active_model: String,
    pub synced_to_codex: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            default_model: crate::site::DEFAULT_MODEL.to_string(),
            models: vec![crate::site::DEFAULT_MODEL.to_string()],
            min_client_version: env!("CARGO_PKG_VERSION").to_string(),
            latest_version: env!("CARGO_PKG_VERSION").to_string(),
            download_url: crate::site::WEBSITE_DOWNLOAD_URL.to_string(),
            website_url: crate::site::WEBSITE_URL.to_string(),
            announcement: String::new(),
            user_agreement: String::new(),
            privacy_policy: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceStatus {
    pub enabled: bool,
    pub status: Option<i64>,
    pub auth_expired: bool,
}

// ---------------- 内部辅助 ----------------

/// 处理登录/2FA 登录成功后的响应：缓存 JWT 与用户信息。
async fn absorb_login_data(state: &AppState, data: &Value) -> AppResult<LoginOutcome> {
    let access_token = data
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::msg("登录响应缺少 access_token 字段"))?;
    let user = data.get("user").cloned().unwrap_or(Value::Null);
    let user_id = user.get("id").and_then(Value::as_i64);
    let username = user
        .get("username")
        .and_then(Value::as_str)
        .map(str::to_string);

    {
        let mut jwt = state.session_jwt.lock().await;
        *jwt = Some(access_token.to_string());
    }
    {
        let mut store = state.store.lock().await;
        store.user_id = user_id;
        store.username = username.clone();
        store.save()?;
    }

    Ok(LoginOutcome {
        require_2fa: false,
        flow_token: None,
        user_id,
        username,
    })
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "sk-****".to_string();
    }
    format!("{}****{}", &key[..6], &key[key.len() - 4..])
}

// ---------------- Tauri commands ----------------

/// GET /api/status（公开接口）：站点能力探测，客户端第一步必调。
#[tauri::command]
pub async fn get_status(state: tauri::State<'_, AppState>) -> Result<Value, AppError> {
    let resp = state
        .http
        .get(api_url("/api/status"))
        .header("User-Agent", UA)
        .send()
        .await?;
    parse_envelope(read_json(resp).await?)
}

/// 比较 x.y.z 版本；current < min 时返回 true（需要升级）
fn version_less_than(current: &str, min: &str) -> bool {
    fn parse(v: &str) -> [u64; 3] {
        let mut out = [0u64; 3];
        for (i, part) in v.split('.').take(3).enumerate() {
            out[i] = part
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0);
        }
        out
    }
    let a = parse(current.trim());
    let b = parse(min.trim());
    a < b
}

pub async fn get_client_config_inner(state: &AppState) -> AppResult<ClientConfig> {
    let resp = state
        .http
        .get(api_url("/api/client_config"))
        .header("User-Agent", UA)
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(ClientConfig::default());
    }
    let data = parse_envelope(read_json(resp).await?)?;
    let mut config: ClientConfig = serde_json::from_value(data)
        .map_err(|e| AppError::msg(format!("客户端配置格式异常：{e}")))?;
    // 服务端未配置下载地址时，回退到编译期官网常量，保证更新弹窗可跳转
    if config.download_url.trim().is_empty() {
        config.download_url = crate::site::WEBSITE_DOWNLOAD_URL.to_string();
    }
    if config.website_url.trim().is_empty() {
        config.website_url = crate::site::WEBSITE_URL.to_string();
    }
    // 版本过低不再在此直接 Err：交给前端弹窗引导官网下载
    Ok(config)
}

#[tauri::command]
pub async fn get_client_config(
    state: tauri::State<'_, AppState>,
) -> Result<ClientConfig, AppError> {
    get_client_config_inner(&state).await
}

/// 当前客户端版本（与 Cargo.toml / tauri.conf 一致）。
#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 用系统默认浏览器打开 http(s) 链接（更新弹窗跳转官网）。
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), AppError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(AppError::msg("链接为空"));
    }
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(AppError::msg("仅允许打开 http(s) 链接"));
    }
    open_url_in_browser(trimmed)
}

fn open_url_in_browser(url: &str) -> AppResult<()> {
    let status = {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", url])
                .spawn()
                .map(|_| ())
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(url)
                .spawn()
                .map(|_| ())
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("xdg-open")
                .arg(url)
                .spawn()
                .map(|_| ())
        }
    };
    status.map_err(|e| AppError::msg(format!("无法打开浏览器：{e}")))
}

/// 供前端判断是否需要更新弹窗。
#[tauri::command]
pub fn check_client_update(config: ClientConfig) -> UpdateCheckResult {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let min = config.min_client_version.trim();
    let latest = {
        let l = config.latest_version.trim();
        if l.is_empty() {
            min
        } else {
            l
        }
    };
    let force = !min.is_empty() && version_less_than(&current, min);
    let soft = !force && !latest.is_empty() && version_less_than(&current, latest);
    let download_url = {
        let d = config.download_url.trim();
        if !d.is_empty() {
            d.to_string()
        } else if !config.website_url.trim().is_empty() {
            config.website_url.trim().to_string()
        } else {
            crate::site::WEBSITE_DOWNLOAD_URL.to_string()
        }
    };
    UpdateCheckResult {
        current_version: current,
        min_client_version: min.to_string(),
        latest_version: latest.to_string(),
        force_update: force,
        soft_update: soft,
        download_url,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub min_client_version: String,
    pub latest_version: String,
    pub force_update: bool,
    pub soft_update: bool,
    pub download_url: String,
}

pub async fn get_user_models_inner(state: &AppState) -> AppResult<Vec<String>> {
    let data = authed_get(state, "/api/user/models").await?;
    let items = data
        .as_array()
        .ok_or_else(|| AppError::msg("可用模型列表格式异常"))?;
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for item in items {
        let Some(model) = item
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if seen.insert(model.to_string()) {
            models.push(model.to_string());
        }
    }
    if models.is_empty() {
        return Err(AppError::msg("当前账号暂无可用模型"));
    }
    Ok(models)
}

fn normalize_model_list(models: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for model in models {
        let model = model.trim().to_string();
        if model.is_empty() {
            continue;
        }
        if seen.insert(model.clone()) {
            result.push(model);
        }
    }
    result
}

async fn get_client_config_models_inner(
    state: &AppState,
) -> AppResult<(Vec<String>, Option<String>)> {
    let config = get_client_config_inner(state).await?;
    let mut available_models = normalize_model_list(config.models);
    let default_model = config.default_model.trim().to_string();
    if available_models.is_empty() && !default_model.is_empty() {
        available_models.push(default_model.clone());
    }
    if available_models.is_empty() {
        return Err(AppError::msg("运营端尚未配置客户端展示模型"));
    }
    let preferred = if !default_model.is_empty()
        && available_models.iter().any(|model| model == &default_model)
    {
        Some(default_model)
    } else {
        None
    };
    Ok((available_models, preferred))
}

pub async fn resolve_model_settings_inner(state: &AppState) -> AppResult<ModelSettings> {
    // 前台是 Smart Agent 自己控制的客户端；模型下拉只读取运营端下发的
    // client_config.models，不能使用 /api/user/models，否则会把 NewAPI 账号层面
    // 可见但运营端未选择的模型展示给用户。
    let (available_models, preferred) = get_client_config_models_inner(state).await?;
    let available: HashSet<&str> = available_models.iter().map(String::as_str).collect();

    let mut store = state.store.lock().await;
    // 单模型策略：只保留一个 active_model，写入 Codex config.model
    let active_model = store
        .default_model
        .as_deref()
        .filter(|model| available.contains(*model))
        .map(str::to_string)
        .or_else(|| {
            store
                .selected_models
                .iter()
                .find(|model| available.contains(model.as_str()))
                .cloned()
        })
        .or_else(|| preferred.filter(|model| available.contains(model.as_str())))
        .unwrap_or_else(|| available_models[0].clone());

    let selected = vec![active_model.clone()];
    if store.selected_models != selected
        || store.default_model.as_deref() != Some(active_model.as_str())
    {
        store.selected_models = selected;
        store.default_model = Some(active_model.clone());
        store.save()?;
    }

    Ok(ModelSettings {
        available_models,
        active_model,
        synced_to_codex: false,
    })
}

#[tauri::command]
pub async fn get_model_settings(
    state: tauri::State<'_, AppState>,
) -> Result<ModelSettings, AppError> {
    let mut settings = resolve_model_settings_inner(&state).await?;
    // 已接入时把唯一 active_model 同步进 Codex（不依赖 Desktop 模型列表 UI）
    settings.synced_to_codex = crate::codex::sync_model_selection(
        settings.active_model.clone(),
        vec![settings.active_model.clone()],
    )
    .await?;
    Ok(settings)
}

#[tauri::command]
pub async fn save_model_settings(
    state: tauri::State<'_, AppState>,
    active_model: String,
) -> Result<ModelSettings, AppError> {
    let (available_models, _) = get_client_config_models_inner(&state).await?;
    let available: HashSet<&str> = available_models.iter().map(String::as_str).collect();
    let active_model = active_model.trim().to_string();
    if active_model.is_empty() {
        return Err(AppError::msg("请选择一个模型"));
    }
    if !available.contains(active_model.as_str()) {
        return Err(AppError::msg("所选模型当前不可用"));
    }

    {
        let mut store = state.store.lock().await;
        store.selected_models = vec![active_model.clone()];
        store.default_model = Some(active_model.clone());
        store.save()?;
    }
    let synced_to_codex =
        crate::codex::sync_model_selection(active_model.clone(), vec![active_model.clone()])
            .await?;

    Ok(ModelSettings {
        available_models,
        active_model,
        synced_to_codex,
    })
}

/// POST /api/user/register。本地/自营站点关闭邮箱验证时仅需用户名+密码。
#[tauri::command]
pub async fn register_account(
    state: tauri::State<'_, AppState>,
    username: String,
    password: String,
    invite_code: Option<String>,
) -> Result<(), AppError> {
    let invite_code = invite_code.unwrap_or_default().trim().to_string();
    let body = if invite_code.is_empty() {
        json!({ "username": username, "password": password })
    } else {
        json!({ "username": username, "password": password, "aff_code": invite_code })
    };
    let resp = state
        .http
        .post(api_url("/api/user/register"))
        .header("User-Agent", UA)
        .json(&body)
        .send()
        .await?;
    parse_envelope(read_json(resp).await?)?;
    Ok(())
}

/// POST /api/user/login。开启 2FA 的账号返回 require_2fa + flow_token（5 分钟有效），
/// 需继续调 login_2fa 提交 TOTP。
#[tauri::command]
pub async fn login(
    state: tauri::State<'_, AppState>,
    username: String,
    password: String,
) -> Result<LoginOutcome, AppError> {
    let resp = state
        .http
        .post(api_url("/api/user/login"))
        .header("User-Agent", UA)
        .json(&json!({ "username": username, "password": password }))
        .send()
        .await?;
    let data = parse_envelope(read_json(resp).await?)?;

    if data.get("require_2fa").and_then(Value::as_bool) == Some(true) {
        let flow_token = data
            .get("flow_token")
            .and_then(Value::as_str)
            .map(str::to_string);
        return Ok(LoginOutcome {
            require_2fa: true,
            flow_token,
            user_id: None,
            username: None,
        });
    }
    absorb_login_data(&state, &data).await
}

/// POST /api/user/login/2fa：提交 TOTP 验证码或备用码完成登录。
#[tauri::command]
pub async fn login_2fa(
    state: tauri::State<'_, AppState>,
    flow_token: String,
    code: String,
) -> Result<LoginOutcome, AppError> {
    let resp = state
        .http
        .post(api_url("/api/user/login/2fa"))
        .header("User-Agent", UA)
        .json(&json!({ "flow_token": flow_token, "code": code }))
        .send()
        .await?;
    let data = parse_envelope(read_json(resp).await?)?;
    absorb_login_data(&state, &data).await
}

/// 确保本地持有可用 PAT。
///
/// 注意：GET /api/user/token 每调用一次都会重新生成 PAT 并【作废旧 PAT】
/// （每用户仅一个，会踢掉其他设备上的旧凭据），因此：
/// 1. 先用本地已存 PAT 调 /api/user/self 验证有效性，有效则直接复用；
/// 2. 仅在本地无 PAT 或已失效时，用登录 JWT 生成一次，严禁每次启动重刷。
#[tauri::command]
pub async fn ensure_pat(state: tauri::State<'_, AppState>) -> Result<PatResult, AppError> {
    // 1) 已有 PAT → 验证
    let (existing_pat, user_id) = {
        let store = state.store.lock().await;
        (store.pat.clone(), store.user_id)
    };
    if let Some(pat) = existing_pat {
        let resp = with_auth(state.http.get(api_url("/api/user/self")), &pat, user_id)
            .send()
            .await;
        if let Ok(resp) = resp {
            if let Ok(body) = read_json(resp).await {
                if body.get("success").and_then(Value::as_bool) == Some(true) {
                    return Ok(PatResult {
                        reused: true,
                        generated: false,
                    });
                }
            }
        }
        // PAT 失效：清掉，走重新生成
        let mut store = state.store.lock().await;
        store.pat = None;
        store.save()?;
    }

    // 2) 用登录 JWT 生成新 PAT（服务端会作废旧 PAT）
    let jwt = {
        let jwt = state.session_jwt.lock().await;
        jwt.clone()
    }
    .ok_or_else(|| AppError::msg("登录状态已过期，请重新登录后再生成访问令牌"))?;

    let resp = with_auth(state.http.get(api_url("/api/user/token")), &jwt, user_id)
        .send()
        .await?;
    let data = parse_envelope(read_json(resp).await?)?;
    let pat = data
        .as_str()
        .ok_or_else(|| AppError::msg("生成访问令牌失败：响应格式异常"))?
        .to_string();

    let mut store = state.store.lock().await;
    store.pat = Some(pat);
    store.save()?;
    Ok(PatResult {
        reused: false,
        generated: true,
    })
}

/// GET /api/user/self：余额（quota）与累计用量。
#[tauri::command]
pub async fn get_self(state: tauri::State<'_, AppState>) -> Result<SelfInfo, AppError> {
    let data = authed_get(&state, "/api/user/self").await?;
    Ok(SelfInfo {
        user_id: data.get("id").and_then(Value::as_i64).unwrap_or_default(),
        username: data
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        display_name: data
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        email: data
            .get("email")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        quota: data
            .get("quota")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        used_quota: data
            .get("used_quota")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        request_count: data
            .get("request_count")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
    })
}

#[tauri::command]
pub async fn update_profile(
    state: tauri::State<'_, AppState>,
    display_name: String,
) -> Result<(), AppError> {
    let self_data = authed_get(&state, "/api/user/self").await?;
    let username = self_data
        .get("username")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::msg("账户信息不完整"))?;
    let (cred, uid) = current_credential(&state).await?;
    let resp = with_auth(state.http.put(api_url("/api/user/self")), &cred, uid)
        .json(&json!({
            "username": username,
            "display_name": display_name.trim(),
            "password": ""
        }))
        .send()
        .await?;
    parse_envelope(read_json(resp).await?)?;
    Ok(())
}

#[tauri::command]
pub async fn change_password(
    state: tauri::State<'_, AppState>,
    original_password: String,
    password: String,
) -> Result<(), AppError> {
    let jwt = state
        .session_jwt
        .lock()
        .await
        .clone()
        .ok_or_else(|| AppError::msg("为保障安全，请退出后重新登录再修改密码"))?;
    let self_data = authed_get(&state, "/api/user/self").await?;
    let username = self_data
        .get("username")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::msg("账户信息不完整"))?;
    let display_name = self_data
        .get("display_name")
        .and_then(Value::as_str)
        .unwrap_or(username);
    let uid = state.store.lock().await.user_id;
    let resp = with_auth(state.http.put(api_url("/api/user/self")), &jwt, uid)
        .json(&json!({
            "username": username,
            "display_name": display_name,
            "original_password": original_password,
            "password": password
        }))
        .send()
        .await?;
    parse_envelope(read_json(resp).await?)?;
    Ok(())
}

/// GET /api/data/self：按日聚合用量，取今日（本地时区 0 点起）消耗。
/// 服务端限制时间跨度 ≤ 1 个月。
#[tauri::command]
pub async fn get_usage(state: tauri::State<'_, AppState>) -> Result<UsageSummary, AppError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| AppError::msg("系统时间异常"))?
        .as_secs() as i64;
    // 近 24 小时窗口内的日聚合数据；QuotaData.created_at 是聚合槽位时间戳，
    // 这里按“今天（UTC 日界）”过滤，雏形阶段先接受时区近似。
    let day_start = now - now % 86400;
    let path = format!(
        "/api/data/self?start_timestamp={}&end_timestamp={}",
        day_start, now
    );
    let data = authed_get(&state, &path).await?;

    let mut today_quota: i64 = 0;
    let mut today_requests: i64 = 0;
    let mut today_tokens: i64 = 0;
    if let Some(rows) = data.as_array() {
        for row in rows {
            today_quota += row.get("quota").and_then(Value::as_i64).unwrap_or(0);
            today_requests += row.get("count").and_then(Value::as_i64).unwrap_or(0);
            today_tokens += row.get("token_used").and_then(Value::as_i64).unwrap_or(0);
        }
    }
    Ok(UsageSummary {
        today_quota,
        today_requests,
        today_tokens,
    })
}

fn value_as_i64(value: Option<&Value>, fallback: i64) -> i64 {
    value
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|raw| raw.parse::<i64>().ok()))
        })
        .unwrap_or(fallback)
}

fn payment_label(method_type: &str) -> &'static str {
    match method_type {
        "alipay" => "支付宝",
        "wxpay" | "wechat" => "微信支付",
        "qqpay" => "QQ 钱包",
        "unionpay" => "银联支付",
        _ => "在线支付",
    }
}

/// 客户端充值能力。技术供应商字段在 Rust 侧收敛，前端只拿到可展示的支付方式。
#[tauri::command]
pub async fn get_topup_info(state: tauri::State<'_, AppState>) -> Result<TopUpInfo, AppError> {
    let data = authed_get(&state, "/api/user/topup/info").await?;
    let online_enabled = data
        .get("enable_online_topup")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let min_amount = value_as_i64(data.get("min_topup"), 1);
    let amount_options = data
        .get("amount_options")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_i64())
                .filter(|amount| *amount > 0)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| vec![10, 20, 50, 100, 200]);

    let methods = if online_enabled {
        data.get("pay_methods")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let method_type = item.get("type")?.as_str()?.trim();
                        if method_type.is_empty() {
                            return None;
                        }
                        Some(TopUpMethod {
                            id: method_type.to_string(),
                            label: payment_label(method_type).to_string(),
                            min_amount: value_as_i64(item.get("min_topup"), min_amount),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(TopUpInfo {
        online_enabled: online_enabled && !methods.is_empty(),
        redemption_enabled: data
            .get("enable_redemption")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        min_amount,
        amount_options,
        methods,
    })
}

#[tauri::command]
pub async fn get_topups(state: tauri::State<'_, AppState>) -> Result<Value, AppError> {
    authed_get(&state, "/api/user/topup/self?p=1&page_size=20").await
}

#[tauri::command]
pub async fn get_invite_info(state: tauri::State<'_, AppState>) -> Result<InviteInfo, AppError> {
    let data = authed_get(&state, "/api/user/invite").await?;
    Ok(InviteInfo {
        aff_code: data
            .get("aff_code")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        inviter_id: data
            .get("inviter_id")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        aff_count: data
            .get("aff_count")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        aff_quota: data
            .get("aff_quota")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        aff_history_quota: data
            .get("aff_history_quota")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        inviter_reward: data
            .get("inviter_reward")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        invitee_reward: data
            .get("invitee_reward")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        enabled: data
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

#[tauri::command]
pub async fn claim_invite_reward(
    state: tauri::State<'_, AppState>,
) -> Result<InviteClaimResult, AppError> {
    let (device_id, token_id) = {
        let mut store = state.store.lock().await;
        let device_id = store.ensure_device_id()?;
        let token_id = store
            .token_id
            .ok_or_else(|| AppError::msg("请先在本机完成服务连接"))?;
        (device_id, token_id)
    };
    let data = authed_post(
        &state,
        "/api/user/invite/claim",
        &json!({ "device_id": device_id, "token_id": token_id }),
    )
    .await?;
    Ok(InviteClaimResult {
        rewarded: data
            .get("rewarded")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        already_done: data
            .get("already_done")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        inviter_quota: data
            .get("inviter_quota")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        invitee_quota: data
            .get("invitee_quota")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        status: data
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        message: data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

#[tauri::command]
pub async fn redeem_topup(state: tauri::State<'_, AppState>, key: String) -> Result<i64, AppError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::msg("请输入兑换码"));
    }
    let data = authed_post(&state, "/api/user/topup", &json!({ "key": key })).await?;
    Ok(value_as_i64(Some(&data), 0))
}

/// 创建充值订单并在 Smart Agent 自有支付窗口中打开收银台。
/// 第三方地址和签名参数仅停留在 Rust 侧，不进入业务 UI。
#[tauri::command]
pub async fn start_topup_checkout(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    amount: i64,
    payment_method: String,
) -> Result<(), AppError> {
    if amount <= 0 {
        return Err(AppError::msg("请输入有效的充值金额"));
    }

    let (cred, uid) = current_credential(&state).await?;
    let resp = with_auth(state.http.post(api_url("/api/user/pay")), &cred, uid)
        .json(&json!({
            "amount": amount,
            "payment_method": payment_method,
        }))
        .send()
        .await?;
    let body = read_json(resp).await?;
    if body.get("message").and_then(Value::as_str) != Some("success") {
        let message = body
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or("暂时无法发起支付");
        return Err(AppError::msg(message.to_string()));
    }

    let raw_url = body
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::msg("支付页面响应异常"))?;
    let mut checkout_url =
        reqwest::Url::parse(raw_url).map_err(|_| AppError::msg("支付页面地址异常"))?;
    if !matches!(checkout_url.scheme(), "http" | "https") {
        return Err(AppError::msg("支付页面地址不受支持"));
    }
    if let Some(params) = body.get("data").and_then(Value::as_object) {
        let mut query = checkout_url.query_pairs_mut();
        for (key, value) in params {
            let value = value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string());
            query.append_pair(key, &value);
        }
    }

    let label = format!(
        "smart-agent-checkout-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| AppError::msg("系统时间异常"))?
            .as_millis()
    );
    tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::External(checkout_url))
        .title("Smart Agent · 安全支付")
        .inner_size(960.0, 720.0)
        .min_inner_size(720.0, 560.0)
        .center()
        .build()
        .map_err(|error| AppError::msg(format!("打开支付窗口失败：{error}")))?;
    Ok(())
}

#[tauri::command]
pub async fn get_open_platform_overview(
    state: tauri::State<'_, AppState>,
) -> Result<OpenPlatformOverview, AppError> {
    let data = authed_get(&state, "/api/open/overview").await?;
    serde_json::from_value(data).map_err(|_| AppError::msg("开放平台配置格式异常"))
}

#[tauri::command]
pub async fn get_open_id_items(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<OpenIdItem>, AppError> {
    let data = authed_get(&state, "/api/open/id/items").await?;
    serde_json::from_value(data).map_err(|_| AppError::msg("ID 数据格式异常"))
}

fn encode_path_segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect::<Vec<_>>()
        .join("")
}

#[tauri::command]
pub async fn reveal_open_id_item(
    state: tauri::State<'_, AppState>,
    item_id: String,
    field: String,
) -> Result<String, AppError> {
    let field = field.trim().to_ascii_lowercase();
    if field != "username" && field != "password" {
        return Err(AppError::msg("仅支持读取账号或密码"));
    }
    let item_id = item_id.trim();
    if item_id.is_empty() {
        return Err(AppError::msg("ID 条目标识不能为空"));
    }
    let data = authed_post(
        &state,
        &format!("/api/open/id/items/{}/reveal", encode_path_segment(item_id)),
        &json!({ "field": field }),
    )
    .await?;
    let result: OpenIdReveal =
        serde_json::from_value(data).map_err(|_| AppError::msg("ID 安全读取结果格式异常"))?;
    if result.value.trim().is_empty() {
        return Err(AppError::msg("服务端未返回可复制内容"));
    }
    Ok(result.value)
}

/// GET /api/token/：令牌列表（脱敏 key）。返回 items 数组。
#[tauri::command]
pub async fn list_tokens(state: tauri::State<'_, AppState>) -> Result<Value, AppError> {
    let data = authed_get(&state, "/api/token/?p=1&page_size=100").await?;
    Ok(data.get("items").cloned().unwrap_or(json!([])))
}

/// POST /api/token/：创建令牌。额度策略：令牌本身不限额（unlimited_quota=true），
/// 消费额度由用户账户余额统一约束（与站点 GENERATE_DEFAULT_TOKEN 行为一致）。
#[tauri::command]
pub async fn create_token(state: tauri::State<'_, AppState>, name: String) -> Result<(), AppError> {
    let body = json!({
        "name": name,
        "expired_time": -1,
        "remain_quota": 0,
        "unlimited_quota": true,
        "model_limits_enabled": false,
        "model_limits": "",
        "group": "",
    });
    authed_post(&state, "/api/token/", &body).await?;
    Ok(())
}

/// POST /api/token/:id/key：取回完整 key（rc.22 起列表 key 脱敏，须用本接口）。
/// 服务端返回的 key 不带 sk- 前缀，这里补齐。挂 CriticalRateLimit，禁止轮询。
#[tauri::command]
pub async fn fetch_token_key(
    state: tauri::State<'_, AppState>,
    token_id: i64,
) -> Result<String, AppError> {
    let data = authed_post(&state, &format!("/api/token/{token_id}/key"), &json!({})).await?;
    let raw = data
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::msg("取回令牌 key 失败：响应格式异常"))?;
    let full = if raw.starts_with("sk-") {
        raw.to_string()
    } else {
        format!("sk-{raw}")
    };
    Ok(full)
}

const ACCOUNT_CLIENT_TOKEN_NAME: &str = "Smart Agent 客户端";

fn token_id(token: &Value) -> Option<i64> {
    token.get("id").and_then(Value::as_i64)
}

fn token_name(token: &Value) -> &str {
    token.get("name").and_then(Value::as_str).unwrap_or("")
}

fn looks_like_legacy_device_token(name: &str) -> bool {
    uuid::Uuid::parse_str(name).is_ok() || name.to_ascii_lowercase().starts_with("smart-agent-")
}

/// 为当前账户选择一把稳定的客户端密钥。
///
/// 选择顺序：
/// 1. 新版固定名称；
/// 2. 本机已经使用过的密钥；
/// 3. 最早创建的旧版设备密钥。
///
/// 这样重装或换设备登录同一账户时，会继续使用原来的客户端密钥，
/// 设备 UUID 只用于设备识别与邀请防刷，不再参与密钥命名。
fn find_account_client_token(items: &Value, local_token_id: Option<i64>) -> Option<(i64, String)> {
    let tokens = items.as_array()?;

    let choose_oldest = |matches: Vec<&Value>| {
        matches
            .into_iter()
            .filter_map(|token| token_id(token).map(|id| (id, token_name(token).to_string())))
            .min_by_key(|(id, _)| *id)
    };

    let exact = tokens
        .iter()
        .filter(|token| token_name(token) == ACCOUNT_CLIENT_TOKEN_NAME)
        .collect::<Vec<_>>();
    if let Some(found) = choose_oldest(exact) {
        return Some(found);
    }

    if let Some(local_id) = local_token_id {
        if let Some(token) = tokens
            .iter()
            .find(|token| token_id(token) == Some(local_id))
        {
            return Some((local_id, token_name(token).to_string()));
        }
    }

    let legacy = tokens
        .iter()
        .filter(|token| looks_like_legacy_device_token(token_name(token)))
        .collect::<Vec<_>>();
    choose_oldest(legacy)
}

/// 自动准备账户级客户端密钥：同一账户始终复用同一把默认密钥。
#[tauri::command]
pub async fn auto_provision(
    state: tauri::State<'_, AppState>,
) -> Result<ProvisionResult, AppError> {
    let local_token_id = {
        let mut store = state.store.lock().await;
        store.ensure_device_id()?;
        store.token_id
    };

    // 1) 先查
    let items = list_tokens_inner(&state).await?;
    let mut created = false;
    let (token_id, name) = match find_account_client_token(&items, local_token_id) {
        Some(found) => found,
        None => {
            // 2) 当前账户没有客户端密钥时才创建；创建后重新选择最早的同名密钥，
            // 避免两台设备同时首次登录时各自继续使用不同密钥。
            let body = json!({
                "name": ACCOUNT_CLIENT_TOKEN_NAME,
                "expired_time": -1,
                "remain_quota": 0,
                "unlimited_quota": true,
                "model_limits_enabled": false,
                "model_limits": "",
                "group": "",
            });
            authed_post(&state, "/api/token/", &body).await?;
            created = true;
            let items = list_tokens_inner(&state).await?;
            find_account_client_token(&items, None)
                .ok_or_else(|| AppError::msg("客户端密钥已创建，但暂时无法读取，请刷新后重试"))?
        }
    };

    // 3) 取完整 key
    let data = authed_post(&state, &format!("/api/token/{token_id}/key"), &json!({})).await?;
    let raw = data
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::msg("取回令牌 key 失败：响应格式异常"))?;
    let full = if raw.starts_with("sk-") {
        raw.to_string()
    } else {
        format!("sk-{raw}")
    };

    let masked = mask_key(&full);
    let mut store = state.store.lock().await;
    store.sk_key = Some(full);
    store.token_id = Some(token_id);
    store.token_name = Some(name.clone());
    store.save()?;

    Ok(ProvisionResult {
        token_id,
        token_name: name,
        masked_key: masked,
        created,
    })
}

#[tauri::command]
pub async fn check_device_status(
    state: tauri::State<'_, AppState>,
) -> Result<DeviceStatus, AppError> {
    let (cred, uid, token_id, token_name) = {
        let store = state.store.lock().await;
        (
            store.pat.clone().ok_or_else(|| AppError::msg("尚未登录"))?,
            store.user_id,
            store.token_id,
            store.token_name.clone(),
        )
    };
    let resp = with_auth(
        state.http.get(api_url("/api/token/?p=1&page_size=100")),
        &cred,
        uid,
    )
    .send()
    .await?;
    if matches!(
        resp.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Ok(DeviceStatus {
            enabled: false,
            status: None,
            auth_expired: true,
        });
    }
    let data = parse_envelope(read_json(resp).await?)?;
    let item = data
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|token| {
                token_id
                    .and_then(|id| token.get("id").and_then(Value::as_i64).map(|v| v == id))
                    .unwrap_or(false)
                    || token_name
                        .as_deref()
                        .and_then(|name| {
                            token
                                .get("name")
                                .and_then(Value::as_str)
                                .map(|value| value == name)
                        })
                        .unwrap_or(false)
            })
        });
    let status = item
        .and_then(|token| token.get("status"))
        .and_then(Value::as_i64);
    Ok(DeviceStatus {
        enabled: status == Some(1),
        status,
        auth_expired: false,
    })
}

async fn list_tokens_inner(state: &AppState) -> AppResult<Value> {
    let data = authed_get(state, "/api/token/?p=1&page_size=100").await?;
    Ok(data.get("items").cloned().unwrap_or(json!([])))
}

#[cfg(test)]
mod account_client_token_tests {
    use super::{find_account_client_token, ACCOUNT_CLIENT_TOKEN_NAME};
    use serde_json::json;

    #[test]
    fn prefers_fixed_account_token_across_installs() {
        let items = json!([
            {"id": 30, "name": "4b163e8c-88d9-4f33-b4fb-c0796b10f2c0"},
            {"id": 20, "name": ACCOUNT_CLIENT_TOKEN_NAME},
            {"id": 10, "name": ACCOUNT_CLIENT_TOKEN_NAME}
        ]);

        assert_eq!(
            find_account_client_token(&items, None),
            Some((10, ACCOUNT_CLIENT_TOKEN_NAME.to_string()))
        );
    }

    #[test]
    fn keeps_the_locally_known_token_during_migration() {
        let items = json!([
            {"id": 5, "name": "普通开发密钥"},
            {"id": 9, "name": "71d737b3-854a-41a2-adce-3eb4426a6be2"}
        ]);

        assert_eq!(
            find_account_client_token(&items, Some(5)),
            Some((5, "普通开发密钥".to_string()))
        );
    }

    #[test]
    fn reuses_the_oldest_legacy_device_token_for_a_fresh_install() {
        let items = json!([
            {"id": 12, "name": "smart-agent-office"},
            {"id": 7, "name": "71d737b3-854a-41a2-adce-3eb4426a6be2"},
            {"id": 3, "name": "手动创建的密钥"}
        ]);

        assert_eq!(
            find_account_client_token(&items, None),
            Some((7, "71d737b3-854a-41a2-adce-3eb4426a6be2".to_string()))
        );
    }
}
