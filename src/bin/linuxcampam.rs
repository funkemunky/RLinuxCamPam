use std::env;
use std::io::{self, BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::SystemTime;

use pam_linuxcampam::constants::{LINUXCAMPAM_VERSION, SOCKET_PATH};

fn get_current_user() -> String {
    if let Ok(sudo_user) = env::var("SUDO_USER") {
        if !sudo_user.is_empty() {
            return sudo_user;
        }
    }
    if let Ok(user) = env::var("USER") {
        if !user.is_empty() {
            return user;
        }
    }
    String::new()
}

fn send_cmd(cmd: &str) -> String {
    let mut stream = match UnixStream::connect(SOCKET_PATH) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("Could not connect to service at {SOCKET_PATH}. Is linuxcampamd running?");
            return String::new();
        }
    };

    if stream.write_all(cmd.as_bytes()).is_err() {
        eprintln!("Error sending command to socket.");
        return String::new();
    }

    let mut buffer = [0u8; 4096];
    match stream.read(&mut buffer) {
        Ok(bytes) if bytes > 0 => String::from_utf8_lossy(&buffer[..bytes]).into_owned(),
        _ => String::new(),
    }
}

fn print_response(resp: &str) {
    if !resp.is_empty() {
        println!("Response: {resp}");
    } else {
        eprintln!("Error: Connection closed by service (empty response).");
    }
}

