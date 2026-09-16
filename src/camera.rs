use std::collections::HashSet;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use image::RgbImage;

use crate::constants::*;
use crate::logger::{log_error, log_info, log_warn};
use crate::utils::{
    V4l2Queryctrl, VIDIOC_DQBUF, VIDIOC_G_CTRL, VIDIOC_G_FMT, VIDIOC_QBUF, VIDIOC_QUERYBUF,
    VIDIOC_QUERYCTRL, VIDIOC_REQBUFS, VIDIOC_STREAMOFF, VIDIOC_STREAMON, VIDIOC_S_CTRL,
    V4L2_BUF_TYPE_VIDEO_CAPTURE, V4L2_CID_EXPOSURE_ABSOLUTE, V4L2_CID_EXPOSURE_AUTO,
    V4L2_CTRL_FLAG_DISABLED, V4L2_MEMORY_MMAP, V4L2_PIX_FMT_BGR24, V4L2_PIX_FMT_GREY,
    V4L2_PIX_FMT_MJPEG, V4L2_PIX_FMT_NV12, V4L2_PIX_FMT_RGB24, V4L2_PIX_FMT_UYVY,
    V4L2_PIX_FMT_Y10, V4L2_PIX_FMT_Y12, V4L2_PIX_FMT_Y16, V4L2_PIX_FMT_YUYV,
};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct V4l2PixFormat {
    pub width: u32,
    pub height: u32,
    pub pixelformat: u32,
    pub field: u32,
    pub bytesperline: u32,
    pub sizeimage: u32,
    pub colorspace: u32,
    pub priv_: u32,
    pub flags: u32,
    pub ycbcr_enc: u32,
    pub quantization: u32,
    pub xfer_func: u32,
}

