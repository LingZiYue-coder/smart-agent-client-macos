//! 站点常量 —— 品牌锁定铁律：
//! 站点地址是 Rust 编译期常量，前端一律通过 Tauri command 间接访问，
//! 任何 command 永不接收/传递 baseUrl 参数，UI 与本地配置中不出现可改的站点地址。
//!
//! 生产构建通过环境变量在编译期注入：
//!   SMART_SITE_URL=https://api.example.com
//!   SMART_WEBSITE_URL=https://www.example.com
//!   SMART_WEBSITE_DOWNLOAD_URL=https://www.example.com/#download
//!   SMART_DEFAULT_MODEL=gpt-5-codex

/// 站点根地址（编译期固定，不落任何本地可改配置）。
/// 生产默认：https://api.smart-agent.eu.cc ；本地开发请设置 SMART_SITE_URL=http://localhost:3000
pub const SITE_BASE_URL: &str = match option_env!("SMART_SITE_URL") {
    Some(v) => v,
    None => "https://api.smart-agent.eu.cc",
};

/// 官网首页（用户下载/文档站点，不是 API）。
pub const WEBSITE_URL: &str = match option_env!("SMART_WEBSITE_URL") {
    Some(v) => v,
    None => "https://www.smart-agent.eu.cc",
};

/// 官网下载页（更新弹窗默认跳转）。
pub const WEBSITE_DOWNLOAD_URL: &str = match option_env!("SMART_WEBSITE_DOWNLOAD_URL") {
    Some(v) => v,
    None => "https://www.smart-agent.eu.cc/#download",
};

/// 写入 Codex config.toml 的默认模型名（必须是站点实际可用的模型名）。
pub const DEFAULT_MODEL: &str = match option_env!("SMART_DEFAULT_MODEL") {
    Some(v) => v,
    None => "gpt-5-codex",
};

/// 拼接管理面 API 路径，如 api_url("/api/status")。
pub fn api_url(path: &str) -> String {
    format!("{}{}", SITE_BASE_URL.trim_end_matches('/'), path)
}

/// 数据面 /v1 根地址（写入 Codex 的 base_url，Codex 自动拼 /responses）。
pub fn v1_base_url() -> String {
    format!("{}/v1", SITE_BASE_URL.trim_end_matches('/'))
}
