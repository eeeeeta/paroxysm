use std::collections::HashSet;

#[derive(Deserialize)]
pub struct Config {
    pub database_url: String,
    pub irc_config_path: String,
    #[serde(default)]
    pub admins: HashSet<String>,
    #[serde(default)]
    pub log_filter: Option<String>
}

