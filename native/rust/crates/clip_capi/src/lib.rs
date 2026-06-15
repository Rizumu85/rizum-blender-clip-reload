use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::ptr;
use std::slice;

use clip_model::Rect;
use clip_runtime::{ClipSession, RuntimeError};

pub const CLIP_RENDERER_ABI_VERSION: u32 = 1;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipRendererStatus {
    Ok = 0,
    NullArgument = 1,
    InvalidUtf8Path = 2,
    OpenFailed = 3,
    InvalidRegion = 4,
    ReadFailed = 5,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipRendererImageInfo {
    pub width: u32,
    pub height: u32,
    pub root_layer_id: u32,
    pub layer_count: usize,
    pub external_data_count: usize,
}

pub struct ClipRendererSession {
    inner: ClipSession,
}

#[unsafe(no_mangle)]
pub extern "C" fn clip_renderer_abi_version() -> u32 {
    CLIP_RENDERER_ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn clip_renderer_last_error() -> *const c_char {
    LAST_ERROR.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or(ptr::null(), |message| message.as_ptr())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn clip_renderer_session_open(
    path: *const c_char,
    out_session: *mut *mut ClipRendererSession,
) -> ClipRendererStatus {
    clear_last_error();
    if path.is_null() || out_session.is_null() {
        set_last_error("path and out_session must be non-null");
        return ClipRendererStatus::NullArgument;
    }

    let path = unsafe { CStr::from_ptr(path) };
    let Ok(path) = path.to_str() else {
        set_last_error("path must be valid UTF-8");
        return ClipRendererStatus::InvalidUtf8Path;
    };

    match ClipSession::open(path) {
        Ok(inner) => {
            let session = Box::new(ClipRendererSession { inner });
            unsafe {
                *out_session = Box::into_raw(session);
            }
            ClipRendererStatus::Ok
        }
        Err(err) => {
            set_last_runtime_error(&err);
            ClipRendererStatus::OpenFailed
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn clip_renderer_session_close(session: *mut ClipRendererSession) {
    if session.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(session));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn clip_renderer_session_info(
    session: *const ClipRendererSession,
    out_info: *mut ClipRendererImageInfo,
) -> ClipRendererStatus {
    clear_last_error();
    if session.is_null() || out_info.is_null() {
        set_last_error("session and out_info must be non-null");
        return ClipRendererStatus::NullArgument;
    }

    let session = unsafe { &*session };
    let summary = session.inner.summary();
    unsafe {
        *out_info = ClipRendererImageInfo {
            width: summary.canvas.width,
            height: summary.canvas.height,
            root_layer_id: summary.root_layer_id.0,
            layer_count: summary.layer_count,
            external_data_count: summary.external_data_count,
        };
    }
    ClipRendererStatus::Ok
}

#[unsafe(no_mangle)]
pub extern "C" fn clip_renderer_session_read_rgba8(
    session: *mut ClipRendererSession,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    out_pixels: *mut u8,
    out_len: usize,
) -> ClipRendererStatus {
    clear_last_error();
    if session.is_null() || out_pixels.is_null() {
        set_last_error("session and out_pixels must be non-null");
        return ClipRendererStatus::NullArgument;
    }

    let session = unsafe { &mut *session };
    let out = unsafe { slice::from_raw_parts_mut(out_pixels, out_len) };
    let region = Rect {
        x,
        y,
        width,
        height,
    };

    match session.inner.read_rgba8_region(region, out) {
        Ok(()) => ClipRendererStatus::Ok,
        Err(RuntimeError::InvalidRegion) => {
            set_last_error("requested image region is outside the canvas");
            ClipRendererStatus::InvalidRegion
        }
        Err(err) => {
            set_last_runtime_error(&err);
            ClipRendererStatus::ReadFailed
        }
    }
}

fn clear_last_error() {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

fn set_last_runtime_error(err: &RuntimeError) {
    set_last_error(err.to_string());
}

fn set_last_error(message: impl AsRef<str>) {
    let sanitized = message.as_ref().replace('\0', " ");
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = CString::new(sanitized).ok();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_api_opens_summary_and_reads_native_pixel() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Opacity.clip");
        let path = CString::new(path.to_string_lossy().as_bytes()).unwrap();
        let mut session = ptr::null_mut();

        assert_eq!(
            clip_renderer_session_open(path.as_ptr(), &mut session),
            ClipRendererStatus::Ok,
        );
        assert!(!session.is_null());

        let mut info = ClipRendererImageInfo {
            width: 0,
            height: 0,
            root_layer_id: 0,
            layer_count: 0,
            external_data_count: 0,
        };
        assert_eq!(
            clip_renderer_session_info(session, &mut info),
            ClipRendererStatus::Ok,
        );
        assert_eq!(info.width, 512);
        assert_eq!(info.height, 512);
        assert_eq!(info.root_layer_id, 2);
        assert_eq!(info.layer_count, 4);
        assert_eq!(info.external_data_count, 8);

        let mut pixel = [0u8; 4];
        assert_eq!(
            clip_renderer_session_read_rgba8(session, 0, 0, 1, 1, pixel.as_mut_ptr(), pixel.len()),
            ClipRendererStatus::Ok,
        );
        assert_eq!(pixel, [226, 226, 226, 255]);

        clip_renderer_session_close(session);
    }
}
