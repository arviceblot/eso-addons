use eso_addons_api;

#[derive(Debug)]
pub enum Error {
    AddonNotFound(String),
    NoAddonsInstalled,
    AppError(eso_addons_core::errors::Error),
    Other(Box<dyn std::error::Error>),
    ApiError(eso_addons_api::errors::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            Self::AddonNotFound(name) => f.write_str(&format!("addon {} not found", &name)),
            Self::NoAddonsInstalled => f.write_str("no addons installed"),
            Self::AppError(err) => f.write_str(&format!("app error: {}", err)),
            Self::Other(err) => f.write_str(&format!("other error: {}", err)),
            Self::ApiError(err) => f.write_str(&format!("api error: {}", err)),
        }
    }
}

impl std::error::Error for Error {}

impl From<eso_addons_core::errors::Error> for Error {
    fn from(err: eso_addons_core::errors::Error) -> Self {
        Self::AppError(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
