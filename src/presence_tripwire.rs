use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::hardware_manager::HardwareId;
use crate::sensor_factory::ISensorFactory;
use crate::sensor_parser::SensorParser;

pub type HidDevicePtr = *mut libc::c_void;
pub type ReadTimeoutFn = dyn Fn(HidDevicePtr, &mut [u8], i32) -> i32 + Send + Sync;

#[derive(Clone)]
pub struct PresenceTripwireHidOps {
    pub init: Arc<dyn Fn() -> i32 + Send + Sync>,
    pub open_path: Arc<dyn Fn(&str) -> HidDevicePtr + Send + Sync>,
    pub read_timeout: Arc<ReadTimeoutFn>,
    pub close_fn: Arc<dyn Fn(HidDevicePtr) + Send + Sync>,
    pub exit_fn: Arc<dyn Fn() + Send + Sync>,
}

struct HidApiLib {
    _handle: *mut libc::c_void,
    init: unsafe extern "C" fn() -> libc::c_int,
    open_path: unsafe extern "C" fn(*const libc::c_char) -> HidDevicePtr,
    read_timeout:
        unsafe extern "C" fn(HidDevicePtr, *mut u8, libc::size_t, libc::c_int) -> libc::c_int,
    close_fn: unsafe extern "C" fn(HidDevicePtr),
    exit_fn: unsafe extern "C" fn() -> libc::c_int,
}

unsafe impl Send for HidApiLib {}
unsafe impl Sync for HidApiLib {}

impl HidApiLib {
    fn try_load() -> Option<Self> {
        let names = [
            c"libhidapi-hidraw.so.0".as_ptr(),
            c"libhidapi-hidraw.so".as_ptr(),
            c"libhidapi-libusb.so.0".as_ptr(),
            c"libhidapi-libusb.so".as_ptr(),
        ];

        for &name in &names {
            let handle = unsafe { libc::dlopen(name, libc::RTLD_LAZY) };
            if !handle.is_null() {
                unsafe {
                    let init_ptr = libc::dlsym(handle, c"hid_init".as_ptr());
                    let open_ptr = libc::dlsym(handle, c"hid_open_path".as_ptr());
                    let read_ptr = libc::dlsym(handle, c"hid_read_timeout".as_ptr());
                    let close_ptr = libc::dlsym(handle, c"hid_close".as_ptr());
                    let exit_ptr = libc::dlsym(handle, c"hid_exit".as_ptr());

                    if !init_ptr.is_null()
                        && !open_ptr.is_null()
                        && !read_ptr.is_null()
                        && !close_ptr.is_null()
                        && !exit_ptr.is_null()
                    {
                        return Some(Self {
                            _handle: handle,
                            init: std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn() -> libc::c_int>(init_ptr),
                            open_path: std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn(*const libc::c_char) -> HidDevicePtr>(open_ptr),
                            read_timeout: std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn(HidDevicePtr, *mut u8, libc::size_t, libc::c_int) -> libc::c_int>(read_ptr),
                            close_fn: std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn(HidDevicePtr)>(close_ptr),
                            exit_fn: std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn() -> libc::c_int>(exit_ptr),
                        });
                    }
                    libc::dlclose(handle);
                }
            }
        }
        None
    }
}

impl Default for PresenceTripwireHidOps {
    fn default() -> Self {
        if let Some(lib) = HidApiLib::try_load() {
            let lib = Arc::new(lib);
            let l_init = Arc::clone(&lib);
            let l_open = Arc::clone(&lib);
            let l_read = Arc::clone(&lib);
            let l_close = Arc::clone(&lib);
            let l_exit = Arc::clone(&lib);

            Self {
                init: Arc::new(move || unsafe { (l_init.init)() }),
                open_path: Arc::new(move |p: &str| {
                    if let Ok(c_path) = std::ffi::CString::new(p) {
                        unsafe { (l_open.open_path)(c_path.as_ptr()) }
                    } else {
                        std::ptr::null_mut()
                    }
                }),
                read_timeout: Arc::new(move |h, buf, timeout| unsafe {
                    (l_read.read_timeout)(h, buf.as_mut_ptr(), buf.len(), timeout)
                }),
                close_fn: Arc::new(move |h| unsafe { (l_close.close_fn)(h) }),
                exit_fn: Arc::new(move || unsafe {
                    let _ = (l_exit.exit_fn)();
                }),
            }
        } else {
            Self {
                init: Arc::new(|| -1),
                open_path: Arc::new(|_| std::ptr::null_mut()),
                read_timeout: Arc::new(|_, _, _| -1),
                close_fn: Arc::new(|_| {}),
                exit_fn: Arc::new(|| {}),
            }
        }
    }
}

pub type PresenceCallback = Arc<dyn Fn(bool, i32) + Send + Sync>;

pub struct PresenceTripwire {
    factory: Arc<dyn ISensorFactory>,
    hid_ops: PresenceTripwireHidOps,
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    handle: Option<HidDevicePtr>,
}

impl PresenceTripwire {
    pub fn new(factory: Arc<dyn ISensorFactory>, hid_ops: PresenceTripwireHidOps) -> Self {
        Self {
            factory,
            hid_ops,
            running: Arc::new(AtomicBool::new(false)),
            worker: None,
            handle: None,
        }
    }

    pub fn start<F>(
        &mut self,
        hidraw_node: &str,
        hardware_id: &HardwareId,
        callback: F,
    ) -> bool
    where
        F: Fn(bool, i32) + Send + Sync + 'static,
    {
        if self.running.load(Ordering::SeqCst) {
            return false;
        }

        let parser: Box<dyn SensorParser> = match self.factory.create(hardware_id.str()) {
            Some(p) => p,
            None => return false,
        };

        if (self.hid_ops.init)() != 0 {
            return false;
        }

        let handle = (self.hid_ops.open_path)(hidraw_node);
        if handle.is_null() {
            (self.hid_ops.exit_fn)();
            return false;
        }

        self.handle = Some(handle);
        self.running.store(true, Ordering::SeqCst);

        let running_clone = Arc::clone(&self.running);
        let hid_ops_clone = self.hid_ops.clone();
        let cb: PresenceCallback = Arc::new(callback);

        let handle_addr = handle as usize;

        let worker = thread::spawn(move || {
            const READ_BUFFER_SIZE: usize = 64;
            const POLL_TIMEOUT_MS: i32 = 500;
            let mut buffer = [0u8; READ_BUFFER_SIZE];
            let raw_handle = handle_addr as HidDevicePtr;

            while running_clone.load(Ordering::SeqCst) {
                let bytes = (hid_ops_clone.read_timeout)(raw_handle, &mut buffer, POLL_TIMEOUT_MS);
                if bytes > 0 {
                    let len = bytes as usize;
                    if let Some(state) = parser.parse_payload(&buffer[..len]) {
                        cb(state.human_present, state.confidence_cm);
                    }
                } else if bytes < 0 {
                    running_clone.store(false, Ordering::SeqCst);
                    break;
                }
            }
        });

        self.worker = Some(worker);
        true
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(handle) = self.handle.take() {
            (self.hid_ops.close_fn)(handle);
            (self.hid_ops.exit_fn)();
        }
    }
}

impl Drop for PresenceTripwire {
    fn drop(&mut self) {
        self.stop();
    }
}
