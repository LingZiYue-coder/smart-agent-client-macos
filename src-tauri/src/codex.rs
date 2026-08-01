//! Codex 环境检测与「一键接入」配置写入引擎。
//!
//! 原子写 / 备份 / 回滚 / TOML 校验逻辑借鉴自 cc-switch（MIT License）：
//! vendor/cc-switch/src-tauri/src/codex_config.rs 与 config.rs。
//!
//! 写入规则（对齐 CC Switch 的 Codex 官方登录保留机制）：
//! - 第三方服务凭据写入 provider 的 experimental_bearer_token；
//! - 接入时保留 auth.json 的官方登录态，避免 Codex 桌面端从账号缓存把它恢复后顺手清掉自定义 provider；
//! - 断开或“一键恢复官方登录状态”时清理 Smart Agent provider，并从首次备份恢复 ChatGPT 登录态；
//! - 写之前内部备份原状态，用于失败回滚与找回官方登录态（不向普通用户暴露任意恢复）；
//! - 不写 disable_response_storage（该键已从 Codex 官方配置参考移除）；
//! - 不占用保留 provider id（openai/ollama/lmstudio 等）。
//! - **provider id 必须用 `custom`**：与 CC Switch（MIT）一致。Codex Desktop 对
//!   第三方路由/外部 model_catalog 的展示路径按 custom provider 打通；用
//!   `smartagent` 等自造 id 时后端能跑通，但桌面端模型列表常只剩「自定义」。
//! - 旧版本若曾覆盖登录态，会尝试从最近的安全备份自动恢复。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::site::{v1_base_url, SITE_BASE_URL};
use crate::store::app_data_dir;
use crate::AppState;

/// 与 CC Switch `CC_SWITCH_CODEX_MODEL_PROVIDER_ID` 对齐。
pub const PROVIDER_ID: &str = "custom";
/// 历史版本曾用 smartagent，清理/迁移时一并处理。
const LEGACY_PROVIDER_ID: &str = "smartagent";
const CC_SWITCH_PROVIDER_ID: &str = "custom";
const BACKUP_KEEP: usize = 10;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ---------------- 基础文件工具 ----------------

/// 原子写入：同目录临时文件 + rename 替换，避免半写状态。
/// （借鉴 cc-switch config.rs::atomic_write；Windows 上 rename 目标存在会失败，先移除再重命名）
pub fn atomic_write(path: &Path, data: &[u8]) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::msg("无效的目标路径"))?;
    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;

    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::msg("无效的文件名"))?
        .to_string_lossy()
        .to_string();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = parent.join(format!("{file_name}.tmp.{ts}"));

    {
        let mut f = fs::File::create(&tmp).map_err(|e| AppError::io(&tmp, e))?;
        f.write_all(data).map_err(|e| AppError::io(&tmp, e))?;
        f.flush().map_err(|e| AppError::io(&tmp, e))?;
    }

    #[cfg(windows)]
    {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }

    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        AppError::io(path, e)
    })
}

