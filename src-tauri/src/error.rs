use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdError {
    pub code: String,
    pub message: String,
}

impl CmdError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }

    pub fn locked() -> Self {
        Self::new("locked", "凭据库已锁定")
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }
}

impl From<String> for CmdError {
    fn from(message: String) -> Self {
        Self::internal(message)
    }
}

impl From<&str> for CmdError {
    fn from(message: &str) -> Self {
        Self::internal(message)
    }
}

impl From<std::io::Error> for CmdError {
    fn from(error: std::io::Error) -> Self {
        Self::internal(error.to_string())
    }
}

impl From<serde_json::Error> for CmdError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(error.to_string())
    }
}

impl From<keydock_core::vault::VaultError> for CmdError {
    fn from(error: keydock_core::vault::VaultError) -> Self {
        use keydock_core::vault::VaultError;
        match error {
            VaultError::Locked => Self::locked(),
            VaultError::WrongPassphrase => Self::new("wrong_passphrase", "主密码错误"),
            VaultError::Missing => Self::new("missing_vault", "凭据库不存在"),
            VaultError::Invalid(message) => Self::new("invalid_vault", message),
            VaultError::Io(error) => Self::internal(error.to_string()),
        }
    }
}

pub type CmdResult<T> = Result<T, CmdError>;
