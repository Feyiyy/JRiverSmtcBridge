use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AppConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub debug: bool,
    #[serde(default = "default_show_notifications")]
    pub show_notifications: bool,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    52199
}

fn default_poll_interval() -> u64 {
    500
}

fn default_show_notifications() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            username: String::new(),
            password: String::new(),
            poll_interval_ms: default_poll_interval(),
            debug: false,
            show_notifications: default_show_notifications(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct AppSettingsWrapper {
    #[serde(rename = "JRiver")]
    jriver: AppConfig,
}

pub fn get_app_dir() -> PathBuf {
    if let Ok(exe_path) = env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            return parent.to_path_buf();
        }
    }
    PathBuf::from(".")
}

pub fn check_first_run(base_dir: &Path) -> bool {
    let default_path = base_dir.join("appsettings.json");
    let local_path = base_dir.join("appsettings.local.json");
    !default_path.exists() && !local_path.exists()
}

pub fn load_configuration(base_dir: &Path) -> (AppConfig, bool) {
    let is_first_run = check_first_run(base_dir);
    let mut config = AppConfig::default();

    let default_path = base_dir.join("appsettings.json");
    let local_path = base_dir.join("appsettings.local.json");

    if default_path.exists() {
        load_from_file(&default_path, &mut config);
    }
    if local_path.exists() {
        load_from_file(&local_path, &mut config);
    }

    (config, is_first_run)
}

fn load_from_file(path: &Path, target: &mut AppConfig) {
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(wrapper) = serde_json::from_str::<AppSettingsWrapper>(&content) {
            *target = wrapper.jriver;
        } else if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(jriver) = val.get("JRiver") {
                if let Some(host) = jriver.get("Host").and_then(|v| v.as_str()) {
                    target.host = host.to_string();
                }
                if let Some(port) = jriver.get("Port").and_then(|v| v.as_u64()) {
                    target.port = port as u16;
                }
                if let Some(user) = jriver.get("Username").and_then(|v| v.as_str()) {
                    target.username = user.to_string();
                }
                if let Some(pass) = jriver.get("Password").and_then(|v| v.as_str()) {
                    target.password = pass.to_string();
                }
                if let Some(poll) = jriver.get("PollIntervalMs").and_then(|v| v.as_u64()) {
                    target.poll_interval_ms = poll;
                }
                if let Some(debug) = jriver.get("Debug").and_then(|v| v.as_bool()) {
                    target.debug = debug;
                }
                if let Some(notif) = jriver.get("ShowNotifications").and_then(|v| v.as_bool()) {
                    target.show_notifications = notif;
                }
            }
        }
    }
}

pub fn save_configuration(base_dir: &Path, config: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let default_path = base_dir.join("appsettings.json");
    let wrapper = AppSettingsWrapper {
        jriver: config.clone(),
    };
    let json = serde_json::to_string_pretty(&wrapper)?;
    fs::write(default_path, json)?;
    Ok(())
}