fn restore_optional_file(path: &Path, bytes: Option<&[u8]>) {
    match bytes {
        Some(bytes) => {
            let _ = atomic_write(path, bytes);
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

/// Codex 配置根目录：$CODEX_HOME，默认 ~/.codex。
pub fn codex_home() -> PathBuf {
    if let Ok(custom) = std::env::var("CODEX_HOME") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

fn config_path() -> PathBuf {
    codex_home().join("config.toml")
}

fn auth_path() -> PathBuf {
    codex_home().join("auth.json")
}

fn model_catalog_path() -> PathBuf {
    codex_home().join("smart-agent-models.json")
}

fn backups_root() -> AppResult<PathBuf> {
    Ok(app_data_dir()?.join("backups"))
}

fn latest_chatgpt_auth_backup() -> AppResult<Option<Vec<u8>>> {
    let root = backups_root()?;
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(None);
    };
    let mut candidates = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let timestamp = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.parse::<i64>().ok())?;
            path.is_dir().then_some((timestamp, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.0.cmp(&left.0));

    for (_, directory) in candidates {
        let path = directory.join("auth.json");
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        let has_login = serde_json::from_slice::<Value>(&bytes)
            .map(|auth| auth_has_chatgpt_login(&auth))
            .unwrap_or(false);
        if has_login {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

// ---------------- Codex CLI 检测 ----------------

fn version_from_output(out: std::process::Output) -> Option<String> {
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

#[cfg(windows)]
fn run_version(candidate: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    // npm 全局安装的是 codex.cmd，必须经 cmd.exe 启动；加 CREATE_NO_WINDOW 防止闪黑窗。
    let out = Command::new("cmd")
        .args(["/C", &format!("\"{candidate}\" --version")])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    version_from_output(out)
}

#[cfg(not(windows))]
fn run_version(candidate: &str) -> Option<String> {
    let out = Command::new(candidate).arg("--version").output().ok()?;
    version_from_output(out)
}

/// 依次尝试 PATH 中的 codex 与 npm 全局目录，返回 (可执行标识, 版本, 是否 npm 全局)。
fn detect_codex_cli() -> (Option<String>, Option<String>, bool) {
    // 1) PATH
    if let Some(version) = run_version("codex") {
        return (Some("codex".to_string()), Some(version), false);
    }
    // 2) npm 全局（GUI 进程 PATH 常缺 npm 目录）
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            for name in ["codex.cmd", "codex.exe"] {
                let candidate = PathBuf::from(&appdata).join("npm").join(name);
                if candidate.exists() {
                    let candidate_str = candidate.to_string_lossy().to_string();
                    let version = run_version(&candidate_str);
                    return (Some(candidate_str), version, true);
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![
            PathBuf::from("/Applications/Codex.app/Contents/Resources/codex"),
            PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex"),
            PathBuf::from("/opt/homebrew/bin/codex"),
            PathBuf::from("/usr/local/bin/codex"),
        ];
        if let Some(home) = dirs::home_dir() {
            candidates.insert(
                0,
                home.join("Applications/Codex.app/Contents/Resources/codex"),
            );
            candidates.insert(
                1,
                home.join("Applications/ChatGPT.app/Contents/Resources/codex"),
            );
        }
        for candidate in candidates {
            if candidate.is_file() {
                let candidate = candidate.to_string_lossy().to_string();
                let version = run_version(&candidate);
                return (Some(candidate), version, false);
            }
        }
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        for candidate in ["/opt/homebrew/bin/codex", "/usr/local/bin/codex"] {
            if Path::new(candidate).exists() {
                let version = run_version(candidate);
                return (Some(candidate.to_string()), version, false);
            }
        }
    }
    (None, None, false)
}

fn auth_has_chatgpt_login(auth: &Value) -> bool {
    match auth.get("tokens") {
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexEnv {
    pub installed: bool,
    pub desktop_installed: bool,
    pub cli_path: Option<String>,
    pub version: Option<String>,
    pub npm_global: bool,
    pub codex_home: String,
    pub config_exists: bool,
    pub auth_exists: bool,
    /// config.toml 中的 cli_auth_credentials_store（file|keyring|auto，缺省为 file 语义）。
    pub credentials_store: Option<String>,
    /// credentials_store 为 keyring 时为 true：直写 auth.json 可能不生效，需提示。
    pub keyring_warning: bool,
    /// 现有 auth.json 含 ChatGPT 登录态（tokens 字段）。
    pub has_chatgpt_login: bool,
    /// config.toml 已指向本站（model_provider = smartagent 且 base_url 匹配）。
    pub provider_configured: bool,
    pub config_parse_error: Option<String>,
}

#[tauri::command]
pub async fn detect_codex() -> Result<CodexEnv, AppError> {
    tauri::async_runtime::spawn_blocking(detect_codex_blocking)
        .await
        .map_err(|e| AppError::msg(format!("环境检测任务失败：{e}")))?
}

fn detect_codex_blocking() -> Result<CodexEnv, AppError> {
    let (cli_path, version, npm_global) = detect_codex_cli();
    let desktop_installed = codex_desktop_installed_blocking();
    let home = codex_home();
    let cfg_path = config_path();
    let auth_p = auth_path();

    let mut credentials_store = None;
    let mut provider_configured = false;
    let mut config_parse_error = None;

    if cfg_path.exists() {
        match fs::read_to_string(&cfg_path) {
            Ok(text) => match text.parse::<toml::Table>() {
                Ok(table) => {
                    credentials_store = table
                        .get("cli_auth_credentials_store")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    provider_configured = table_points_to_smartagent(&table);
                }
                Err(e) => config_parse_error = Some(e.to_string()),
            },
            Err(e) => config_parse_error = Some(e.to_string()),
        }
    }

    let has_chatgpt_login = if auth_p.exists() {
        fs::read_to_string(&auth_p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| auth_has_chatgpt_login(&v))
            .unwrap_or(false)
    } else {
        false
    };

    let keyring_warning = matches!(credentials_store.as_deref(), Some("keyring"));

    Ok(CodexEnv {
        installed: cli_path.is_some() || desktop_installed,
        desktop_installed,
        cli_path,
        version,
        npm_global,
        codex_home: home.display().to_string(),
        config_exists: cfg_path.exists(),
        auth_exists: auth_p.exists(),
        credentials_store,
        keyring_warning,
        has_chatgpt_login,
        provider_configured,
        config_parse_error,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexInstallResult {
    pub ok: bool,
    pub message: String,
}

/// 从 Microsoft Store 官方源自动安装 Codex / ChatGPT 桌面包。
///
/// Windows Store 当前公开包名为 ChatGPT，PackageFamilyName 为 OpenAI.Codex。
/// 使用 winget + msstore 源可以自动拉取当前最新 MSIX，不硬编码非官方安装包。
#[tauri::command]
pub async fn install_codex_desktop() -> Result<CodexInstallResult, AppError> {
    tauri::async_runtime::spawn_blocking(install_codex_desktop_blocking)
        .await
        .map_err(|e| AppError::msg(format!("安装任务失败：{e}")))?
}

#[cfg(windows)]
fn install_codex_desktop_blocking() -> Result<CodexInstallResult, AppError> {
    use std::os::windows::process::CommandExt;

    if codex_desktop_installed_blocking() {
        return Ok(CodexInstallResult {
            ok: true,
            message: "Codex 客户端已安装".to_string(),
        });
    }

    let output = Command::new("winget.exe")
        .args([
            "install",
            "--id",
            "9PLM9XGG6VKS",
            "--source",
            "msstore",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| {
            AppError::msg(format!(
                "未找到 Windows 官方包管理器 winget，请使用「官方安装」打开微软商店：{error}"
            ))
        })?;

    if output.status.success() || codex_desktop_installed_blocking() {
        return Ok(CodexInstallResult {
            ok: true,
            message: "Codex 客户端安装完成".to_string(),
        });
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = [stderr.trim(), stdout.trim()]
        .into_iter()
        .find(|text| !text.is_empty())
        .unwrap_or("winget 安装未完成");
    Err(AppError::msg(format!(
        "自动安装失败，请使用「官方安装」打开微软商店。{detail}"
    )))
}

#[cfg(target_os = "macos")]
fn install_codex_desktop_blocking() -> Result<CodexInstallResult, AppError> {
    if codex_desktop_installed_blocking() {
        return Ok(CodexInstallResult {
            ok: true,
            message: "Codex 客户端已安装".to_string(),
        });
    }

    if let (Some(cli), _, _) = detect_codex_cli() {
        Command::new(cli)
            .arg("app")
            .spawn()
            .map_err(|error| AppError::msg(format!("打开 Codex 官方安装程序失败：{error}")))?;
        return Ok(CodexInstallResult {
            ok: true,
            message: "已打开 Codex 官方安装程序，请完成安装后重新检测".to_string(),
        });
    }

    let status = Command::new("open")
        .arg("https://chatgpt.com/download/")
        .status()
        .map_err(|error| AppError::msg(format!("打开 Codex 官方下载页失败：{error}")))?;
    if !status.success() {
        return Err(AppError::msg("无法打开 Codex 官方下载页"));
    }
    Ok(CodexInstallResult {
        ok: true,
        message: "已打开 Codex 官方下载页，请完成安装后重新检测".to_string(),
    })
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn install_codex_desktop_blocking() -> Result<CodexInstallResult, AppError> {
    Err(AppError::msg("当前系统暂不支持自动安装 Codex 客户端"))
}

// ---------------- 配置生成 ----------------

fn provider_base_url_matches(table: &toml::Table, provider_id: &str) -> bool {
    table
        .get("model_providers")
        .and_then(|value| value.get(provider_id))
        .and_then(|value| value.get("base_url"))
        .and_then(|value| value.as_str())
        == Some(v1_base_url().as_str())
}

fn table_points_to_smartagent(table: &toml::Table) -> bool {
    let active = table
        .get("model_provider")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    // 新：custom；旧：smartagent（迁移窗口内仍算已接入）
    let id_ok = active == PROVIDER_ID || active == LEGACY_PROVIDER_ID;
    id_ok && provider_base_url_matches(table, active)
}

fn config_bytes_point_to_smartagent(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes)
        .parse::<toml::Table>()
        .map(|table| table_points_to_smartagent(&table))
        .unwrap_or(false)
}

/// 在现有 config.toml 基础上合并写入 Smart Agent provider（保留用户其他设置；
/// 注释与键顺序不保留 —— 写入前已有完整备份）。返回新的 TOML 文本。
///
/// bearer_token 参数保留用于兼容，但当前实现使用 requires_openai_auth = true 方式，
/// sk-key 会直接写入 auth.json 的 OPENAI_API_KEY 字段，这是最稳定的方式。
/// 生成 config.toml 文本（CC Switch 方案）：
/// - bearer_token 参数用于写入 experimental_bearer_token
/// - 不使用 requires_openai_auth，因为我们把 token 放在 provider 配置里
fn build_config_text(
    existing_text: &str,
    model: &str,
    bearer_token: Option<&str>,
) -> AppResult<String> {
    let mut table: toml::Table = if existing_text.trim().is_empty() {
        toml::Table::new()
    } else {
        existing_text
            .parse::<toml::Table>()
            .map_err(|e| AppError::toml(&config_path(), e))?
    };

    // 顶层：模型与当前 provider（模型名必须是站点实际可用模型）
    table.insert("model".into(), toml::Value::String(model.into()));
    table.insert(
        "model_provider".into(),
        toml::Value::String(PROVIDER_ID.into()),
    );
    table.insert(
        "model_catalog_json".into(),
        // 与 CC Switch 一致使用相对 CODEX_HOME 的文件名，避免桌面沙箱对绝对路径
        // 解析行为与 CLI 不一致。
        toml::Value::String("smart-agent-models.json".into()),
    );
    // 旧客户端曾写入 service_tier = "default"，新版 Codex 只接受 fast/flex。
    if table.get("service_tier").and_then(|value| value.as_str()) == Some("default") {
        table.remove("service_tier");
    }

    // [model_providers.custom] —— 与 CC Switch 一致：
    // token 写 experimental_bearer_token；Desktop 按 custom id 加载外部目录。
    let mut provider = table
        .get("model_providers")
        .and_then(|value| value.get(PROVIDER_ID))
        .and_then(|value| value.as_table())
        .cloned()
        .or_else(|| {
            table
                .get("model_providers")
                .and_then(|value| value.get(LEGACY_PROVIDER_ID))
                .and_then(|value| value.as_table())
                .cloned()
        })
        .unwrap_or_default();
    provider.insert("name".into(), toml::Value::String("Smart Agent".into()));
    provider.insert("base_url".into(), toml::Value::String(v1_base_url()));
    provider.insert("wire_api".into(), toml::Value::String("responses".into()));

    if let Some(token) = bearer_token {
        provider.insert(
            "experimental_bearer_token".into(),
            toml::Value::String(token.into()),
        );
    }

    provider.remove("requires_openai_auth");
    provider.remove("env_key");

    let providers = table
        .entry("model_providers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    match providers {
        toml::Value::Table(map) => {
            map.insert(PROVIDER_ID.into(), toml::Value::Table(provider));
            // 清掉旧 smartagent 表，避免双 provider 并存
            map.remove(LEGACY_PROVIDER_ID);
        }
        _ => {
            return Err(AppError::msg(
                "现有 config.toml 的 model_providers 字段类型异常，无法合并写入",
            ))
        }
    }

    let text = toml::to_string_pretty(&table)
        .map_err(|e| AppError::msg(format!("生成 config.toml 失败：{e}")))?;
    // 写出前用 toml crate 再校验一遍
    text.parse::<toml::Table>()
        .map_err(|e| AppError::toml(&config_path(), e))?;
    Ok(text)
}

/// 与 CC Switch `resources/codex_native_responses_template.json` 对齐的精简模板。
///
/// **禁止**从 `models_cache.json` 整表克隆：那是官方在线目录的 ModelInfo 超集，
/// 外部 `model_catalog_json` 解析器字段集更窄，塞进 apply_patch/model_messages/
/// service_tiers 等会整文件拒载，Desktop 模型列表就只剩「自定义」。
fn native_responses_model_template() -> Value {
    json!({
        "slug": "native-responses-template",
        "display_name": "native-responses-template",
        "description": "native-responses-template",
        "base_instructions": "You are Codex, a coding agent. You and the user share the same workspace and collaborate to achieve the user's goals.",
        "default_reasoning_level": "high",
        "supported_reasoning_levels": [
            { "effort": "none", "description": "Disable Thinking" },
            { "effort": "low", "description": "Fast responses with lighter reasoning" },
            { "effort": "medium", "description": "Balances speed and reasoning depth" },
            { "effort": "high", "description": "Greater reasoning depth for complex problems" },
            { "effort": "xhigh", "description": "Extra high reasoning depth" }
        ],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 0,
        "supports_reasoning_summaries": true,
        "default_reasoning_summary": "none",
        "support_verbosity": false,
        "truncation_policy": { "mode": "bytes", "limit": 10000 },
        "supports_parallel_tool_calls": false,
        "supports_image_detail_original": false,
        "context_window": 262144,
        "max_context_window": 262144,
        "effective_context_window_percent": 95,
        "experimental_supported_tools": [],
        "input_modalities": ["text", "image"],
        "supports_search_tool": false
    })
}

fn build_model_catalog(models: &[String], default_model: &str) -> AppResult<String> {
    // 单模型策略：目录里只放当前 active 模型。Desktop 列表不可靠，
    // 真正生效的是 config.toml 的 model=；目录尽量只含一条避免干扰。
    let active = if models.iter().any(|m| m == default_model) {
        default_model.to_string()
    } else if let Some(first) = models.first() {
        first.clone()
    } else {
        default_model.to_string()
    };
    let ordered_models = vec![active];

    let template = native_responses_model_template();

    let models = ordered_models
        .iter()
        .enumerate()
        .map(|(index, model)| {
            let mut entry = template.clone();
            if let Some(object) = entry.as_object_mut() {
                object.insert("slug".into(), Value::String(model.clone()));
                object.insert("display_name".into(), Value::String(model.clone()));
                object.insert("description".into(), Value::String(model.clone()));
                object.insert("context_window".into(), json!(262144));
                object.insert("max_context_window".into(), json!(262144));
                object.insert("visibility".into(), Value::String("list".into()));
                object.insert("supported_in_api".into(), Value::Bool(true));
                object.insert("priority".into(), json!(1000 + index));
                object.insert("upgrade".into(), Value::Null);
                object.insert("additional_speed_tiers".into(), json!([]));
                object.insert("service_tiers".into(), json!([]));
                object.insert("availability_nux".into(), Value::Null);
            }
            entry
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&json!({ "models": models }))
        .map_err(|error| AppError::msg(format!("生成 Codex 模型目录失败：{error}")))
}

fn mask_middle(s: &str) -> String {
    if s.len() <= 10 {
        return "sk-****".to_string();
    }
    format!("{}****{}", &s[..7], &s[s.len() - 4..])
}

fn config_without_smartagent_live_token(config_text: &str) -> AppResult<String> {
    let mut table = config_text
        .parse::<toml::Table>()
        .map_err(|error| AppError::toml(&config_path(), error))?;
    if let Some(toml::Value::Table(providers)) = table.get_mut("model_providers") {
        if let Some(toml::Value::Table(provider)) = providers.get_mut(PROVIDER_ID) {
            provider.remove("experimental_bearer_token");
        }
    }
    toml::to_string_pretty(&table)
        .map_err(|error| AppError::msg(format!("生成 CC Switch Codex 配置失败：{error}")))
}

fn cc_switch_root() -> Option<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".cc-switch"))
        .filter(|path| path.is_dir())
}

fn cc_switch_model_catalog_settings(models: &[String]) -> Value {
    let entries = models
        .iter()
        .filter(|model| !model.trim().is_empty())
        .map(|model| {
            json!({
                "model": model,
                "displayName": model,
                "contextWindow": 262144,
                "inputModalities": ["text", "image"]
            })
        })
        .collect::<Vec<_>>();
    json!({ "models": entries })
}

fn update_cc_switch_settings_file(root: &Path, provider_id: &str) -> AppResult<()> {
    let path = root.join("settings.json");
    if !path.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(&path).map_err(|error| AppError::io(&path, error))?;
    let mut settings = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({}));
    if !settings.is_object() {
        settings = json!({});
    }
    if let Some(object) = settings.as_object_mut() {
        object.insert(
            "currentProviderCodex".to_string(),
            Value::String(provider_id.to_string()),
        );
    }
    let bytes = serde_json::to_vec_pretty(&settings)?;
    atomic_write(&path, &bytes)
}

fn sync_cc_switch_codex_state(
    stored_config_text: &str,
    sk_key: &str,
    models: &[String],
) -> AppResult<bool> {
    let Some(root) = cc_switch_root() else {
        return Ok(false);
    };
    let db_path = root.join("cc-switch.db");
    if !db_path.exists() {
        return Ok(false);
    }

    let settings_config = json!({
        "auth": { "OPENAI_API_KEY": sk_key },
        "config": stored_config_text,
        "modelCatalog": cc_switch_model_catalog_settings(models),
    });
    let settings_json = serde_json::to_string(&settings_config)?;
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let mut conn = Connection::open(&db_path)
        .map_err(|error| AppError::msg(format!("打开 CC Switch 数据库失败：{error}")))?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|error| AppError::msg(format!("等待 CC Switch 数据库失败：{error}")))?;
    let tx = conn
        .transaction()
        .map_err(|error| AppError::msg(format!("更新 CC Switch 状态失败：{error}")))?;
    tx.execute(
        "UPDATE providers SET is_current = 0 WHERE app_type = 'codex'",
        [],
    )
    .map_err(|error| AppError::msg(format!("更新 CC Switch 当前供应商失败：{error}")))?;
    tx.execute(
        "INSERT INTO providers (
            id, app_type, name, settings_config, website_url, category,
            created_at, sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue
        ) VALUES (?1, 'codex', 'Smart Agent', ?2, NULL, 'custom', ?3, 1, NULL, NULL, NULL, '{}', 1, 0)
        ON CONFLICT(id, app_type) DO UPDATE SET
            name = excluded.name,
            settings_config = excluded.settings_config,
            category = excluded.category,
            sort_index = excluded.sort_index,
            meta = excluded.meta,
            is_current = 1,
            in_failover_queue = 0",
        params![CC_SWITCH_PROVIDER_ID, settings_json, created_at],
    )
    .map_err(|error| AppError::msg(format!("写入 CC Switch Smart Agent 供应商失败：{error}")))?;
    tx.commit()
        .map_err(|error| AppError::msg(format!("保存 CC Switch 状态失败：{error}")))?;

    update_cc_switch_settings_file(&root, CC_SWITCH_PROVIDER_ID)?;
    Ok(true)
}

fn sync_cc_switch_codex_official_state() -> AppResult<bool> {
    let Some(root) = cc_switch_root() else {
        return Ok(false);
    };
    let db_path = root.join("cc-switch.db");
    if !db_path.exists() {
        return Ok(false);
    }

    let mut conn = Connection::open(&db_path)
        .map_err(|error| AppError::msg(format!("打开 CC Switch 数据库失败：{error}")))?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|error| AppError::msg(format!("等待 CC Switch 数据库失败：{error}")))?;
    let tx = conn
        .transaction()
        .map_err(|error| AppError::msg(format!("更新 CC Switch 状态失败：{error}")))?;
    tx.execute(
        "UPDATE providers SET is_current = CASE WHEN id = 'codex-official' THEN 1 ELSE 0 END WHERE app_type = 'codex'",
        [],
    )
    .map_err(|error| AppError::msg(format!("恢复 CC Switch 官方供应商失败：{error}")))?;
    tx.commit()
        .map_err(|error| AppError::msg(format!("保存 CC Switch 官方状态失败：{error}")))?;

    update_cc_switch_settings_file(&root, "codex-official")?;
    Ok(true)
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyPreview {
    /// 将写入的 config.toml 摘要（站点地址已脱敏，UI 不展示可改地址）。
    pub config_preview: String,
    /// 将写入的 auth.json 摘要（key 已脱敏）。
    pub auth_preview: String,
    pub has_chatgpt_login: bool,
    pub keyring_warning: bool,
    pub codex_home: String,
}

/// 一键接入前的预览：展示将写入的配置摘要（脱敏），不落盘。
#[tauri::command]
pub async fn plan_codex_config(
    state: tauri::State<'_, AppState>,
) -> Result<ApplyPreview, AppError> {
    let model_settings = crate::api::resolve_model_settings_inner(&state).await?;
    let sk_key = {
        let store = state.store.lock().await;
        store
            .sk_key
            .clone()
            .ok_or_else(|| AppError::msg("尚未获取 API 令牌，请先完成上一步「自动开卡」"))?
    };

    let env = detect_codex_blocking()?;
    let existing = if config_path().exists() {
        fs::read_to_string(config_path()).unwrap_or_default()
    } else {
        String::new()
    };
    // 用户原配置解析失败时按空配置生成（原文件已在应用时备份）
    let masked_key = mask_middle(&sk_key);
    let config_text = build_config_text(&existing, &model_settings.active_model, Some(&masked_key))
        .or_else(|_| build_config_text("", &model_settings.active_model, Some(&masked_key)))?;

    // 铁律：UI 不出现站点地址 —— 预览中将编译期站点地址和 token 整体脱敏
    let redacted = config_text.replace(SITE_BASE_URL, "<内置站点地址>");
    let auth_preview =
        "保留 Codex 官方登录态；第三方令牌写入 config.toml（Desktop 模型列表门控需要官方登录）"
            .to_string();

    Ok(ApplyPreview {
        config_preview: redacted,
        auth_preview,
        has_chatgpt_login: env.has_chatgpt_login,
        keyring_warning: env.keyring_warning,
        codex_home: env.codex_home,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyResult {
    pub backup_dir: Option<String>,
    /// 兼容旧前端：新版不再覆盖官方登录态，始终为 false。
    pub chatgpt_login_overwritten: bool,
    /// 官方登录态是否已备份，可由“一键恢复官方登录状态”还原。
    pub official_login_preserved: bool,
    /// 桌面端模型选择器是否具备显示 Smart Agent 模型目录所需的 provider token。
    pub desktop_model_picker_ready: bool,
    /// 检测到 CC Switch 时，是否已同步其 Codex 当前供应商状态。
    pub cc_switch_synced: bool,
    /// 本次是否自动关闭并重新启动了 Codex 桌面端。
    pub codex_restarted: bool,
    pub keyring_warning: bool,
}

/// 一键接入：写 provider + 模型目录（保留官方 auth）。
///
/// **刻意不做**「关 Codex → 启动 → 盯 2 分钟防洗」：那会让 UI 长时间停在
/// 「正在自动完成设置…」像卡死。防洗交给后台 `ensure_codex_config`（且仅
/// `codex_desired=true` 时）。用户需自行完全退出再开 Codex 以刷新模型列表。
#[tauri::command]
pub async fn apply_codex_config(
    state: tauri::State<'_, AppState>,
) -> Result<ApplyResult, AppError> {
    let model_settings = crate::api::resolve_model_settings_inner(&state).await?;
    let sk_key = {
        let store = state.store.lock().await;
        store
            .sk_key
            .clone()
            .ok_or_else(|| AppError::msg("尚未获取 API 令牌，请先完成「自动开卡」"))?
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        apply_codex_config_blocking(
            &sk_key,
            &model_settings.active_model,
            &[model_settings.active_model.clone()],
        )
    })
    .await
    .map_err(|e| AppError::msg(format!("配置写入任务失败：{e}")))??;

    {
        let mut store = state.store.lock().await;
        store.codex_desired = true;
        let _ = store.save();
    }
    Ok(result)
}

/// 一键接入核心逻辑（对齐 CC Switch 官方 FAQ / 保留官方登录攻略）：
/// - config.toml：provider=`custom` + experimental_bearer_token + model_catalog_json
/// - auth.json：**默认不动**，保留 ChatGPT / Codex 官方登录态
///
/// 依据：`vendor/cc-switch/docs/guides/codex-desktop-custom-model-visibility-zh.md`
/// Codex Desktop 模型选择器按**登录身份门控**——检测不到官方登录时，会把
/// 自定义模型藏起来，只剩官方默认/「自定义」。官方已标 not planned。
/// 缓解办法就是保留 auth.json 官方 tokens，第三方 key 只写 config.toml。
fn apply_codex_config_blocking(
    sk_key: &str,
    model: &str,
    models: &[String],
) -> Result<ApplyResult, AppError> {
    let cfg_path = config_path();
    let auth_p = auth_path();
    fs::create_dir_all(codex_home()).map_err(|e| AppError::io(&codex_home(), e))?;

    // 0) 读原文件（备份 + 回滚 + ChatGPT 登录态检测）
    let old_config = if cfg_path.exists() {
        Some(fs::read(&cfg_path).map_err(|e| AppError::io(&cfg_path, e))?)
    } else {
        None
    };
    let old_auth = if auth_p.exists() {
        Some(fs::read(&auth_p).map_err(|e| AppError::io(&auth_p, e))?)
    } else {
        None
    };
    let catalog_p = model_catalog_path();
    let old_catalog = if catalog_p.exists() {
        Some(fs::read(&catalog_p).map_err(|e| AppError::io(&catalog_p, e))?)
    } else {
        None
    };
    let live_has_chatgpt_login = old_auth
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .map(|v| auth_has_chatgpt_login(&v))
        .unwrap_or(false);

    // 若当前 auth 已被洗成纯 API Key，尽量从备份找回官方 tokens 再写回
    // （Desktop 门控需要官方登录身份才能展示自定义模型列表）
    let restored_official_auth = if !live_has_chatgpt_login {
        if let Some(bytes) = latest_chatgpt_auth_backup()? {
            atomic_write(&auth_p, &bytes)?;
            true
        } else {
            false
        }
    } else {
        false
    };
    let has_chatgpt_login = live_has_chatgpt_login || restored_official_auth;

    let env = detect_codex_blocking()?;

    // 1) 每次接入都备份 config（轮转 10 份）；auth 有官方登录时也备份
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let dir = backups_root()?.join(ts.to_string());
    fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    if let Some(bytes) = &old_config {
        fs::write(dir.join("config.toml"), bytes).map_err(|e| AppError::io(&dir, e))?;
    }
    // 备份「接入前」auth；若刚从备份恢复，写恢复后的内容
    let auth_for_backup = fs::read(&auth_p).ok().or(old_auth.clone());
    if let Some(bytes) = &auth_for_backup {
        fs::write(dir.join("auth.json"), bytes).map_err(|e| AppError::io(&dir, e))?;
    }
    rotate_backups()?;
    let backup_dir = Some(dir.display().to_string());

    // 2) 生成 config + 模型目录（auth.json 不覆盖）
    let existing_text = old_config
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .unwrap_or_default();
    let config_text = build_config_text(&existing_text, model, Some(sk_key))
        .or_else(|_| build_config_text("", model, Some(sk_key)))?;
    let stored_config_text = config_without_smartagent_live_token(&config_text)?;
    let catalog_text = build_model_catalog(models, model)?;

    // 3) 只写模型目录 + config.toml；失败回滚目录
    atomic_write(&catalog_p, catalog_text.as_bytes())?;
    if let Err(e) = atomic_write(&cfg_path, config_text.as_bytes()) {
        restore_optional_file(&catalog_p, old_catalog.as_deref());
        return Err(e);
    }

    let cc_switch_synced =
        sync_cc_switch_codex_state(&stored_config_text, sk_key, models).unwrap_or(false);

    set_codex_desired_on_disk(true)?;

    Ok(ApplyResult {
        backup_dir,
        // 新策略不再覆盖官方登录
        chatgpt_login_overwritten: false,
        official_login_preserved: has_chatgpt_login,
        // 无官方登录时 Desktop 门控仍可能藏模型列表，UI 需提示用户先登录 ChatGPT
        desktop_model_picker_ready: has_chatgpt_login,
        cc_switch_synced,
        codex_restarted: false,
        keyring_warning: env.keyring_warning,
    })
}

pub fn apply_codex_config_from_local_store_blocking() -> AppResult<ApplyResult> {
    let store = crate::store::LocalStore::load();
    let sk_key = store
        .sk_key
        .ok_or_else(|| AppError::msg("本机尚未保存 API 令牌，请先在 Smart Agent 登录"))?;
    let active = store
        .default_model
        .or_else(|| store.selected_models.first().cloned())
        .unwrap_or_else(|| "gpt-5".to_string());
    apply_codex_config_blocking(&sk_key, &active, &[active.clone()])
}

/// 已接入时只刷新模型目录与默认模型，不重复覆盖凭据或创建备份。
pub async fn sync_model_selection(
    default_model: String,
    selected_models: Vec<String>,
) -> Result<bool, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        sync_model_selection_blocking(&default_model, &selected_models)
    })
    .await
    .map_err(|error| AppError::msg(format!("模型同步任务失败：{error}")))?
}

fn sync_model_selection_blocking(
    default_model: &str,
    selected_models: &[String],
) -> AppResult<bool> {
    // 用户已断开时不要偷偷改 Codex 配置
    if !crate::store::LocalStore::load().codex_desired {
        return Ok(false);
    }
    let cfg_path = config_path();
    if !cfg_path.exists() {
        return Ok(false);
    }
    let existing = fs::read_to_string(&cfg_path).map_err(|error| AppError::io(&cfg_path, error))?;
    let table = existing
        .parse::<toml::Table>()
        .map_err(|error| AppError::toml(&cfg_path, error))?;
    if !table_points_to_smartagent(&table) {
        return Ok(false);
    }

    let catalog_p = model_catalog_path();
    let old_catalog = fs::read(&catalog_p).ok();
    let old_config = fs::read(&cfg_path).ok();
    let catalog_text = build_model_catalog(selected_models, default_model)?;
    let config_text = build_config_text(&existing, default_model, None)?;
    atomic_write(&catalog_p, catalog_text.as_bytes())?;
    if let Err(error) = atomic_write(&cfg_path, config_text.as_bytes()) {
        restore_optional_file(&catalog_p, old_catalog.as_deref());
        restore_optional_file(&cfg_path, old_config.as_deref());
        return Err(error);
    }
    Ok(true)
}

#[tauri::command]
pub async fn disconnect_codex(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    // 先清意图再落盘清理，杜绝 ensure 竞态回写
    {
        let mut store = state.store.lock().await;
        store.codex_desired = false;
        store.save()?;
    }
    tauri::async_runtime::spawn_blocking(disconnect_codex_blocking)
        .await
        .map_err(|error| AppError::msg(format!("断开连接任务失败：{error}")))?
}

fn set_codex_desired_on_disk(desired: bool) -> AppResult<()> {
    let mut store = crate::store::LocalStore::load();
    store.codex_desired = desired;
    store.save()
}

fn disconnect_codex_blocking() -> AppResult<()> {
    set_codex_desired_on_disk(false)?;
    restore_official_codex_login_blocking().map(|_| ())
}

/// 检测可能覆写 Codex 配置的桌面端进程。
fn codex_desktop_running_blocking() -> AppResult<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| AppError::msg(format!("检查运行状态失败：{e}")))?;
        let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        Ok(text.contains("\"chatgpt.exe\""))
    }
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("pgrep")
            .args(["-f", "/(Codex|ChatGPT)\\.app/Contents/MacOS/"])
            .status()
            .map_err(|error| AppError::msg(format!("检查 Codex 运行状态失败：{error}")))?;
        Ok(status.success())
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        Ok(false)
    }
}

#[cfg(windows)]
fn stop_codex_desktop_blocking() -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    // 先向主窗口发送正常关闭请求，让 Codex 有机会保存语言、窗口和插件状态。
    // 禁止直接 /F：强杀会把 Chromium 的鼠标捕获与配置写入截断，既可能造成
    // 指针暂时消失，也会让下一次启动使用不完整的界面设置。
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Process ChatGPT -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | ForEach-Object { $_.CloseMainWindow() | Out-Null }",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| AppError::msg(format!("关闭 Codex 失败：{error}")))?;
    if !output.status.success() {
        return Err(AppError::msg(format!(
            "关闭 Codex 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    for _ in 0..150 {
        if !codex_desktop_running_blocking()? {
            // 等待桌面端最后一次配置落盘完成，随后再由 Smart Agent 覆盖。
            std::thread::sleep(std::time::Duration::from_millis(500));
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(AppError::msg(
        "Codex 未能正常退出。请手动关闭 Codex 后再次点击接入；Smart Agent 不会强制结束它。",
    ))
}

#[cfg(target_os = "macos")]
fn stop_codex_desktop_blocking() -> AppResult<()> {
    if !codex_desktop_running_blocking()? {
        return Ok(());
    }

    for app_name in ["Codex", "ChatGPT"] {
        let script = format!("tell application \"{app_name}\" to quit");
        let _ = Command::new("osascript").args(["-e", &script]).status();
    }
    for _ in 0..150 {
        if !codex_desktop_running_blocking()? {
            std::thread::sleep(Duration::from_millis(500));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(AppError::msg(
        "Codex 未能正常退出。请手动退出 Codex 后再次点击接入。",
    ))
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn stop_codex_desktop_blocking() -> AppResult<()> {
    Ok(())
}

#[cfg(windows)]
fn launch_codex_desktop_blocking() -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    Command::new("explorer.exe")
        .arg("shell:AppsFolder\\OpenAI.Codex_2p2nqsd0c76g0!App")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| AppError::msg(format!("重新打开 Codex 失败：{error}")))?;
    Ok(())
}

/// 把 Smart Agent provider / 模型目录写回磁盘（**不碰 auth.json**）。
/// 必须读盘再合并：Desktop 可能随时重写 config。
fn reassert_smartagent_config_blocking(
    sk_key: &str,
    model: &str,
    models: &[String],
) -> AppResult<()> {
    let cfg_path = config_path();
    let existing = if cfg_path.exists() {
        fs::read_to_string(&cfg_path).unwrap_or_default()
    } else {
        String::new()
    };
    let config_text = build_config_text(&existing, model, Some(sk_key))
        .or_else(|_| build_config_text("", model, Some(sk_key)))?;
    let catalog_text = build_model_catalog(models, model)?;
    atomic_write(&model_catalog_path(), catalog_text.as_bytes())?;
    atomic_write(&cfg_path, config_text.as_bytes())?;
    Ok(())
}

fn smartagent_config_present_on_disk() -> bool {
    let config_ok = fs::read(config_path())
        .ok()
        .map(|bytes| config_bytes_point_to_smartagent(&bytes))
        .unwrap_or(false);
    let catalog_ok = model_catalog_path().is_file();
    config_ok && catalog_ok
}

fn smartagent_stack_present_on_disk(_sk_key: &str) -> bool {
    // 鉴权走 config 内 experimental_bearer_token + 保留的官方 auth，不校验 API Key 独占 auth
    smartagent_config_present_on_disk()
}

#[cfg(windows)]
fn codex_desktop_installed_blocking() -> bool {
    // MSIX 包名稳定；探测失败时仍允许尝试 launch（最坏只是打不开）。
    use std::os::windows::process::CommandExt;
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "if (Get-AppxPackage -Name OpenAI.Codex -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|status| status.success())
        .unwrap_or(true)
}

/// Codex Desktop 启动后可能做账号/插件同步，把 model_provider、模型目录甚至 auth
/// 洗回官方态，选择器只剩「自定义 + 推理强度」。启动窗口内反复检测并回写。
#[cfg(any(windows, target_os = "macos"))]
fn hold_codex_config_against_desktop_sync_blocking(
    sk_key: &str,
    model: &str,
    models: &[String],
) -> AppResult<()> {
    let mut process_seen = false;
    let mut stable_ticks = 0_u32;
    let mut reassert_count = 0_u32;
    // 约 120s：覆盖冷启动 + 晚到的账号同步/退出落盘。
    for _ in 0..240 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        process_seen |= codex_desktop_running_blocking()?;
        if !process_seen {
            continue;
        }
        if smartagent_stack_present_on_disk(sk_key) {
            stable_ticks += 1;
            // 连续约 20s 未被洗掉，视为稳定。
            if stable_ticks >= 40 {
                return Ok(());
            }
            continue;
        }
        stable_ticks = 0;
        reassert_count += 1;
        reassert_smartagent_config_blocking(sk_key, model, models)?;
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    if !process_seen {
        return Err(AppError::msg("Codex 已配置，但自动重新打开失败"));
    }
    if smartagent_stack_present_on_disk(sk_key) {
        return Ok(());
    }
    Err(AppError::msg(format!(
        "Codex 桌面端启动后持续覆盖 Smart Agent 配置（已回写 {reassert_count} 次仍被清洗）。请完全退出 Codex 后再次点击接入。"
    )))
}

#[cfg(target_os = "macos")]
fn launch_codex_desktop_blocking() -> AppResult<()> {
    if let (Some(cli), _, _) = detect_codex_cli() {
        Command::new(cli)
            .arg("app")
            .spawn()
            .map_err(|error| AppError::msg(format!("重新打开 Codex 失败：{error}")))?;
        return Ok(());
    }

    for app_name in ["Codex", "ChatGPT"] {
        if Command::new("open")
            .args(["-a", app_name])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return Ok(());
        }
    }
    Err(AppError::msg("未找到 Codex，请先完成官方安装"))
}

#[cfg(target_os = "macos")]
fn codex_desktop_installed_blocking() -> bool {
    let mut candidates = vec![
        PathBuf::from("/Applications/Codex.app"),
        PathBuf::from("/Applications/ChatGPT.app"),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Applications/Codex.app"));
        candidates.push(home.join("Applications/ChatGPT.app"));
    }
    candidates.into_iter().any(|candidate| candidate.is_dir())
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn launch_codex_desktop_blocking() -> AppResult<()> {
    Ok(())
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn codex_desktop_installed_blocking() -> bool {
    false
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn hold_codex_config_against_desktop_sync_blocking(
    _sk_key: &str,
    _model: &str,
    _models: &[String],
) -> AppResult<()> {
    Ok(())
}

/// 首页/轮询用：仅当用户仍希望保持接入时，才在配置被洗掉后静默回写。
/// 用户点过「断开」后 `codex_desired=false`，这里直接 no-op，绝不会自动重连。
#[tauri::command]
pub async fn ensure_codex_config(state: tauri::State<'_, AppState>) -> Result<bool, AppError> {
    let (sk_key, active_model, desired) = {
        let store = state.store.lock().await;
        if !store.codex_desired {
            return Ok(false);
        }
        let sk = match &store.sk_key {
            Some(v) if !v.is_empty() => v.clone(),
            _ => return Ok(false),
        };
        let active = store
            .default_model
            .clone()
            .or_else(|| store.selected_models.first().cloned())
            .unwrap_or_else(|| "gpt-5".to_string());
        (sk, active, store.codex_desired)
    };
    if !desired {
        return Ok(false);
    }
    tauri::async_runtime::spawn_blocking(move || {
        // 再读一次磁盘意图，防止断开与 ensure 竞态
        let store = crate::store::LocalStore::load();
        if !store.codex_desired {
            return Ok(false);
        }
        if smartagent_stack_present_on_disk(&sk_key) {
            return Ok(false);
        }
        reassert_smartagent_config_blocking(&sk_key, &active_model, &[active_model.clone()])?;
        Ok(true)
    })
    .await
    .map_err(|e| AppError::msg(format!("恢复 Codex 配置任务失败：{e}")))?
}

#[tauri::command]
pub async fn codex_desktop_running() -> Result<bool, AppError> {
    tauri::async_runtime::spawn_blocking(codex_desktop_running_blocking)
        .await
        .map_err(|e| AppError::msg(format!("检查运行状态失败：{e}")))?
}

// ---------------- 内部备份轮转 / 官方登录恢复 ----------------

fn rotate_backups() -> AppResult<()> {
    let root = backups_root()?;
    let mut dirs: Vec<(i64, PathBuf)> = Vec::new();
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(ts) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse::<i64>().ok())
        {
            dirs.push((ts, path));
        }
    }
    dirs.sort_by(|a, b| b.0.cmp(&a.0)); // 新的在前
    for (_, path) in dirs.into_iter().skip(BACKUP_KEEP) {
        let _ = fs::remove_dir_all(path);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct OfficialLoginRestoreResult {
    pub official_login_restored: bool,
    pub login_required: bool,
}

fn clean_smartagent_from_config(existing_text: &str) -> AppResult<String> {
    let mut table = existing_text
        .parse::<toml::Table>()
        .map_err(|error| AppError::toml(&config_path(), error))?;
    // 仅清理本站 custom / 历史 smartagent，避免误伤用户其它 custom 配置
    let active = table
        .get("model_provider")
        .and_then(toml::Value::as_str)
        .unwrap_or_default();
    let was_managed = (active == PROVIDER_ID && provider_base_url_matches(&table, PROVIDER_ID))
        || active == LEGACY_PROVIDER_ID;

    if table.get("service_tier").and_then(toml::Value::as_str) == Some("default") {
        table.remove("service_tier");
    }
    if was_managed {
        table.remove("model_provider");
        table.remove("model");
    }
    if table
        .get("model_catalog_json")
        .and_then(toml::Value::as_str)
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == model_catalog_path()
            .file_name()
            .and_then(|name| name.to_str())
    {
        table.remove("model_catalog_json");
    }
    let drop_custom = provider_base_url_matches(&table, PROVIDER_ID);
    if let Some(toml::Value::Table(providers)) = table.get_mut("model_providers") {
        if drop_custom {
            providers.remove(PROVIDER_ID);
        }
        providers.remove(LEGACY_PROVIDER_ID);
        if providers.is_empty() {
            table.remove("model_providers");
        }
    }
    toml::to_string_pretty(&table)
        .map_err(|error| AppError::msg(format!("生成官方 Codex 配置失败：{error}")))
}

fn restore_official_codex_login_blocking() -> AppResult<OfficialLoginRestoreResult> {
    let auth_p = auth_path();
    let current_auth = fs::read(&auth_p).ok();
    let current_has_login = current_auth
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .map(|auth| auth_has_chatgpt_login(&auth))
        .unwrap_or(false);
    let official_auth = if current_has_login {
        current_auth.clone()
    } else {
        latest_chatgpt_auth_backup()?
    };

    if let Some(auth) = official_auth.as_deref() {
        if current_auth.as_deref() != Some(auth) {
            atomic_write(&auth_p, auth)?;
        }
    }

    let cfg_path = config_path();
    if cfg_path.exists() {
        let existing =
            fs::read_to_string(&cfg_path).map_err(|error| AppError::io(&cfg_path, error))?;
        let cleaned = clean_smartagent_from_config(&existing)?;
        atomic_write(&cfg_path, cleaned.as_bytes())?;
    }
    let catalog = model_catalog_path();
    if catalog.exists() {
        fs::remove_file(&catalog).map_err(|error| AppError::io(&catalog, error))?;
    }
    let _ = sync_cc_switch_codex_official_state();

    let restored = official_auth.is_some();
    Ok(OfficialLoginRestoreResult {
        official_login_restored: restored,
        login_required: !restored,
    })
}

#[tauri::command]
pub async fn restore_official_codex_login() -> Result<OfficialLoginRestoreResult, AppError> {
    tauri::async_runtime::spawn_blocking(restore_official_codex_login_blocking)
        .await
        .map_err(|error| AppError::msg(format!("恢复官方登录状态失败：{error}")))?
}
