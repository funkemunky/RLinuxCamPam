use std::fs;
use std::path::PathBuf;
use pam_linuxcampam::hardware_manager::HardwareManager;
use tempfile::TempDir;

struct TestEnv {
    _temp: TempDir,
    sys_i2c: PathBuf,
    sys_hid: PathBuf,
    dev_dir: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let sys_i2c = temp.path().join("sys/bus/i2c/devices");
        let sys_hid = temp.path().join("sys/bus/hid/devices");
        let dev_dir = temp.path().join("dev");

        fs::create_dir_all(&sys_i2c).unwrap();
        fs::create_dir_all(&sys_hid).unwrap();
        fs::create_dir_all(&dev_dir).unwrap();

        Self {
            _temp: temp,
            sys_i2c,
            sys_hid,
            dev_dir,
        }
    }
}

#[test]
fn constructor_and_destructor() {
    let env = TestEnv::new();
    let _hm = HardwareManager::new("test_addr", &env.sys_i2c, &env.sys_hid, &env.dev_dir);
}

#[test]
fn seize_sensor_fails_if_path_missing() {
    let env = TestEnv::new();
    let mut hm = HardwareManager::new("missing_addr", &env.sys_i2c, &env.sys_hid, &env.dev_dir);
    assert!(!hm.seize_sensor());
}

#[test]
fn seize_sensor_succeeds_with_mock_dirs() {
    let env = TestEnv::new();
    let device_dir = env.sys_i2c.join("test_addr");
    fs::create_dir_all(device_dir.join("power")).unwrap();
    fs::write(device_dir.join("power/wakeup"), "enabled").unwrap();
    fs::create_dir_all(device_dir.join("0018:ABCD:1234.0001")).unwrap();

    let mut hm = HardwareManager::new("test_addr", &env.sys_i2c, &env.sys_hid, &env.dev_dir);
    assert!(hm.seize_sensor());
}

#[test]
fn get_hidraw_node_returns_none_if_missing() {
    let env = TestEnv::new();
    let hm = HardwareManager::new("test_addr", &env.sys_i2c, &env.sys_hid, &env.dev_dir);
    assert!(hm.get_hidraw_node().is_none());
}

#[test]
fn get_hidraw_node_succeeds() {
    let env = TestEnv::new();
    let device_dir = env.sys_i2c.join("test_addr");
    fs::create_dir_all(device_dir.join("power")).unwrap();
    fs::write(device_dir.join("power/wakeup"), "enabled").unwrap();
    fs::create_dir_all(device_dir.join("0018:ABCD:1234.0001")).unwrap();

    let mut hm = HardwareManager::new("test_addr", &env.sys_i2c, &env.sys_hid, &env.dev_dir);
    assert!(hm.seize_sensor());

    let hid_device_dir = env.sys_hid.join("0018:ABCD:1234.0001");
    fs::create_dir_all(hid_device_dir.join("hidraw/hidraw0")).unwrap();

    let node = hm.get_hidraw_node();
    assert!(node.is_some());
    assert_eq!(
        node.unwrap(),
        env.dev_dir.join("hidraw0").to_string_lossy().to_string()
    );
}
