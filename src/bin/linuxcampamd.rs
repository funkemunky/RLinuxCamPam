use std::env;
use std::fs::{self, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pam_linuxcampam::auth_engine::AuthEngine;
use pam_linuxcampam::config::ProximitySensorMode;
use pam_linuxcampam::constants::{CONFIG_PATH, LINUXCAMPAM_VERSION, SOCKET_PATH};
use pam_linuxcampam::hardware_manager::{HardwareId, HardwareManager};
use pam_linuxcampam::ipc_protocol::{command_to_string, Command, Request};
use pam_linuxcampam::logger::{log_debug, log_error, log_info, log_warn, LogLevel, Logger};
use pam_linuxcampam::presence_tripwire::{PresenceTripwire, PresenceTripwireHidOps};
use pam_linuxcampam::sensor_factory::SensorFactory;
use pam_linuxcampam::utils::{execute_command_spawn, get_home_dir, get_ir_emitter_version, get_uid_for_username};
use pam_linuxcampam::virtual_keyboard::VirtualKeyboard;

static G_RUNNING: AtomicBool = AtomicBool::new(true);
static G_PROXIMITY_PRESENT: AtomicBool = AtomicBool::new(true);

struct LockState {
    was_away: bool,
    absence_timer_active: bool,
    absence_start: Instant,
}

extern "C" fn sig_handler(_: libc::c_int) {
    G_RUNNING.store(false, Ordering::SeqCst);
}

fn handle_client(mut stream: UnixStream, engine: &Arc<Mutex<AuthEngine>>) {
    let mut buffer = [0u8; 1024];
    let n = match stream.read(&mut buffer) {
        Ok(bytes) if bytes > 0 => bytes,
        _ => return,
    };

    let fd = stream.as_raw_fd();
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;

    unsafe {
        if libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        ) < 0
        {
            log_error("Failed to retrieve peer credentials.");
            return;
        }
    }

    let request_str = String::from_utf8_lossy(&buffer[..n]);
    log_debug(format!("Received Request: {request_str}"));

    let req = Request::deserialize(&request_str);
    log_debug(format!("Command: {}", command_to_string(req.cmd)));

    let check_permission = |target_user: &str| -> bool {
        if cred.uid == 0 {
            return true;
        }
        if let Some(target_uid) = get_uid_for_username(target_user) {
            cred.uid == target_uid
        } else {
            false
        }
    };

    let mut response = "ERROR Unknown Command".to_string();

    let mut eng = engine.lock().unwrap();

    match req.cmd {
        Command::AuthRequest => {
            if !req.args.is_empty() {
                let user = &req.args[0];
                if !check_permission(user) {
                    response = "AUTH_FAIL".to_string();
                } else if eng.get_config().proximity_enforce && !G_PROXIMITY_PRESENT.load(Ordering::SeqCst) {
                    response = "AUTH_FAIL: Proximity sensor reports no human present".to_string();
                    log_warn("Authentication rejected by proximity sensor enforcement.");
                } else {
                    let success = eng.verify_user(user);
                    response = if success { "AUTH_SUCCESS" } else { "AUTH_FAIL" }.to_string();
                }
            }
        }
        Command::AddUser => {
            if !req.args.is_empty() {
                let user = &req.args[0];
                if !check_permission(user) {
                    response = "ERROR Permission Denied".to_string();
                } else {
                    let (ok, msg) = eng.enroll_user(user);
                    response = if ok {
                        "ENROLL_SUCCESS".to_string()
                    } else {
                        format!("ENROLL_FAIL {msg}")
                    };
                }
            }
        }
        Command::TrainUser => {
            if !req.args.is_empty() {
                let user = &req.args[0];
                if !check_permission(user) {
                    response = "ERROR Permission Denied".to_string();
                } else {
                    let label = req.args.get(1).map(|s| s.as_str()).unwrap_or("default");
                    let ok = eng.train_user(user, label, false);
                    response = if ok { "TRAIN_SUCCESS" } else { "TRAIN_FAIL" }.to_string();
                }
            }
        }
        Command::GetVersion => {
            let ir_status =
                get_ir_emitter_version(&eng.get_config().ir_emitter_path.to_string_lossy());
            let ir_append = if ir_status.is_empty() {
                String::new()
            } else {
                format!(" (IR Emitter: {ir_status})")
            };
            response = format!("{LINUXCAMPAM_VERSION}{ir_append}");
        }
        Command::TestAuth => {
            if let Some(user) = req.args.first() {
                if !check_permission(user) {
                    response = "ERROR Permission Denied".to_string();
                } else {
                    log_info(format!("Testing Auth for user: {user}"));
                    let result = eng.verify_user_with_details(user);
                    let auth_status = if result.success {
                        "AUTH_SUCCESS".to_string()
                    } else {
                        format!("AUTH_FAIL: {}", result.reason)
                    };
                    response = format!("HW_OK | {auth_status}");
                }
            } else {
                let ok = eng.test_camera_and_auth();
                response = if ok { "HW_OK" } else { "HW_FAIL" }.to_string();
            }
        }
        Command::SetLabel => {
            if req.args.len() < 2 {
                response = "ERROR Missing user or label".to_string();
            } else {
                let user = &req.args[0];
                let label = &req.args[1];
                if !check_permission(user) {
                    response = "ERROR Permission Denied".to_string();
                } else {
                    let ok = eng.set_label(user, label);
                    response = if ok { "LABEL_SET" } else { "LABEL_FAIL" }.to_string();
                }
            }
        }
        Command::TrainNew => {
            if req.args.is_empty() {
                response = "ERROR Missing user".to_string();
            } else {
                let user = &req.args[0];
                if !check_permission(user) {
                    response = "ERROR Permission Denied".to_string();
                } else {
                    let label = req.args.get(1).map(|s| s.as_str()).unwrap_or("default");
                    let ok = eng.train_user(user, label, true);
                    response = if ok { "TRAIN_SUCCESS" } else { "TRAIN_FAIL" }.to_string();
                }
            }
        }
        Command::ListEmbeddings => {
            if req.args.is_empty() {
                response = "ERROR Missing user".to_string();
            } else {
                let user = &req.args[0];
                if !check_permission(user) {
                    response = "ERROR Permission Denied".to_string();
                } else {
                    let labels = eng.list_embeddings(user);
                    if labels.is_empty() {
                        response = "No embeddings found".to_string();
                    } else {
                        response = format!("Labels: {}", labels.join(" "));
                    }
                }
            }
        }
        Command::RemoveEmbedding => {
            if req.args.len() < 2 {
                response = "ERROR Missing user or label".to_string();
            } else {
                let user = &req.args[0];
                let label = &req.args[1];
                if !check_permission(user) {
                    response = "ERROR Permission Denied".to_string();
                } else {
                    let ok = eng.remove_embedding(user, label);
                    response = if ok { "REMOVED" } else { "REMOVE_FAIL" }.to_string();
                }
            }
        }
        Command::GetConfig => {
            response = eng.get_config_string();
        }
        Command::SetLogLevel => {
            if !req.args.is_empty() {
                if cred.uid != 0 {
                    response = "ERROR Permission Denied".to_string();
                } else {
                    let lvl = &req.args[0];
                    if lvl == "DEBUG" {
                        Logger::set_level(LogLevel::Debug);
                        response = "LOG_LEVEL_DEBUG".to_string();
                        log_info("Log level set to DEBUG via socket.");
                    } else if lvl == "INFO" {
                        Logger::set_level(LogLevel::Info);
                        response = "LOG_LEVEL_INFO".to_string();
                        log_info("Log level set to INFO via socket.");
                    } else {
                        response = "ERROR Invalid Log Level".to_string();
                    }
                }
            }
        }
        Command::GetLogLevel => {
            response = match Logger::get_level() {
                LogLevel::Debug => "DEBUG",
                LogLevel::Info => "INFO",
                LogLevel::Warn => "WARN",
                LogLevel::Error => "ERROR",
            }
            .to_string();
        }
        Command::Unknown => {}
    }

    let _ = stream.write_all(response.as_bytes());
}

