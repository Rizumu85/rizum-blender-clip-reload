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
    BufferTooSmall = 6,
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

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClipRendererSupportInfo {
    pub source_count: usize,
    pub unsupported_count: usize,
    pub raster_count: usize,
    pub raster_bytes: u64,
    pub max_raster_layer_id: u32,
    pub max_raster_width: u32,
    pub max_raster_height: u32,
    pub max_raster_bytes: u64,
    pub mask_count: usize,
    pub mask_bytes: u64,
    pub max_mask_layer_id: u32,
    pub max_mask_width: u32,
    pub max_mask_height: u32,
    pub max_mask_bytes: u64,
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
pub extern "C" fn clip_renderer_session_open_memory(
    bytes: *const u8,
    len: usize,
    out_session: *mut *mut ClipRendererSession,
) -> ClipRendererStatus {
    clear_last_error();
    if bytes.is_null() || out_session.is_null() {
        set_last_error("bytes and out_session must be non-null");
        return ClipRendererStatus::NullArgument;
    }

    let data = unsafe { slice::from_raw_parts(bytes, len) }.to_vec();
    match ClipSession::from_bytes(data) {
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

#[unsafe(no_mangle)]
pub extern "C" fn clip_renderer_session_support_info(
    session: *mut ClipRendererSession,
    out_info: *mut ClipRendererSupportInfo,
    out_report: *mut c_char,
    report_len: usize,
    out_required_report_len: *mut usize,
) -> ClipRendererStatus {
    clear_last_error();
    if session.is_null() || out_info.is_null() {
        set_last_error("session and out_info must be non-null");
        return ClipRendererStatus::NullArgument;
    }

    let session = unsafe { &mut *session };
    match session.inner.check_normal_raster_stack_support() {
        Ok(result) => {
            let stats = result.resource_stats;
            unsafe {
                *out_info = ClipRendererSupportInfo {
                    source_count: result.source_count,
                    unsupported_count: result.unsupported.len(),
                    raster_count: stats.raster_count,
                    raster_bytes: stats.raster_bytes,
                    max_raster_layer_id: stats.max_raster_layer_id.map_or(0, |layer_id| layer_id.0),
                    max_raster_width: stats.max_raster_width,
                    max_raster_height: stats.max_raster_height,
                    max_raster_bytes: stats.max_raster_bytes,
                    mask_count: stats.mask_count,
                    mask_bytes: stats.mask_bytes,
                    max_mask_layer_id: stats.max_mask_layer_id.map_or(0, |layer_id| layer_id.0),
                    max_mask_width: stats.max_mask_width,
                    max_mask_height: stats.max_mask_height,
                    max_mask_bytes: stats.max_mask_bytes,
                };
            }
            write_support_report(
                &support_report(result.source_count, &result.unsupported),
                out_report,
                report_len,
                out_required_report_len,
            )
        }
        Err(err) => {
            set_last_runtime_error(&err);
            ClipRendererStatus::ReadFailed
        }
    }
}

fn support_report(
    source_count: usize,
    unsupported: &[clip_runtime::SimpleRasterStackUnsupported],
) -> String {
    if unsupported.is_empty() {
        return format!("Full native support for {source_count} source(s).");
    }
    let mut report = format!("{} unsupported node(s).", unsupported.len());
    for item in unsupported {
        report.push_str(&format!(
            "\n- layer {} node {} {:?}: {}",
            item.layer_id.0, item.render_node_id.0, item.kind, item.reason,
        ));
    }
    report
}

fn write_support_report(
    report: &str,
    out_report: *mut c_char,
    report_len: usize,
    out_required_report_len: *mut usize,
) -> ClipRendererStatus {
    let bytes = report.as_bytes();
    let required_len = bytes.len() + 1;
    if !out_required_report_len.is_null() {
        unsafe {
            *out_required_report_len = required_len;
        }
    }
    if out_report.is_null() || report_len == 0 {
        return ClipRendererStatus::Ok;
    }
    if report_len < required_len {
        set_last_error("support report buffer is too small");
        return ClipRendererStatus::BufferTooSmall;
    }
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), out_report.cast::<u8>(), bytes.len());
        *out_report.add(bytes.len()) = 0;
    }
    ClipRendererStatus::Ok
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

        let mut support = ClipRendererSupportInfo {
            source_count: 0,
            unsupported_count: 0,
            raster_count: 0,
            raster_bytes: 0,
            max_raster_layer_id: 0,
            max_raster_width: 0,
            max_raster_height: 0,
            max_raster_bytes: 0,
            mask_count: 0,
            mask_bytes: 0,
            max_mask_layer_id: 0,
            max_mask_width: 0,
            max_mask_height: 0,
            max_mask_bytes: 0,
        };
        let mut report = [0i8; 128];
        let mut required_report_len = 0usize;
        assert_eq!(
            clip_renderer_session_support_info(
                session,
                &mut support,
                report.as_mut_ptr(),
                report.len(),
                &mut required_report_len,
            ),
            ClipRendererStatus::Ok,
        );
        assert!(support.source_count > 0);
        assert_eq!(support.unsupported_count, 0);
        assert!(support.raster_count > 0);
        assert!(required_report_len > 1);
        let report = unsafe { CStr::from_ptr(report.as_ptr()) }.to_str().unwrap();
        assert!(report.contains("Full native support"));

        clip_renderer_session_close(session);
    }

    #[test]
    fn c_api_opens_session_from_memory_bytes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../img/Test_Clipping.clip");
        let bytes = std::fs::read(path).expect("read Test_Clipping.clip bytes");
        let mut session = ptr::null_mut();

        assert_eq!(
            clip_renderer_session_open_memory(bytes.as_ptr(), bytes.len(), &mut session),
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
        assert_eq!(info.external_data_count, 7);

        let mut support = ClipRendererSupportInfo {
            source_count: 0,
            unsupported_count: 0,
            raster_count: 0,
            raster_bytes: 0,
            max_raster_layer_id: 0,
            max_raster_width: 0,
            max_raster_height: 0,
            max_raster_bytes: 0,
            mask_count: 0,
            mask_bytes: 0,
            max_mask_layer_id: 0,
            max_mask_width: 0,
            max_mask_height: 0,
            max_mask_bytes: 0,
        };
        assert_eq!(
            clip_renderer_session_support_info(
                session,
                &mut support,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
            ),
            ClipRendererStatus::Ok,
        );
        assert_eq!(support.unsupported_count, 0);

        clip_renderer_session_close(session);
    }

    #[test]
    fn c_api_rejects_null_memory_session_arguments() {
        let bytes = [0u8; 8];
        let mut session = ptr::null_mut();

        assert_eq!(
            clip_renderer_session_open_memory(ptr::null(), bytes.len(), &mut session),
            ClipRendererStatus::NullArgument,
        );
        assert_eq!(
            clip_renderer_session_open_memory(bytes.as_ptr(), bytes.len(), ptr::null_mut()),
            ClipRendererStatus::NullArgument,
        );
        assert!(session.is_null());
    }

    #[test]
    fn support_report_lists_all_unsupported_details() {
        use clip_graph::{RenderNodeId, RenderNodeKind};
        use clip_model::LayerId;
        use clip_runtime::{SimpleRasterStackUnsupported, SimpleRasterStackUnsupportedReason};

        let unsupported: Vec<_> = (0..10)
            .map(|index| SimpleRasterStackUnsupported {
                render_node_id: RenderNodeId(index + 4),
                layer_id: LayerId(index + 9),
                kind: RenderNodeKind::Filter,
                reason: SimpleRasterStackUnsupportedReason::Filter,
            })
            .collect();

        let report = support_report(12, &unsupported);

        assert!(report.starts_with("10 unsupported node(s)."));
        assert!(report.contains(
            "- layer 9 node 4 Filter: filter layer is not in the strict raster stack pass"
        ));
        assert!(report.contains(
            "- layer 16 node 11 Filter: filter layer is not in the strict raster stack pass"
        ));
        assert!(report.contains(
            "- layer 17 node 12 Filter: filter layer is not in the strict raster stack pass"
        ));
        assert!(report.contains(
            "- layer 18 node 13 Filter: filter layer is not in the strict raster stack pass"
        ));
        assert!(!report.contains("more unsupported node(s)"));
    }
}
