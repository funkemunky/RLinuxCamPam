use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::constants::DEFAULT_MIN_UID;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PamConfig {
    pub min_uid: libc::uid_t,
    pub require_confirmation: bool,
    pub confirmation_exempt_services: Vec<String>,
    pub show_welcome: bool,
    pub welcome_message: String,
}

impl Default for PamConfig {
    fn default() -> Self {
        Self {
            min_uid: DEFAULT_MIN_UID,
            require_confirmation: true,
            confirmation_exempt_services: vec![
                "gdm-password".to_string(),
                "sddm".to_string(),
                "lightdm".to_string(),
                "login".to_string(),
                "swaylock".to_string(),
                "i3lock".to_string(),
                "xscreensaver".to_string(),
                "kscreenlocker".to_string(),
                "kde".to_string(),
                "systemd-user".to_string(),
            ],
            show_welcome: true,
            welcome_message: "LinuxCamPAM: Welcome, %u!".to_string(),
        }
    }
}

pub type IniData = BTreeMap<String, BTreeMap<String, String>>;

#[derive(Debug, Default)]
pub struct PamConfigState {
    pub current_section: String,
    pub data: IniData,
}

pub fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\r' || c == '\n')
}

pub fn split(s: &str, delimiter: char) -> Vec<String> {
    s.split(delimiter)
        .map(trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_string())
        .collect()
}

pub fn process_pam_config_line(line: &str, state: &mut PamConfigState) {
    let sv = trim(line);
    if sv.is_empty() || sv.starts_with(';') || sv.starts_with('#') {
        return;
    }

    if sv.starts_with('[') && sv.ends_with(']') {
        state.current_section = sv[1..sv.len() - 1].to_string();
        return;
    }

    if let Some(eq_pos) = sv.find('=') {
        let key = trim(&sv[..eq_pos]);
        let val = trim(&sv[eq_pos + 1..]);
        if !key.is_empty() {
            state
                .data
                .entry(state.current_section.clone())
                .or_default()
                .insert(key.to_string(), val.to_string());
        }
    }
}

pub fn resolve_pam_config(state: &PamConfigState) -> PamConfig {
    let mut config = PamConfig::default();

    let get_value = |key: &str| -> Option<String> {
        if let Some(sec) = state.data.get("Security") {
            if let Some(val) = sec.get(key) {
                return Some(val.clone());
            }
        }
        for (sec_name, sec) in &state.data {
            if sec_name != "Security" {
                if let Some(val) = sec.get(key) {
                    return Some(val.clone());
                }
            }
        }
        None
    };

    // min_uid
    if let Some(uid_str) = get_value("min_uid") {
        if !uid_str.is_empty() && !uid_str.starts_with('-') {
            if let Ok(parsed) = uid_str.parse::<u32>() {
                config.min_uid = parsed as libc::uid_t;
            }
        }
    }

    // require_confirmation
    if let Some(rc_str) = get_value("require_confirmation") {
        config.require_confirmation = rc_str == "true" || rc_str == "1" || rc_str == "yes";
    }

    // confirmation_exempt_services
    if let Some(mut ces_str) = get_value("confirmation_exempt_services") {
        if ces_str.len() >= 2 && ces_str.starts_with('"') && ces_str.ends_with('"') {
            ces_str = ces_str[1..ces_str.len() - 1].to_string();
        }
        config.confirmation_exempt_services = split(&ces_str, ',');
    }

    // show_welcome
    if let Some(sw_str) = get_value("show_welcome") {
        config.show_welcome = sw_str == "true" || sw_str == "1" || sw_str == "yes";
    }

    // welcome_message
    if let Some(mut wm_str) = get_value("welcome_message") {
        if wm_str.len() >= 2 && wm_str.starts_with('"') && wm_str.ends_with('"') {
            wm_str = wm_str[1..wm_str.len() - 1].to_string();
        }
        config.welcome_message = wm_str;
    }

    config
}

pub fn load_pam_config(path: &str) -> PamConfig {
    let mut state = PamConfigState::default();
    if let Ok(file) = File::open(path) {
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            process_pam_config_line(&line, &mut state);
        }
    }
    resolve_pam_config(&state)
}