fn main() {
    unsafe {
        libc::signal(libc::SIGINT, sig_handler as *const () as usize);
        libc::signal(libc::SIGTERM, sig_handler as *const () as usize);
    }

    for arg in env::args().skip(1) {
        if arg == "--debug" || arg == "-d" {
            Logger::set_level(LogLevel::Debug);
            log_debug("Debug logging enabled via command line.");
        }
    }

    let socket_path = Path::new(SOCKET_PATH);
    if let Some(parent) = socket_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            log_error(format!(
                "Failed to create socket directory '{}': {e}",
                parent.display()
            ));
            std::process::exit(1);
        }
    }

    let mut config_path = Path::new(CONFIG_PATH);
    let local_config = Path::new("config.ini");
    if !config_path.exists() && local_config.exists() {
        config_path = local_config;
    }

    log_info("Starting LinuxCamPAM Service...");
    log_info(format!("Loading Config: {}", config_path.display()));

    // Clear OpenCL cache if present
    let home = get_home_dir(unsafe { libc::getuid() }).unwrap_or_else(|| PathBuf::from("/root"));
    let opencv_cache = home.join(".cache").join("opencv");
    if opencv_cache.exists() {
        let _ = fs::remove_dir_all(opencv_cache);
        log_debug("Cleared OpenCL cache");
    }

    let mut engine = AuthEngine::new();
    if !engine.init(config_path) {
        log_error("AuthEngine init failed, shutting down.");
        std::process::exit(1);
    }

    // Apply log settings
    let cfg = engine.get_config().clone();
    match cfg.log_level.as_str() {
        "debug" => Logger::set_level(LogLevel::Debug),
        "warn" | "warning" => Logger::set_level(LogLevel::Warn),
        "error" => Logger::set_level(LogLevel::Error),
        _ => Logger::set_level(LogLevel::Info),
    }

    if !cfg.log_file.is_empty() {
        Logger::set_log_file(&cfg.log_file);
    }

    Logger::enable_syslog("linuxcampamd");

    let vkb = Arc::new(VirtualKeyboard::new());
    let lock_state = Arc::new(Mutex::new(LockState {
        was_away: true,
        absence_timer_active: false,
        absence_start: Instant::now(),
    }));

    // Proximity sensor setup
    let mut hw_manager_opt: Option<HardwareManager> = None;
    let mut tripwire_opt: Option<PresenceTripwire> = None;

    if cfg.proximity_sensor == ProximitySensorMode::Auto
        || cfg.proximity_sensor == ProximitySensorMode::Enabled
    {
        let hw_id = cfg.proximity_sensor_id.clone();
        let mut i2c_addr = String::new();

        let expected_path = format!("/sys/bus/acpi/devices/{hw_id}:00/physical_node");
        if let Ok(target) = fs::read_link(&expected_path) {
            if let Some(name) = target.file_name() {
                i2c_addr = name.to_string_lossy().into_owned();
            }
        }

        if i2c_addr.is_empty() {
            if let Ok(entries) = fs::read_dir("/sys/bus/acpi/devices") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.contains(&hw_id) {
                        let phys = entry.path().join("physical_node");
                        if let Ok(target) = fs::read_link(&phys) {
                            if let Some(n) = target.file_name() {
                                i2c_addr = n.to_string_lossy().into_owned();
                                break;
                            }
                        }
                    }
                }
            }
        }

        if !i2c_addr.is_empty() {
            let mut hw_manager = HardwareManager::with_defaults(&i2c_addr);
            if hw_manager.seize_sensor() {
                if let Some(hidraw_node) = hw_manager.get_hidraw_node() {
                    let sensor_factory = Arc::new(SensorFactory::new());
                    let mut tripwire = PresenceTripwire::new(sensor_factory, PresenceTripwireHidOps::default());

                    let vkb_clone = Arc::clone(&vkb);
                    let lock_clone = Arc::clone(&lock_state);
                    let cfg_clone = cfg.clone();

                    struct ProximityTracking {
                        last_state: bool,
                        last_confidence: i32,
                    }
                    let tracking = Arc::new(Mutex::new(ProximityTracking {
                        last_state: false,
                        last_confidence: 0,
                    }));

                    let started = if let Ok(hw_id_obj) = HardwareId::new(&hw_id) {
                        tripwire.start(&hidraw_node, &hw_id_obj, move |present, confidence| {
                            let (just_returned, log_msg) = {
                                let mut tr = tracking.lock().unwrap();
                                let just_returned = present && !tr.last_state;
                                let mut msg = None;
                                if present != tr.last_state {
                                    if present {
                                        msg = Some(format!("Presence detected start at {confidence}% confidence"));
                                    } else {
                                        msg = Some(format!("Presence detected stop (last seen at {}% confidence)", tr.last_confidence));
                                    }
                                    tr.last_state = present;
                                }
                                if present {
                                    tr.last_confidence = confidence;
                                }
                                (just_returned, msg)
                            };

                            if let Some(m) = log_msg {
                                log_info(m);
                            }

                            G_PROXIMITY_PRESENT.store(present, Ordering::SeqCst);
                            let mut ls = lock_clone.lock().unwrap();

                            if present
                                && cfg_clone.wake_enabled
                                && confidence >= cfg_clone.wake_confidence_threshold
                                && (ls.was_away || (cfg_clone.always_wake_on_presence_detected && just_returned))
                            {
                                log_info("Wake threshold reached, waking screen...");
                                if !vkb_clone.emit_wakeup() {
                                    log_warn("Failed to emit wake event via VirtualKeyboard");
                                }
                                ls.was_away = false;
                            }

                            if cfg_clone.lock_enabled {
                                if confidence < cfg_clone.lock_confidence_threshold || !present {
                                    if !ls.was_away && !ls.absence_timer_active {
                                        ls.absence_start = Instant::now();
                                        ls.absence_timer_active = true;
                                    }
                                } else {
                                    ls.absence_timer_active = false;
                                }
                            } else if confidence < cfg_clone.lock_confidence_threshold || !present {
                                ls.was_away = true;
                            }
                        })
                    } else {
                        false
                    };

                    if started {
                        log_info(format!("Proximity sensor started successfully on {hidraw_node}"));
                        if cfg.proximity_enforce {
                            G_PROXIMITY_PRESENT.store(false, Ordering::SeqCst);
                        }
                        tripwire_opt = Some(tripwire);
                        hw_manager_opt = Some(hw_manager);
                    }
                } else if cfg.proximity_sensor == ProximitySensorMode::Enabled {
                    log_warn("Proximity sensor set to enabled, but hidraw node could not be resolved.");
                }
            } else {
                log_warn("Failed to seize hardware sensor.");
            }
        } else if cfg.proximity_sensor == ProximitySensorMode::Enabled {
            log_warn("Proximity sensor set to enabled, but hardware node was not found.");
        }
    }

    if let Some(parent) = Path::new(socket_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::remove_file(socket_path);
    let listener = match UnixListener::bind(socket_path) {
        Ok(l) => l,
        Err(e) => {
            log_error(format!("bind failed on {SOCKET_PATH}: {e}"));
            std::process::exit(1);
        }
    };

    let _ = fs::set_permissions(socket_path, Permissions::from_mode(0o666));
    let _ = listener.set_nonblocking(true);

    log_info(format!("Listening on {SOCKET_PATH}"));

    let engine_arc = Arc::new(Mutex::new(engine));

    while G_RUNNING.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                handle_client(stream, &engine_arc);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
            }
        }

        // Periodic maintenance and lock checks
        if let Ok(mut eng) = engine_arc.try_lock() {
            let _ = eng.perform_maintenance();
            let config = eng.get_config().clone();
            drop(eng);

            if config.lock_enabled {
                let mut ls = lock_state.lock().unwrap();
                if ls.absence_timer_active && !ls.was_away {
                    let elapsed = ls.absence_start.elapsed().as_secs() as i32;
                    if elapsed >= config.lock_timeout_seconds {
                        log_info(format!(
                            "Lock timeout reached ({elapsed}s), locking screen..."
                        ));
                        let ret = execute_command_spawn(&config.lock_command);
                        if ret != 0 {
                            log_warn(format!(
                                "Lock command returned non-zero exit code: {ret}"
                            ));
                        }
                        ls.was_away = true;
                        ls.absence_timer_active = false;
                    }
                }
            }
        }
    }

    if let Some(mut tw) = tripwire_opt {
        tw.stop();
    }
    drop(hw_manager_opt);

    let _ = fs::remove_file(socket_path);
    log_info("Stopped.");
}