#[repr(C)]
pub struct V4l2Format {
    pub type_: u32,
    pub _pad: u32,
    pub pix: V4l2PixFormat,
    pub _raw: [u8; 152],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct V4l2Requestbuffers {
    pub count: u32,
    pub type_: u32,
    pub memory: u32,
    pub capabilities: u32,
    pub flags: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct V4l2Timecode {
    pub type_: u32,
    pub flags: u32,
    pub frames: u8,
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub userbits: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct V4l2Buffer {
    pub index: u32,
    pub type_: u32,
    pub bytesused: u32,
    pub flags: u32,
    pub field: u32,
    pub _pad1: u32,
    pub timestamp: libc::timeval,
    pub timecode: V4l2Timecode,
    pub sequence: u32,
    pub memory: u32,
    pub m_offset: u32,
    pub _pad_m: u32,
    pub length: u32,
    pub reserved2: u32,
    pub reserved: u32,
    pub _pad2: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct V4l2Control {
    pub id: u32,
    pub value: i32,
}

struct MmappedBuffer {
    ptr: *mut libc::c_void,
    length: usize,
}

unsafe impl Send for MmappedBuffer {}
unsafe impl Sync for MmappedBuffer {}

pub fn decode_v4l2_frame(
    data: &[u8],
    width: u32,
    height: u32,
    pixelformat: u32,
    bytesperline: u32,
) -> Option<RgbImage> {
    if width == 0 || height == 0 {
        return None;
    }

    match pixelformat {
        V4L2_PIX_FMT_MJPEG => image::load_from_memory(data).ok().map(|img| img.to_rgb8()),
        V4L2_PIX_FMT_RGB24 => {
            let stride = (bytesperline as usize).max((width * 3) as usize);
            let unpadded = (width * height * 3) as usize;
            if stride == (width * 3) as usize && data.len() >= unpadded {
                RgbImage::from_raw(width, height, data[..unpadded].to_vec())
            } else if data.len() >= stride * height as usize {
                let mut rgb = vec![0u8; unpadded];
                for y in 0..height as usize {
                    let src_start = y * stride;
                    let dst_start = y * (width as usize) * 3;
                    rgb[dst_start..dst_start + (width as usize) * 3]
                        .copy_from_slice(&data[src_start..src_start + (width as usize) * 3]);
                }
                RgbImage::from_raw(width, height, rgb)
            } else if data.len() >= unpadded {
                RgbImage::from_raw(width, height, data[..unpadded].to_vec())
            } else {
                None
            }
        }
        V4L2_PIX_FMT_BGR24 => {
            let stride = (bytesperline as usize).max((width * 3) as usize);
            if data.len() < stride * height as usize && data.len() < (width * height * 3) as usize {
                return None;
            }
            let mut rgb = vec![0u8; (width * height * 3) as usize];
            let effective_stride = if data.len() >= stride * height as usize {
                stride
            } else {
                (width * 3) as usize
            };
            for y in 0..height as usize {
                let src_row = y * effective_stride;
                let dst_row = y * (width as usize) * 3;
                for x in 0..width as usize {
                    let s = src_row + x * 3;
                    let d = dst_row + x * 3;
                    rgb[d] = data[s + 2];
                    rgb[d + 1] = data[s + 1];
                    rgb[d + 2] = data[s];
                }
            }
            RgbImage::from_raw(width, height, rgb)
        }
        V4L2_PIX_FMT_GREY => {
            let stride = (bytesperline as usize).max(width as usize);
            if data.len() < stride * height as usize && data.len() < (width * height) as usize {
                return None;
            }
            let mut rgb = vec![0u8; (width * height * 3) as usize];
            let effective_stride = if data.len() >= stride * height as usize {
                stride
            } else {
                width as usize
            };
            for y in 0..height as usize {
                let src_row = y * effective_stride;
                let dst_row = y * (width as usize) * 3;
                for x in 0..width as usize {
                    let g = data[src_row + x];
                    let d = dst_row + x * 3;
                    rgb[d] = g;
                    rgb[d + 1] = g;
                    rgb[d + 2] = g;
                }
            }
            RgbImage::from_raw(width, height, rgb)
        }
        V4L2_PIX_FMT_Y10 | V4L2_PIX_FMT_Y12 | V4L2_PIX_FMT_Y16 => {
            let shift = match pixelformat {
                V4L2_PIX_FMT_Y10 => 2,
                V4L2_PIX_FMT_Y12 => 4,
                _ => 8,
            };
            let stride = (bytesperline as usize).max((width * 2) as usize);
            if data.len() < stride * height as usize && data.len() < (width * height * 2) as usize {
                return None;
            }
            let mut rgb = vec![0u8; (width * height * 3) as usize];
            let effective_stride = if data.len() >= stride * height as usize {
                stride
            } else {
                (width * 2) as usize
            };
            for y in 0..height as usize {
                let src_row = y * effective_stride;
                let dst_row = y * (width as usize) * 3;
                for x in 0..width as usize {
                    let s = src_row + x * 2;
                    let val = u16::from_le_bytes([data[s], data[s + 1]]);
                    let g = (val >> shift).min(255) as u8;
                    let d = dst_row + x * 3;
                    rgb[d] = g;
                    rgb[d + 1] = g;
                    rgb[d + 2] = g;
                }
            }
            RgbImage::from_raw(width, height, rgb)
        }
        V4L2_PIX_FMT_YUYV => {
            let stride = (bytesperline as usize).max((width * 2) as usize);
            if data.len() < stride * height as usize && data.len() < (width * height * 2) as usize {
                return None;
            }
            let mut rgb = vec![0u8; (width * height * 3) as usize];
            let effective_stride = if data.len() >= stride * height as usize {
                stride
            } else {
                (width * 2) as usize
            };
            for y in 0..height as usize {
                let src_row = y * effective_stride;
                let dst_row = y * (width as usize) * 3;
                let mut x = 0;
                while x + 1 < width as usize {
                    let s = src_row + x * 2;
                    let d = dst_row + x * 3;
                    let y0 = data[s] as i32;
                    let u = data[s + 1] as i32;
                    let y1 = data[s + 2] as i32;
                    let v = data[s + 3] as i32;

                    let c0 = y0 - 16;
                    let c1 = y1 - 16;
                    let d_u = u - 128;
                    let e_v = v - 128;

                    rgb[d] = ((298 * c0 + 409 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 1] =
                        ((298 * c0 - 100 * d_u - 208 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 2] = ((298 * c0 + 516 * d_u + 128) >> 8).clamp(0, 255) as u8;

                    rgb[d + 3] = ((298 * c1 + 409 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 4] =
                        ((298 * c1 - 100 * d_u - 208 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 5] = ((298 * c1 + 516 * d_u + 128) >> 8).clamp(0, 255) as u8;

                    x += 2;
                }
            }
            RgbImage::from_raw(width, height, rgb)
        }
        V4L2_PIX_FMT_UYVY => {
            let stride = (bytesperline as usize).max((width * 2) as usize);
            if data.len() < stride * height as usize && data.len() < (width * height * 2) as usize {
                return None;
            }
            let mut rgb = vec![0u8; (width * height * 3) as usize];
            let effective_stride = if data.len() >= stride * height as usize {
                stride
            } else {
                (width * 2) as usize
            };
            for y in 0..height as usize {
                let src_row = y * effective_stride;
                let dst_row = y * (width as usize) * 3;
                let mut x = 0;
                while x + 1 < width as usize {
                    let s = src_row + x * 2;
                    let d = dst_row + x * 3;
                    let u = data[s] as i32;
                    let y0 = data[s + 1] as i32;
                    let v = data[s + 2] as i32;
                    let y1 = data[s + 3] as i32;

                    let c0 = y0 - 16;
                    let c1 = y1 - 16;
                    let d_u = u - 128;
                    let e_v = v - 128;

                    rgb[d] = ((298 * c0 + 409 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 1] =
                        ((298 * c0 - 100 * d_u - 208 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 2] = ((298 * c0 + 516 * d_u + 128) >> 8).clamp(0, 255) as u8;

                    rgb[d + 3] = ((298 * c1 + 409 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 4] =
                        ((298 * c1 - 100 * d_u - 208 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 5] = ((298 * c1 + 516 * d_u + 128) >> 8).clamp(0, 255) as u8;

                    x += 2;
                }
            }
            RgbImage::from_raw(width, height, rgb)
        }
        V4L2_PIX_FMT_NV12 => {
            let stride_y = (bytesperline as usize).max(width as usize);
            let min_len = (width * height) as usize + (width * height / 2) as usize;
            let padded_len = stride_y * height as usize + stride_y * (height as usize / 2);
            if data.len() < padded_len && data.len() < min_len {
                return None;
            }
            let effective_stride = if data.len() >= padded_len {
                stride_y
            } else {
                width as usize
            };
            let mut rgb = vec![0u8; (width * height * 3) as usize];
            let uv_start = effective_stride * height as usize;
            for y in 0..height as usize {
                let dst_row = y * (width as usize) * 3;
                let uv_row = (y / 2) * effective_stride;
                for x in 0..width as usize {
                    let y_val = data[y * effective_stride + x] as i32;
                    let uv_idx = uv_start + uv_row + (x / 2) * 2;
                    let u = data[uv_idx] as i32;
                    let v = data[uv_idx + 1] as i32;

                    let c = y_val - 16;
                    let d_u = u - 128;
                    let e_v = v - 128;

                    let d = dst_row + x * 3;
                    rgb[d] = ((298 * c + 409 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 1] =
                        ((298 * c - 100 * d_u - 208 * e_v + 128) >> 8).clamp(0, 255) as u8;
                    rgb[d + 2] = ((298 * c + 516 * d_u + 128) >> 8).clamp(0, 255) as u8;
                }
            }
            RgbImage::from_raw(width, height, rgb)
        }
        _ => {
            log_warn(format!(
                "[Camera] Unsupported pixel format: 0x{pixelformat:08x}"
            ));
            None
        }
    }
}

static DEVICE_LOCK: OnceLock<(Mutex<HashSet<String>>, Condvar)> = OnceLock::new();

fn acquire_device_lock(path: &str, timeout: Duration) -> bool {
    let (lock, cvar) = DEVICE_LOCK.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()));
    let mut active = lock.lock().unwrap();
    let start = Instant::now();
    while active.contains(path) {
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            return false;
        }
        let (new_active, wait_res) = cvar.wait_timeout(active, timeout - elapsed).unwrap();
        active = new_active;
        if wait_res.timed_out() && active.contains(path) {
            return false;
        }
    }
    active.insert(path.to_string());
    true
}

fn release_device_lock(path: &str) {
    let (lock, cvar) = DEVICE_LOCK.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()));
    let mut active = lock.lock().unwrap();
    active.remove(path);
    cvar.notify_all();
}

pub struct V4l2CaptureSession {
    pub device_path: String,
    pub fd: libc::c_int,
    pub width: u32,
    pub height: u32,
    pub pixelformat: u32,
    pub bytesperline: u32,
    buffers: Vec<MmappedBuffer>,
    is_streaming: bool,
}

impl V4l2CaptureSession {
    pub fn open(device_path: &str) -> Option<Self> {
        if !acquire_device_lock(device_path, Duration::from_secs(5)) {
            log_error(format!(
                "[Camera] Timed out waiting for device lock on {device_path}"
            ));
            return None;
        }

        let c_path = match CString::new(device_path) {
            Ok(p) => p,
            Err(_) => {
                release_device_lock(device_path);
                return None;
            }
        };

        for attempt in 0..CAPTURE_RETRY_ATTEMPTS {
            let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK) };
            if fd < 0 {
                let err = unsafe { *libc::__errno_location() };
                if err == libc::EBUSY && attempt + 1 < CAPTURE_RETRY_ATTEMPTS {
                    log_warn(format!(
                        "[Camera] Device busy on open {device_path}. Retrying ({}/{})...",
                        attempt + 1,
                        CAPTURE_RETRY_ATTEMPTS
                    ));
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
                log_error(format!("[Camera] Failed to open {device_path} (errno={err})"));
                release_device_lock(device_path);
                return None;
            }

            let mut fmt: V4l2Format = unsafe { std::mem::zeroed() };
            fmt.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            let ret = unsafe { libc::ioctl(fd, VIDIOC_G_FMT, &mut fmt) };
            if ret != 0 {
                log_error(format!("[Camera] VIDIOC_G_FMT failed on {device_path}"));
                unsafe { libc::close(fd) };
                release_device_lock(device_path);
                return None;
            }

            let width = fmt.pix.width;
            let height = fmt.pix.height;
            let pixelformat = fmt.pix.pixelformat;
            let bytesperline = fmt.pix.bytesperline;
            if width == 0 || height == 0 {
                log_error(format!(
                    "[Camera] Invalid format {width}x{height} on {device_path}"
                ));
                unsafe { libc::close(fd) };
                release_device_lock(device_path);
                return None;
            }

            let mut req = V4l2Requestbuffers {
                count: 4,
                type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
                memory: V4L2_MEMORY_MMAP,
                capabilities: 0,
                flags: 0,
                reserved: [0; 3],
            };
            let req_ret = unsafe { libc::ioctl(fd, VIDIOC_REQBUFS, &mut req) };

            if req_ret != 0 || req.count == 0 {
                let err = unsafe { *libc::__errno_location() };
                unsafe { libc::close(fd) };
                if (err == libc::EBUSY || req.count == 0) && attempt + 1 < CAPTURE_RETRY_ATTEMPTS {
                    log_warn(format!(
                        "[Camera] Device busy on REQBUFS {device_path}. Retrying ({}/{})...",
                        attempt + 1,
                        CAPTURE_RETRY_ATTEMPTS
                    ));
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
                log_error(format!(
                    "[Camera] VIDIOC_REQBUFS failed on {device_path}: ret={req_ret}, count={}, errno={err}",
                    req.count
                ));
                release_device_lock(device_path);
                return None;
            }

            let mut buffers = Vec::with_capacity(req.count as usize);
            let mut setup_ok = true;

            for i in 0..req.count {
                let mut buf: V4l2Buffer = unsafe { std::mem::zeroed() };
                buf.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
                buf.memory = V4L2_MEMORY_MMAP;
                buf.index = i;

                if unsafe { libc::ioctl(fd, VIDIOC_QUERYBUF, &mut buf) } != 0 {
                    log_error(format!(
                        "[Camera] VIDIOC_QUERYBUF failed for buffer {i} on {device_path}"
                    ));
                    setup_ok = false;
                    break;
                }

                let ptr = unsafe {
                    libc::mmap(
                        std::ptr::null_mut(),
                        buf.length as usize,
                        libc::PROT_READ | libc::PROT_WRITE,
                        libc::MAP_SHARED,
                        fd,
                        buf.m_offset as libc::off_t,
                    )
                };

                if ptr == libc::MAP_FAILED {
                    log_error(format!(
                        "[Camera] mmap failed for buffer {i} on {device_path}"
                    ));
                    setup_ok = false;
                    break;
                }

                buffers.push(MmappedBuffer {
                    ptr,
                    length: buf.length as usize,
                });

                if unsafe { libc::ioctl(fd, VIDIOC_QBUF, &mut buf) } != 0 {
                    log_error(format!(
                        "[Camera] Initial VIDIOC_QBUF failed for buffer {i} on {device_path}"
                    ));
                    setup_ok = false;
                    break;
                }
            }

            if setup_ok {
                let mut typ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
                if unsafe { libc::ioctl(fd, VIDIOC_STREAMON, &mut typ) } == 0 {
                    return Some(Self {
                        device_path: device_path.to_string(),
                        fd,
                        width,
                        height,
                        pixelformat,
                        bytesperline,
                        buffers,
                        is_streaming: true,
                    });
                } else {
                    let err = unsafe { *libc::__errno_location() };
                    if err == libc::EBUSY && attempt + 1 < CAPTURE_RETRY_ATTEMPTS {
                        log_warn(format!(
                            "[Camera] STREAMON busy on {device_path}. Retrying ({}/{})...",
                            attempt + 1,
                            CAPTURE_RETRY_ATTEMPTS
                        ));
                        Self::cleanup_buffers(fd, &buffers);
                        unsafe { libc::close(fd) };
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    log_error(format!(
                        "[Camera] VIDIOC_STREAMON failed on {device_path} with errno={err}"
                    ));
                }
            }

            Self::cleanup_buffers(fd, &buffers);
            unsafe { libc::close(fd) };
            if attempt + 1 < CAPTURE_RETRY_ATTEMPTS {
                thread::sleep(Duration::from_millis(500));
            }
        }

        log_error(format!(
            "[Camera] Failed to establish capture session on {device_path} after retries"
        ));
        release_device_lock(device_path);
        None
    }

    fn cleanup_buffers(fd: libc::c_int, buffers: &[MmappedBuffer]) {
        for buf in buffers {
            unsafe {
                libc::munmap(buf.ptr, buf.length);
            }
        }
        if !buffers.is_empty() {
            let mut req = V4l2Requestbuffers {
                count: 0,
                type_: V4L2_BUF_TYPE_VIDEO_CAPTURE,
                memory: V4L2_MEMORY_MMAP,
                capabilities: 0,
                flags: 0,
                reserved: [0; 3],
            };
            unsafe {
                libc::ioctl(fd, VIDIOC_REQBUFS, &mut req);
            }
        }
    }

    pub fn discard_frame(&mut self, timeout_ms: i32) -> bool {
        if !self.is_streaming {
            return false;
        }

        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms.max(0) as u64);
        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return false;
            }
            let remaining = (timeout - elapsed).as_millis().max(1) as i32;
            let mut pfd = libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ret = unsafe { libc::poll(&mut pfd, 1, remaining) };
            if ret < 0 {
                let err = unsafe { *libc::__errno_location() };
                if err == libc::EINTR {
                    continue;
                }
                return false;
            }
            if ret == 0 {
                return false;
            }
            if (pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
                return false;
            }
            if (pfd.revents & libc::POLLIN) != 0 {
                break;
            }
        }

        let mut buf: V4l2Buffer = unsafe { std::mem::zeroed() };
        buf.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        buf.memory = V4L2_MEMORY_MMAP;

        if unsafe { libc::ioctl(self.fd, VIDIOC_DQBUF, &mut buf) } != 0 {
            return false;
        }

        let _ = unsafe { libc::ioctl(self.fd, VIDIOC_QBUF, &mut buf) };
        true
    }

    pub fn read_frame(&mut self, timeout_ms: i32) -> Option<RgbImage> {
        if !self.is_streaming {
            return None;
        }

        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms.max(0) as u64);
        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                log_error(format!(
                    "[Camera] read_frame timed out after {timeout_ms}ms on {}",
                    self.device_path
                ));
                return None;
            }
            let remaining = (timeout - elapsed).as_millis().max(1) as i32;
            let mut pfd = libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ret = unsafe { libc::poll(&mut pfd, 1, remaining) };
            if ret < 0 {
                let err = unsafe { *libc::__errno_location() };
                if err == libc::EINTR {
                    continue;
                }
                log_error(format!(
                    "[Camera] read_frame poll failed on {}: errno={err}",
                    self.device_path
                ));
                return None;
            }
            if ret == 0 {
                log_error(format!(
                    "[Camera] read_frame poll timed out on {}",
                    self.device_path
                ));
                return None;
            }
            if (pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0 {
                log_error(format!(
                    "[Camera] read_frame poll error revents=0x{:x} on {}",
                    pfd.revents, self.device_path
                ));
                return None;
            }
            if (pfd.revents & libc::POLLIN) != 0 {
                break;
            }
        }

        let mut buf: V4l2Buffer = unsafe { std::mem::zeroed() };
        buf.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        buf.memory = V4L2_MEMORY_MMAP;

        if unsafe { libc::ioctl(self.fd, VIDIOC_DQBUF, &mut buf) } != 0 {
            let err = unsafe { *libc::__errno_location() };
            log_error(format!(
                "[Camera] read_frame VIDIOC_DQBUF failed on {} with errno={err}",
                self.device_path
            ));
            return None;
        }

        let idx = buf.index as usize;
        let bytesused = buf.bytesused as usize;
        let img = if idx < self.buffers.len() {
            let buf_len = self.buffers[idx].length;
            let data_len = if bytesused > 0 {
                bytesused.min(buf_len)
            } else {
                buf_len
            };
            let slice = unsafe {
                std::slice::from_raw_parts(self.buffers[idx].ptr as *const u8, data_len)
            };
            let res = decode_v4l2_frame(
                slice,
                self.width,
                self.height,
                self.pixelformat,
                self.bytesperline,
            );
            if res.is_none() {
                log_error(format!(
                    "[Camera] decode_v4l2_frame failed on {}: slice_len={}, width={}, height={}, pixfmt=0x{:x}, bpl={}",
                    self.device_path, slice.len(), self.width, self.height, self.pixelformat, self.bytesperline
                ));
            }
            res
        } else {
            log_error(format!(
                "[Camera] read_frame invalid buffer idx={idx} on {}",
                self.device_path
            ));
            None
        };

        let _ = unsafe { libc::ioctl(self.fd, VIDIOC_QBUF, &mut buf) };
        img
    }
}

impl Drop for V4l2CaptureSession {
    fn drop(&mut self) {
        if self.is_streaming {
            let mut typ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            unsafe {
                libc::ioctl(self.fd, VIDIOC_STREAMOFF, &mut typ);
            }
            self.is_streaming = false;
        }
        Self::cleanup_buffers(self.fd, &self.buffers);
        self.buffers.clear();
        if self.fd >= 0 {
            unsafe {
                libc::close(self.fd);
            }
            self.fd = -1;
        }
        release_device_lock(&self.device_path);
    }
}

pub trait ICamera: Send + Sync {
    fn trigger_ir_emitter(&mut self);
    fn capture(&mut self) -> Option<RgbImage>;
    fn capture_averaged(&mut self, num_frames: usize) -> Option<RgbImage>;
    fn capture_hdr(&mut self) -> Option<RgbImage>;
    fn supports_manual_exposure(&self) -> bool;
}

pub struct Camera {
    device_path: String,
    ir_emitter_path: PathBuf,
    is_ir_camera: bool,
    supports_manual_exposure: bool,
}

impl Camera {
    pub fn new(device_path: impl Into<String>, is_ir: bool, ir_cmd_path: Option<&Path>) -> Self {
        let dev_str = device_path.into();
        let ir_path = match ir_cmd_path {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from(IR_EMITTER_PATH),
        };

        let supports_manual = Self::detect_exposure_support(&dev_str);
        if supports_manual {
            log_info(format!("{dev_str} supports manual exposure"));
        }

        Self {
            device_path: dev_str,
            ir_emitter_path: ir_path,
            is_ir_camera: is_ir,
            supports_manual_exposure: supports_manual,
        }
    }

    fn detect_exposure_support(device_path: &str) -> bool {
        let c_path = match CString::new(device_path) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            return false;
        }

        let mut queryctrl: V4l2Queryctrl = unsafe { std::mem::zeroed() };
        queryctrl.id = V4L2_CID_EXPOSURE_ABSOLUTE;

        let ret = unsafe { libc::ioctl(fd, VIDIOC_QUERYCTRL, &mut queryctrl) };
        unsafe { libc::close(fd) };

        ret == 0 && (queryctrl.flags & V4L2_CTRL_FLAG_DISABLED) == 0
    }

    fn open_session(&mut self) -> Option<V4l2CaptureSession> {
        let mut session = match V4l2CaptureSession::open(&self.device_path) {
            Some(s) => s,
            None => {
                log_error(format!("[Camera] Failed to open {}", self.device_path));
                return None;
            }
        };

        // Keep camera open while triggering IR emitter
        if self.is_ir_camera {
            self.trigger_ir_emitter();
            thread::sleep(Duration::from_millis(IR_TRIGGER_DELAY_MS));
        }

        // Discard initial warmup frames for auto-exposure/sensor settling
        for _ in 0..CAMERA_WARMUP_FRAMES {
            if !session.discard_frame(500) {
                break;
            }
        }
        thread::sleep(Duration::from_millis(CAMERA_WARMUP_DELAY_MS));

        Some(session)
    }
}

impl ICamera for Camera {
    fn trigger_ir_emitter(&mut self) {
        log_info("[Camera] Triggering IR emitter");

        let mut child = match ProcessCommand::new(&self.ir_emitter_path)
            .arg("run")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                log_error(format!(
                    "[Camera] posix_spawn failed for {}: {}",
                    self.ir_emitter_path.display(),
                    e
                ));
                return;
            }
        };

        const IR_TIMEOUT_MS: u64 = 5000;
        let start = Instant::now();
        let timeout = Duration::from_millis(IR_TIMEOUT_MS);

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        log_info(format!(
                            "[Camera] IR emitter exited with code: {}",
                            status.code().unwrap_or(0)
                        ));
                    } else {
                        log_error(format!(
                            "[Camera] IR emitter exited with code: {}",
                            status.code().unwrap_or(-1)
                        ));
                    }
                    break;
                }
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        log_error("[Camera] IR emitter timed out; sending SIGKILL.");
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => {
                    log_error("[Camera] IR emitter terminated abnormally.");
                    break;
                }
            }
        }
    }

    fn capture(&mut self) -> Option<RgbImage> {
        let mut session = self.open_session()?;
        let frame = session.read_frame(2000);
        if frame.is_none() {
            log_error(format!(
                "[Camera] Failed to capture frame from {}",
                self.device_path
            ));
        }
        frame
    }

    fn capture_averaged(&mut self, num_frames: usize) -> Option<RgbImage> {
        if num_frames == 0 {
            return None;
        }

        let mut session = match self.open_session() {
            Some(s) => s,
            None => {
                log_error("[Camera] Failed to open for averaging");
                return None;
            }
        };

        let mut sum_r = Vec::new();
        let mut sum_g = Vec::new();
        let mut sum_b = Vec::new();
        let mut width = 0;
        let mut height = 0;
        let mut count = 0;

        for _ in 0..num_frames {
            if let Some(frame) = session.read_frame(2000) {
                if count == 0 {
                    width = frame.width();
                    height = frame.height();
                    let len = (width * height) as usize;
                    sum_r = vec![0.0f32; len];
                    sum_g = vec![0.0f32; len];
                    sum_b = vec![0.0f32; len];
                } else if frame.width() != width || frame.height() != height {
                    continue;
                }

                for (idx, pixel) in frame.pixels().enumerate() {
                    sum_r[idx] += pixel[0] as f32;
                    sum_g[idx] += pixel[1] as f32;
                    sum_b[idx] += pixel[2] as f32;
                }
                count += 1;
            }
        }

        if count == 0 {
            return None;
        }

        let mut avg_img = RgbImage::new(width, height);
        for (idx, pixel) in avg_img.pixels_mut().enumerate() {
            let r = (sum_r[idx] / count as f32).round().clamp(0.0, 255.0) as u8;
            let g = (sum_g[idx] / count as f32).round().clamp(0.0, 255.0) as u8;
            let b = (sum_b[idx] / count as f32).round().clamp(0.0, 255.0) as u8;
            *pixel = image::Rgb([r, g, b]);
        }

        log_info(format!("[Camera] Averaged {count} frames"));
        Some(avg_img)
    }

    fn capture_hdr(&mut self) -> Option<RgbImage> {
        if !self.supports_manual_exposure {
            log_warn("[Camera] HDR not supported, falling back to averaging");
            return self.capture_averaged(CAMERA_AVERAGE_FRAMES);
        }

        let mut session = match self.open_session() {
            Some(s) => s,
            None => {
                log_error("[Camera] Failed to open for HDR");
                return None;
            }
        };

        // Save original auto-exposure
        let mut orig_auto = V4l2Control {
            id: V4L2_CID_EXPOSURE_AUTO,
            value: 0,
        };
        unsafe {
            let _ = libc::ioctl(session.fd, VIDIOC_G_CTRL, &mut orig_auto);
        }

        // Disable auto-exposure (1 = manual)
        let mut manual_ctrl = V4l2Control {
            id: V4L2_CID_EXPOSURE_AUTO,
            value: 1,
        };
        unsafe {
            let _ = libc::ioctl(session.fd, VIDIOC_S_CTRL, &mut manual_ctrl);
        }

        let exp_values = [HDR_EXPOSURE_1, HDR_EXPOSURE_2, HDR_EXPOSURE_3];
        let mut exposures = Vec::new();
        let mut last_frame = None;

        for exp in exp_values {
            let mut exp_ctrl = V4l2Control {
                id: V4L2_CID_EXPOSURE_ABSOLUTE,
                value: exp,
            };
            unsafe {
                let _ = libc::ioctl(session.fd, VIDIOC_S_CTRL, &mut exp_ctrl);
            }
            thread::sleep(Duration::from_millis(HDR_SETTLE_MS));

            for _ in 0..3 {
                let _ = session.discard_frame(500);
            }

            if let Some(frame) = session.read_frame(2000) {
                last_frame = Some(frame.clone());
                exposures.push(frame);
            }
        }

        // Restore original auto-exposure
        unsafe {
            let _ = libc::ioctl(session.fd, VIDIOC_S_CTRL, &mut orig_auto);
        }

        if exposures.len() < 2 {
            log_error("[Camera] HDR failed, using last frame");
            return last_frame;
        }

        // Exposure fusion (well-exposedness weighting)
        let width = exposures[0].width();
        let height = exposures[0].height();
        let num_pixels = (width * height) as usize;

        let mut sum_r = vec![0.0f32; num_pixels];
        let mut sum_g = vec![0.0f32; num_pixels];
        let mut sum_b = vec![0.0f32; num_pixels];
        let mut sum_w = vec![0.0f32; num_pixels];

        let weight_fn = |v: u8| -> f32 {
            let val = v as f32 / 255.0;
            // Gaussian centered around 0.5 with sigma = 0.2
            (-((val - 0.5) * (val - 0.5)) / (2.0 * 0.2 * 0.2)).exp().max(1e-4)
        };

        for exp_img in &exposures {
            if exp_img.width() != width || exp_img.height() != height {
                continue;
            }
            for (idx, p) in exp_img.pixels().enumerate() {
                let [r, g, b] = p.0;
                let wr = weight_fn(r);
                let wg = weight_fn(g);
                let wb = weight_fn(b);
                let w = wr * wg * wb;

                sum_r[idx] += r as f32 * w;
                sum_g[idx] += g as f32 * w;
                sum_b[idx] += b as f32 * w;
                sum_w[idx] += w;
            }
        }

        let mut result = RgbImage::new(width, height);
        for (idx, p) in result.pixels_mut().enumerate() {
            let w = sum_w[idx];
            let r = if w > 0.0 {
                (sum_r[idx] / w).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
            let g = if w > 0.0 {
                (sum_g[idx] / w).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
            let b = if w > 0.0 {
                (sum_b[idx] / w).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
            *p = image::Rgb([r, g, b]);
        }

        log_info(format!("[Camera] HDR merged {} exposures", exposures.len()));
        Some(result)
    }

    fn supports_manual_exposure(&self) -> bool {
        self.supports_manual_exposure
    }
}