fn print_help() {
    println!("LinuxCamPAM CLI Tool v{LINUXCAMPAM_VERSION}");
    println!("Usage:");
    println!("  linuxcampam add <username>              Enroll a new user");
    println!("  linuxcampam train [username] [options]  Train/refine model");
    println!("    --label <name>                        Refine specific label");
    println!("    --new                                 Add new embedding");
    println!("  linuxcampam test [username]             Test camera & auth");
    println!("  linuxcampam list <username>             Show embedding labels");
    println!("  linuxcampam remove <user> --label <X>   Remove specific embedding");
    println!("  linuxcampam show-config                 Show active config");
    println!("  linuxcampam debug [on|off]              Toggle debug logging");
    println!("  linuxcampam version                     Show version info");
    println!("  linuxcampam help                        Show this help");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: linuxcampam <add|train|test|list|remove|show-config|debug|version|help> [args]");
        std::process::exit(1);
    }

    let op = &args[1];

    if op == "add" {
        unsafe {
            if libc::getuid() != 0 {
                eprintln!("Error: Adding a user requires root privileges (sudo).");
                std::process::exit(1);
            }
        }
        if args.len() < 3 {
            println!("Usage: linuxcampam add <username>");
            std::process::exit(1);
        }
        let user = &args[2];

        let resp = send_cmd(&format!("ADD_USER {user}"));
        print_response(&resp);

        if resp.contains("ENROLL_SUCCESS") {
            let existing = send_cmd(&format!("LIST_EMBEDDINGS {user}"));

            print!("Label (default): ");
            let _ = io::stdout().flush();
            let mut label = String::new();
            let _ = io::stdin().lock().read_line(&mut label);
            let mut label = label.trim().to_string();
            if label.is_empty() {
                label = "default".to_string();
            }

            if existing.contains(&label) {
                print!("Label '{label}' already exists. Overwrite? [y/N]: ");
                let _ = io::stdout().flush();
                let mut confirm = String::new();
                let _ = io::stdin().lock().read_line(&mut confirm);
                let confirm = confirm.trim();
                if confirm != "y" && confirm != "Y" {
                    println!("Cancelled. Embedding discarded.");
                    return;
                }
            }

            let label_resp = send_cmd(&format!("SET_LABEL {user} {label}"));
            if !label_resp.is_empty() && !label_resp.contains("ERROR") {
                println!("Embedding saved with label: {label}");
            }
        }
    } else if op == "train" {
        let mut user = String::new();
        let mut label = String::new();
        let mut is_new = false;

        let mut i = 2;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--label" && i + 1 < args.len() {
                i += 1;
                label = args[i].clone();
            } else if arg == "--new" {
                is_new = true;
            } else if !arg.starts_with('-') {
                user = arg.clone();
            }
            i += 1;
        }

        if user.is_empty() {
            user = get_current_user();
            if user.is_empty() {
                eprintln!("Could not determine username. Please specify explicitly.");
                std::process::exit(1);
            }
        }

        let current_user = get_current_user();
        unsafe {
            if user != current_user && libc::getuid() != 0 {
                eprintln!("Error: Training for other users requires root privileges (sudo).");
                std::process::exit(1);
            }
        }

        if is_new {
            print!("New label: ");
            let _ = io::stdout().flush();
            let mut l = String::new();
            let _ = io::stdin().lock().read_line(&mut l);
            let l = l.trim();
            let final_label = if l.is_empty() {
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                format!("trained_{now}")
            } else {
                l.to_string()
            };
            print_response(&send_cmd(&format!("TRAIN_NEW {user} {final_label}")));
        } else {
            let final_label = if label.is_empty() {
                "default".to_string()
            } else {
                label
            };
            print_response(&send_cmd(&format!("TRAIN_USER {user} {final_label}")));
        }
    } else if op == "test" {
        let current_user = get_current_user();
        let user = if args.len() >= 3 {
            let u = &args[2];
            unsafe {
                if u != &current_user && libc::getuid() != 0 {
                    eprintln!("Error: Testing other users requires sudo.");
                    std::process::exit(1);
                }
            }
            u.clone()
        } else {
            current_user
        };

        if !user.is_empty() {
            print_response(&send_cmd(&format!("TEST_AUTH {user}")));
        } else {
            print_response(&send_cmd("TEST_AUTH"));
        }
    } else if op == "list" {
        if args.len() < 3 {
            println!("Usage: linuxcampam list <username>");
            std::process::exit(1);
        }
        let user = &args[2];
        let current_user = get_current_user();
        unsafe {
            if user != &current_user && libc::getuid() != 0 {
                eprintln!("Error: Listing other users requires root privileges (sudo).");
                std::process::exit(1);
            }
        }
        print_response(&send_cmd(&format!("LIST_EMBEDDINGS {user}")));
    } else if op == "remove" {
        if args.len() < 4 {
            println!("Usage: linuxcampam remove <username> --label <label>");
            std::process::exit(1);
        }
        let user = &args[2];
        let current_user = get_current_user();
        unsafe {
            if user != &current_user && libc::getuid() != 0 {
                eprintln!("Error: Removing embeddings for other users requires root privileges (sudo).");
                std::process::exit(1);
            }
        }

        let mut label = String::new();
        let mut i = 3;
        while i < args.len() {
            if args[i] == "--label" && i + 1 < args.len() {
                label = args[i + 1].clone();
                break;
            }
            i += 1;
        }

        if label.is_empty() {
            println!("Error: --label is required");
            std::process::exit(1);
        }

        print_response(&send_cmd(&format!("REMOVE_EMBEDDING {user} {label}")));
    } else if op == "show-config" {
        let cfg = send_cmd("GET_CONFIG");
        if !cfg.is_empty() {
            println!("{cfg}");
        } else {
            eprintln!("Error: Could not get config from service.");
        }
    } else if op == "debug" {
        if args.len() < 3 {
            let lvl = send_cmd("GET_LOG_LEVEL");
            if !lvl.is_empty() {
                println!("Current Log Level: {lvl}");
            } else {
                eprintln!("Error: Could not query log level.");
            }
        } else {
            let arg = &args[2];
            if arg == "on" {
                print_response(&send_cmd("SET_LOG_LEVEL DEBUG"));
            } else if arg == "off" {
                print_response(&send_cmd("SET_LOG_LEVEL INFO"));
            } else {
                println!("Usage: linuxcampam debug [on|off]");
            }
        }
    } else if op == "version" || op == "--version" || op == "-v" {
        println!("Client Version: {LINUXCAMPAM_VERSION}");
        let daemon_ver = send_cmd("GET_VERSION");
        if daemon_ver.is_empty() {
            println!("Daemon Version: Not running or unreachable");
        } else {
            println!("Daemon Version: {daemon_ver}");
        }
    } else if op == "help" || op == "--help" || op == "-h" {
        print_help();
    } else {
        println!("Unknown command. Try 'linuxcampam help'.");
        std::process::exit(1);
    }
}
