use anyhow::Result;

mod env;
mod load;
mod parse;
mod types;

pub use types::*;

pub fn load_config() -> Result<AppConfig> {
    load::load_config()
}
