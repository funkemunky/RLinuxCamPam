use pam_linuxcampam::constants::DEFAULT_MIN_UID;
use pam_linuxcampam::pam::pam_config::{
    process_pam_config_line, resolve_pam_config, PamConfig, PamConfigState,
};

fn load_config(content: &str) -> PamConfig {
    let mut state = PamConfigState::default();
    for line in content.lines() {
        process_pam_config_line(line, &mut state);
    }
    resolve_pam_config(&state)
}

#[test]
fn default_value_when_empty() {
    let config = load_config("");
    assert_eq!(config.min_uid, DEFAULT_MIN_UID);
}

#[test]
fn default_value_when_key_missing() {
    let config = load_config("[General]\nfoo=bar");
    assert_eq!(config.min_uid, DEFAULT_MIN_UID);
}

#[test]
fn security_priority_precedence() {
    let config = load_config(
        r#"
[General]
min_uid = 500
[Security]
min_uid = 2000
"#,
    );
    assert_eq!(config.min_uid, 2000);
}

#[test]
fn security_priority_precedence_reverse_order() {
    let config = load_config(
        r#"
[Security]
min_uid = 2000
[General]
min_uid = 500
"#,
    );
    assert_eq!(config.min_uid, 2000);
}

#[test]
fn first_found_fallback() {
    let config = load_config(
        r#"
[General]
min_uid = 500
[Other]
min_uid = 300
"#,
    );
    assert_eq!(config.min_uid, 500);
}

#[test]
fn whitespace_handling() {
    let config = load_config("min_uid =  1234  ");
    assert_eq!(config.min_uid, 1234);
}

#[test]
fn invalid_numbers_ignored() {
    let config = load_config("min_uid = abc");
    assert_eq!(config.min_uid, DEFAULT_MIN_UID);
}

#[test]
fn invalid_numbers_fallback_to_first_valid() {
    let config = load_config(
        r#"
min_uid = abc
min_uid = 500
"#,
    );
    assert_eq!(config.min_uid, 500);
}

#[test]
fn comments_ignored() {
    let config = load_config(
        r#"
; min_uid = 500
# min_uid = 600
min_uid = 1000
"#,
    );
    assert_eq!(config.min_uid, 1000);
}

#[test]
fn negative_numbers_ignored() {
    let config = load_config("min_uid = -5");
    assert_eq!(config.min_uid, DEFAULT_MIN_UID);
}

#[test]
fn explicit_zero_allowed() {
    let config = load_config("min_uid = 0");
    assert_eq!(config.min_uid, 0);
}

#[test]
fn welcome_defaults() {
    let config = load_config("");
    assert!(config.show_welcome);
    assert_eq!(config.welcome_message, "LinuxCamPAM: Welcome, %u!");
}

#[test]
fn welcome_message_parsing() {
    let config = load_config(
        r#"
[Security]
show_welcome = false
welcome_message = "Hello, %u!"
"#,
    );
    assert!(!config.show_welcome);
    assert_eq!(config.welcome_message, "Hello, %u!");
}

#[test]
fn welcome_message_unquoted() {
    let config = load_config(
        r#"
[Security]
welcome_message = Hello World
"#,
    );
    assert_eq!(config.welcome_message, "Hello World");
}

#[test]
fn welcome_message_precedence() {
    let config = load_config(
        r#"
show_welcome = true
welcome_message = "Default"

[Security]
show_welcome = false
welcome_message = "Security"
"#,
    );
    assert!(!config.show_welcome);
    assert_eq!(config.welcome_message, "Security");
}

#[test]
fn require_confirmation_defaults() {
    let config = load_config("");
    assert!(config.require_confirmation);
    assert_eq!(config.confirmation_exempt_services.len(), 10);
    assert_eq!(config.confirmation_exempt_services[0], "gdm-password");
}

#[test]
fn require_confirmation_parsing() {
    let config = load_config(
        r#"
[Security]
require_confirmation = false
confirmation_exempt_services = sshd,su
"#,
    );
    assert!(!config.require_confirmation);
    assert_eq!(config.confirmation_exempt_services.len(), 2);
    assert_eq!(config.confirmation_exempt_services[0], "sshd");
    assert_eq!(config.confirmation_exempt_services[1], "su");
}

#[test]
fn require_confirmation_precedence() {
    let config = load_config(
        r#"
require_confirmation = true
confirmation_exempt_services = "none"

[Security]
require_confirmation = false
confirmation_exempt_services = sudo,login
"#,
    );
    assert!(!config.require_confirmation);
    assert_eq!(config.confirmation_exempt_services.len(), 2);
    assert_eq!(config.confirmation_exempt_services[0], "sudo");
    assert_eq!(config.confirmation_exempt_services[1], "login");
}
