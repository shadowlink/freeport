use serde::Serialize;

/// Application-wide error type. Serializes to a plain string so it can cross the
/// Tauri command boundary and be shown to the frontend.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("error de red: {0}")]
    Http(#[from] reqwest::Error),
    #[error("error de entrada/salida: {0}")]
    Io(#[from] std::io::Error),
    #[error("error de datos: {0}")]
    Json(#[from] serde_json::Error),
    #[error("error al descomprimir: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{0}")]
    Msg(String),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Msg(s.into())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
