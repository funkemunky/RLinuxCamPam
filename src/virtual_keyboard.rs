use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::sync::Mutex;

use crate::logger::{log_error, log_info};

const VIRTUAL_KEYBOARD_VENDOR_ID: u16 = 0x1234;
const VIRTUAL_KEYBOARD_PRODUCT_ID: u16 = 0x5678;

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const KEY_WAKEUP: u16 = 143;
const SYN_REPORT: u16 = 0;
const BUS_USB: u16 = 0x03;
const UINPUT_MAX_NAME_SIZE: usize = 80;

const UI_SET_EVBIT: libc::c_ulong = 0x40045564;
const UI_SET_KEYBIT: libc::c_ulong = 0x40045565;
const UI_DEV_SETUP: libc::c_ulong = 0x405c5503;
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;

#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [libc::c_char; UINPUT_MAX_NAME_SIZE],
    ff_effects_max: u32,
}

#[repr(C)]
struct InputEvent {
    time: libc::timeval,
    type_: u16,
    code: u16,
    value: i32,
}

pub struct VirtualKeyboard {
    file: Mutex<Option<File>>,
}

impl VirtualKeyboard {
    pub fn new() -> Self {
        Self {
            file: Mutex::new(None),
        }
    }

    fn init_locked(guard: &mut Option<File>) -> bool {
        if guard.is_some() {
            return true;
        }

        let file = match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open("/dev/uinput")
        {
            Ok(f) => f,
            Err(_) => {
                log_error("Failed to open /dev/uinput for VirtualKeyboard");
                return false;
            }
        };

        let fd = file.as_raw_fd();

        unsafe {
            if libc::ioctl(fd, UI_SET_EVBIT, EV_KEY as libc::c_int) < 0 {
                log_error("Failed to set EV_KEY on /dev/uinput");
                return false;
            }
            if libc::ioctl(fd, UI_SET_KEYBIT, KEY_WAKEUP as libc::c_int) < 0 {
                log_error("Failed to set KEY_WAKEUP on /dev/uinput");
                return false;
            }

            let mut usetup = UinputSetup {
                id: InputId {
                    bustype: BUS_USB,
                    vendor: VIRTUAL_KEYBOARD_VENDOR_ID,
                    product: VIRTUAL_KEYBOARD_PRODUCT_ID,
                    version: 0,
                },
                name: [0; UINPUT_MAX_NAME_SIZE],
                ff_effects_max: 0,
            };

            let name_str = b"LinuxCamPAM Virtual Keyboard\0";
            for (i, &b) in name_str.iter().take(UINPUT_MAX_NAME_SIZE - 1).enumerate() {
                usetup.name[i] = b as libc::c_char;
            }

            if libc::ioctl(fd, UI_DEV_SETUP, &usetup) < 0 {
                log_error("Failed to setup uinput device");
                return false;
            }

            if libc::ioctl(fd, UI_DEV_CREATE) < 0 {
                log_error("Failed to create uinput device");
                return false;
            }
        }

        log_info("VirtualKeyboard successfully initialized");
        *guard = Some(file);
        true
    }

    pub fn init(&self) -> bool {
        let mut guard = self.file.lock().unwrap();
        Self::init_locked(&mut guard)
    }

    pub fn emit_wakeup(&self) -> bool {
        let mut guard = self.file.lock().unwrap();
        if !Self::init_locked(&mut guard) {
            return false;
        }

        let file = match guard.as_ref() {
            Some(f) => f,
            None => return false,
        };
        let fd = file.as_raw_fd();

        let write_ev = |type_: u16, code: u16, value: i32, err_msg: &str| -> bool {
            let ev = InputEvent {
                time: libc::timeval {
                    tv_sec: 0,
                    tv_usec: 0,
                },
                type_,
                code,
                value,
            };
            let ptr = &ev as *const InputEvent as *const libc::c_void;
            let size = std::mem::size_of::<InputEvent>();
            let written = unsafe { libc::write(fd, ptr, size) };
            if written < 0 {
                log_error(err_msg);
                false
            } else {
                true
            }
        };

        if !write_ev(EV_KEY, KEY_WAKEUP, 1, "Failed to write KEY_WAKEUP press event") {
            return false;
        }
        if !write_ev(EV_KEY, KEY_WAKEUP, 0, "Failed to write KEY_WAKEUP release event") {
            return false;
        }
        if !write_ev(EV_SYN, SYN_REPORT, 0, "Failed to write SYN_REPORT event") {
            return false;
        }

        true
    }
}

impl Default for VirtualKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for VirtualKeyboard {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.take() {
                unsafe {
                    libc::ioctl(file.as_raw_fd(), UI_DEV_DESTROY);
                }
            }
        }
    }
}
