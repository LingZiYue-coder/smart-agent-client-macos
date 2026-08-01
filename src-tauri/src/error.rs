//! 统一错误类型：所有 Tauri command 返回 Result<T, AppError>，
//! AppError 序列化为字符串消息传给前端（中文可读）。

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("IO 错误（{path}）：{source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("网络请求失败：{0}")]
    Http(String),

    #[error("TOML 配置解析失败（{path}）：{message}")]
    Toml { path: String, message: String },

    #[error("JSON 解析失败：{0}")]
    Json(String),

    #[error("{0}")]
    Message(String),
}

impl AppError {
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        AppError::Io {
            path: path.display().to_string(),
            source,
        }
    }

    pub fn toml(path: &Path, err: impl std::fmt::Display) -> Self {
        AppError::Toml {
            path: path.display().to_string(),
            message: err.to_string(),
        }
    }

    pub fn msg(text: impl Into<String>) -> Self {
        AppError::Message(text.into())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            AppError::Http("请求超时，请检查网络后重试".to_string())
        } else if err.is_connect() {
            AppError::Http("无法连接到服务器，请检查网络后重试".to_string())
        } else {
            AppError::Http(err.to_string())
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Json(err.to_string())
    }
}

// Tauri command 的错误必须可序列化；序列化为用户可读的消息字符串。
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
