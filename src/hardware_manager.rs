use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

pub const MAX_HARDWARE_ID_LENGTH: usize = 32;
pub const MAX_HIDRAW_RETRIES: usize = 5;
pub const UDEV_LATENCY_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareId(String);

impl HardwareId {
    pub fn new(id: &str) -> Result<Self, &'static str> {
        if id.is_empty() || id.len() > MAX_HARDWARE_ID_LENGTH {
            return Err("Invalid Hardware ID");
        }
        if !id.chars().all(|c| c.is_alphanumeric()) {
            return Err("Invalid Hardware ID");
        }
        Ok(Self(id.to_string()))
    }

    pub fn get(&self) -> &str {
        &self.0
    }

    pub fn str(&self) -> &str {
        &self.0
    }
}

pub struct HardwareManager {
    i2c_address: String,
    hid_id: String,
    #[allow(dead_code)]
    previous_hid_driver: String,
    sys_i2c_path: PathBuf,
    sys_hid_path: PathBuf,
    dev_path: PathBuf,
}

impl HardwareManager {
    pub fn new(
        addr: impl Into<String>,
        sys_i2c_path: impl Into<PathBuf>,
        sys_hid_path: impl Into<PathBuf>,
        dev_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            i2c_address: addr.into(),
            hid_id: String::new(),
            previous_hid_driver: String::new(),
            sys_i2c_path: sys_i2c_path.into(),
            sys_hid_path: sys_hid_path.into(),
            dev_path: dev_path.into(),
        }
    }

    pub fn with_defaults(addr: impl Into<String>) -> Self {
        Self::new(
            addr,
            Path::new("/sys/bus/i2c/devices"),
            Path::new("/sys/bus/hid/devices"),
            Path::new("/dev"),
        )
    }

    fn write_sysfs(path: &Path, value: &str) -> bool {
        fs::write(path, value).is_ok()
    }

    fn resolve_hid_device(&mut self) -> bool {
        let path = self.sys_i2c_path.join(&self.i2c_address);
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => return false,
        };

        const PREFIX: &str = "0018:";
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(PREFIX) {
                self.hid_id = name_str.into_owned();
                return true;
            }
        }
        false
    }

    fn resolve_current_driver(&mut self) {
        let driver_link = self.sys_hid_path.join(&self.hid_id).join("driver");
        if let Ok(target) = fs::read_link(&driver_link) {
            if let Some(name) = target.file_name() {
                self.previous_hid_driver = name.to_string_lossy().into_owned();
                return;
            }
        }
        self.previous_hid_driver.clear();
    }

    pub fn seize_sensor(&mut self) -> bool {
        let wakeup_path = self
            .sys_i2c_path
            .join(&self.i2c_address)
            .join("power")
            .join("wakeup");

        if !Self::write_sysfs(&wakeup_path, "disabled") {
            return false;
        }

        if !self.resolve_hid_device() {
            return false;
        }

        self.resolve_current_driver();
        true
    }

    pub fn release_sensor(&mut self) {
        if self.hid_id.is_empty() {
            return;
        }
        let wakeup_path = self
            .sys_i2c_path
            .join(&self.i2c_address)
            .join("power")
            .join("wakeup");
        let _ = Self::write_sysfs(&wakeup_path, "enabled");
    }

    pub fn get_hidraw_node(&self) -> Option<String> {
        if self.hid_id.is_empty() {
            return None;
        }
        let parent_path = self.sys_hid_path.join(&self.hid_id);
        let search_path = parent_path.join("hidraw");

        if !parent_path.exists() {
            return None;
        }

        for _ in 0..MAX_HIDRAW_RETRIES {
            if search_path.exists() {
                if let Ok(entries) = fs::read_dir(&search_path) {
                    if let Some(entry) = entries.flatten().next() {
                        let filename = entry.file_name();
                        return Some(self.dev_path.join(filename).to_string_lossy().into_owned());
                    }
                }
            }
            thread::sleep(UDEV_LATENCY_DELAY);
        }

        None
    }
}

impl Drop for HardwareManager {
    fn drop(&mut self) {
        self.release_sensor();
    }
}
