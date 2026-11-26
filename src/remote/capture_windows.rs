#![cfg(target_os = "windows")]
#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::mem;
use std::ptr;

use jpeg_encoder::{ColorType, Encoder};

type HRESULT = i32;
type BOOL = i32;
type UINT = u32;
type HWND = *mut core::ffi::c_void;
type HMONITOR = *mut core::ffi::c_void;
type HDC = *mut core::ffi::c_void;
type HBITMAP = *mut core::ffi::c_void;
type HGDIOBJ = *mut core::ffi::c_void;

const S_OK: HRESULT = 0;
const DXGI_ERROR_WAIT_TIMEOUT: HRESULT = -2005270490i32; // 0x887A0027
const DXGI_ERROR_ACCESS_LOST: HRESULT = -2005270496i32; // 0x887A0026
const D3D_DRIVER_TYPE_UNKNOWN: u32 = 0;
const D3D11_SDK_VERSION: u32 = 7;
const D3D_FEATURE_LEVEL_11_0: u32 = 0xb000;
const D3D_FEATURE_LEVEL_10_1: u32 = 0xa100;
const D3D_FEATURE_LEVEL_10_0: u32 = 0xa000;
const D3D11_USAGE_STAGING: u32 = 3;
const D3D11_CPU_ACCESS_READ: u32 = 0x20000;
const D3D11_MAP_READ: u32 = 1;
const SRCCOPY: u32 = 0x00CC0020;
const CAPTUREBLT: u32 = 0x40000000;
const DIB_RGB_COLORS: u32 = 0;
const BI_RGB: u32 = 0;
const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;
const HALFTONE: i32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct POINT {
    x: i32,
    y: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DXGI_SAMPLE_DESC {
    Count: u32,
    Quality: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct D3D11_TEXTURE2D_DESC {
    Width: u32,
    Height: u32,
    MipLevels: u32,
    ArraySize: u32,
    Format: u32,
    SampleDesc: DXGI_SAMPLE_DESC,
    Usage: u32,
    BindFlags: u32,
    CPUAccessFlags: u32,
    MiscFlags: u32,
}
impl Default for D3D11_TEXTURE2D_DESC {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct D3D11_MAPPED_SUBRESOURCE {
    pData: *mut u8,
    RowPitch: u32,
    DepthPitch: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DXGI_OUTDUPL_POINTER_POSITION {
    Position: POINT,
    Visible: BOOL,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DXGI_OUTDUPL_FRAME_INFO {
    LastPresentTime: i64,
    LastMouseUpdateTime: i64,
    AccumulatedFrames: u32,
    RectsCoalesced: BOOL,
    ProtectedContentMaskedOut: BOOL,
    PointerPosition: DXGI_OUTDUPL_POINTER_POSITION,
    TotalMetadataBufferSize: u32,
    PointerShapeBufferSize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DXGI_OUTPUT_DESC {
    DeviceName: [u16; 32],
    DesktopCoordinates: RECT,
    AttachedToDesktop: BOOL,
    Rotation: u32,
    Monitor: HMONITOR,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct GUID {
    Data1: u32,
    Data2: u16,
    Data3: u16,
    Data4: [u8; 8],
}
const IID_IDXGIFACTORY1: GUID = GUID {
    Data1: 0x770aae78,
    Data2: 0xf26f,
    Data3: 0x4dba,
    Data4: [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
};

#[repr(C)]
struct IUnknownVtbl {
    QueryInterface: usize,
    AddRef: usize,
    Release: unsafe extern "system" fn(*mut IUnknownVtbl) -> u32,
}
unsafe fn com_release(p: *mut core::ffi::c_void) {
    if !p.is_null() {
        unsafe {
            let v = *(p as *const *mut IUnknownVtbl);
            ((*v).Release)(v);
        }
    }
}
unsafe fn vt<T: Copy>(obj: *mut core::ffi::c_void, idx: usize) -> T {
    unsafe {
        let vtbl = *(obj as *const *const usize);
        *(&*vtbl.add(idx) as *const usize as *const T)
    }
}

#[link(name = "d3d11")]
unsafe extern "system" {
    fn D3D11CreateDevice(
        pAdapter: *mut core::ffi::c_void,
        DriverType: u32,
        Software: *mut core::ffi::c_void,
        Flags: u32,
        pFeatureLevels: *const u32,
        FeatureLevels: u32,
        SDKVersion: u32,
        ppDevice: *mut *mut core::ffi::c_void,
        pFeatureLevel: *mut u32,
        ppImmediateContext: *mut *mut core::ffi::c_void,
    ) -> HRESULT;
}
#[link(name = "dxgi")]
unsafe extern "system" {
    fn CreateDXGIFactory1(riid: *const GUID, ppFactory: *mut *mut core::ffi::c_void) -> HRESULT;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetDesktopWindow() -> HWND;
    fn GetDC(hWnd: HWND) -> HDC;
    fn ReleaseDC(hWnd: HWND, hDC: HDC) -> i32;
    fn GetSystemMetrics(nIndex: i32) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(hdc: HDC) -> HDC;
    fn DeleteDC(hdc: HDC) -> BOOL;
    fn CreateCompatibleBitmap(hdc: HDC, cx: i32, cy: i32) -> HBITMAP;
    fn SelectObject(hdc: HDC, h: HGDIOBJ) -> HGDIOBJ;
    fn DeleteObject(ho: HGDIOBJ) -> BOOL;
    fn StretchBlt(
        hdcDest: HDC,
        xDest: i32,
        yDest: i32,
        wDest: i32,
        hDest: i32,
        hdcSrc: HDC,
        xSrc: i32,
        ySrc: i32,
        wSrc: i32,
        hSrc: i32,
        rop: u32,
    ) -> BOOL;
    fn SetStretchBltMode(hdc: HDC, mode: i32) -> i32;
    fn SetBrushOrgEx(hdc: HDC, x: i32, y: i32, lppt: *mut core::ffi::c_void) -> BOOL;
    fn GetDIBits(
        hdc: HDC,
        hbm: HBITMAP,
        start: UINT,
        cLines: UINT,
        lpvBits: *mut core::ffi::c_void,
        lpbmi: *mut BITMAPINFO,
        usage: UINT,
    ) -> i32;
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BITMAPINFOHEADER {
    biSize: u32,
    biWidth: i32,
    biHeight: i32,
    biPlanes: u16,
    biBitCount: u16,
    biCompression: u32,
    biSizeImage: u32,
    biXPelsPerMeter: i32,
    biYPelsPerMeter: i32,
    biClrUsed: u32,
    biClrImportant: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BITMAPINFO {
    bmiHeader: BITMAPINFOHEADER,
    bmiColors: [u32; 3],
}

// vtable fn types
type FnEnumAdapters1 =
    unsafe extern "system" fn(*mut core::ffi::c_void, UINT, *mut *mut core::ffi::c_void) -> HRESULT;
type FnEnumOutputs =
    unsafe extern "system" fn(*mut core::ffi::c_void, UINT, *mut *mut core::ffi::c_void) -> HRESULT;
type FnGetDesc =
    unsafe extern "system" fn(*mut core::ffi::c_void, *mut DXGI_OUTPUT_DESC) -> HRESULT;
type FnQI = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *const GUID,
    *mut *mut core::ffi::c_void,
) -> HRESULT;
type FnDuplicateOutput = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *mut core::ffi::c_void,
    *mut *mut core::ffi::c_void,
) -> HRESULT;
type FnAcquireNextFrame = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    UINT,
    *mut DXGI_OUTDUPL_FRAME_INFO,
    *mut *mut core::ffi::c_void,
) -> HRESULT;
type FnGetFrameDirtyRects =
    unsafe extern "system" fn(*mut core::ffi::c_void, UINT, *mut RECT, *mut UINT) -> HRESULT;
type FnReleaseFrame = unsafe extern "system" fn(*mut core::ffi::c_void) -> HRESULT;
type FnCreateTexture2D = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *const D3D11_TEXTURE2D_DESC,
    *const core::ffi::c_void,
    *mut *mut core::ffi::c_void,
) -> HRESULT;
type FnCopyResource = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *mut core::ffi::c_void,
    *mut core::ffi::c_void,
);
type FnMap = unsafe extern "system" fn(
    *mut core::ffi::c_void,
    *mut core::ffi::c_void,
    UINT,
    u32,
    UINT,
    *mut D3D11_MAPPED_SUBRESOURCE,
) -> HRESULT;
type FnUnmap = unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, UINT);
type FnGetDescTex = unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_TEXTURE2D_DESC);

pub struct Capturer {
    device: *mut core::ffi::c_void,
    context: *mut core::ffi::c_void,
    duplication: *mut core::ffi::c_void,
    staging: *mut core::ffi::c_void,
    width: usize,
    height: usize,
    rgb: Vec<u8>,
    prev_rgb: Vec<u8>,
    dirty_tiles: Vec<bool>,
    out: Vec<u8>,
    tile_jpeg: Vec<u8>,
    max_width: usize,
    quality: u8,
    force_full: bool,
    // GDI fallback
    gdi_mem: HDC,
    gdi_bmp: HBITMAP,
    gdi_old: HGDIOBJ,
    gdi_w: usize,
    gdi_h: usize,
    gdi_bgra: Vec<u8>,
    use_gdi: bool,
}
unsafe impl Send for Capturer {}

impl Capturer {
    pub fn new() -> Result<Self, String> {
        let mut c = Self {
            device: ptr::null_mut(),
            context: ptr::null_mut(),
            duplication: ptr::null_mut(),
            staging: ptr::null_mut(),
            width: 0,
            height: 0,
            rgb: Vec::new(),
            prev_rgb: Vec::new(),
            dirty_tiles: Vec::new(),
            out: Vec::new(),
            tile_jpeg: Vec::new(),
            max_width: 1280,
            quality: 45,
            force_full: true,
            gdi_mem: ptr::null_mut(),
            gdi_bmp: ptr::null_mut(),
            gdi_old: ptr::null_mut(),
            gdi_w: 0,
            gdi_h: 0,
            gdi_bgra: Vec::new(),
            use_gdi: false,
        };
        unsafe {
            if c.init_dxgi().is_err() {
                c.use_gdi = true;
            }
        }
        Ok(c)
    }

    pub fn set_max_width(&mut self, max_w: usize) {
        let max_w = max_w.clamp(480, 1920);
        if self.max_width != max_w {
            self.max_width = max_w;
            self.force_full = true;
            self.prev_rgb.clear();
        }
    }
    pub fn set_quality(&mut self, q: u8) {
        let q = q.clamp(20, 90);
        if self.quality != q {
            self.quality = q;
        }
    }

    unsafe fn init_dxgi(&mut self) -> Result<(), String> {
        unsafe {
            let mut factory: *mut core::ffi::c_void = ptr::null_mut();
            let hr = CreateDXGIFactory1(&IID_IDXGIFACTORY1, &mut factory);
            if hr < 0 {
                return Err(format!("CreateDXGIFactory1 {hr:#x}"));
            }

            let enum_adapters: FnEnumAdapters1 = vt(factory, 12);
            let mut adapter: *mut core::ffi::c_void = ptr::null_mut();
            let hr = enum_adapters(factory, 0, &mut adapter);
            com_release(factory);
            if hr < 0 {
                return Err(format!("EnumAdapters1 {hr:#x}"));
            }

            let enum_outputs: FnEnumOutputs = vt(adapter, 7);
            let mut output: *mut core::ffi::c_void = ptr::null_mut();
            let hr = enum_outputs(adapter, 0, &mut output);
            if hr < 0 {
                com_release(adapter);
                return Err(format!("EnumOutputs {hr:#x}"));
            }

            let mut desc = DXGI_OUTPUT_DESC::default();
            let get_desc: FnGetDesc = vt(output, 7);
            let hr = get_desc(output, &mut desc);
            if hr < 0 {
                com_release(output);
                com_release(adapter);
                return Err(format!("GetDesc {hr:#x}"));
            }

            let feature_levels = [
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_10_0,
            ];
            let mut fl: u32 = 0;
            let hr = D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                ptr::null_mut(),
                0,
                feature_levels.as_ptr(),
                feature_levels.len() as u32,
                D3D11_SDK_VERSION,
                &mut self.device,
                &mut fl,
                &mut self.context,
            );
            com_release(adapter);
            if hr < 0 {
                com_release(output);
                return Err(format!("D3D11CreateDevice {hr:#x}"));
            }

            let iid_out1 = GUID {
                Data1: 0x00cddea8,
                Data2: 0x939b,
                Data3: 0x4b83,
                Data4: [0xa3, 0x40, 0xa6, 0x85, 0x22, 0x6b, 0x66, 0x6c],
            };
            let qi: FnQI = vt(output, 0);
            let mut output1: *mut core::ffi::c_void = ptr::null_mut();
            let hr = qi(output, &iid_out1, &mut output1);
            com_release(output);
            if hr < 0 {
                return Err(format!("QI Output1 {hr:#x}"));
            }

            let dup_out: FnDuplicateOutput = vt(output1, 22);
            let hr = dup_out(output1, self.device, &mut self.duplication);
            com_release(output1);
            if hr < 0 {
                return Err(format!("DuplicateOutput {hr:#x}"));
            }

            self.width =
                (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).max(1) as usize;
            self.height =
                (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).max(1) as usize;
            self.force_full = true;
            Ok(())
        }
    }

    unsafe fn release_dxgi(&mut self) {
        unsafe {
            if !self.staging.is_null() {
                com_release(self.staging);
                self.staging = ptr::null_mut();
            }
            if !self.duplication.is_null() {
                com_release(self.duplication);
                self.duplication = ptr::null_mut();
            }
            if !self.context.is_null() {
                com_release(self.context);
                self.context = ptr::null_mut();
            }
            if !self.device.is_null() {
                com_release(self.device);
                self.device = ptr::null_mut();
            }
        }
    }

    pub fn capture_frame(&mut self) -> Result<Option<&[u8]>, String> {
        if self.use_gdi {
            return self.capture_gdi_frame();
        }
        let res = unsafe { self.capture_dxgi_frame() };
        match res {
            Ok(Some(())) => Ok(Some(&self.out)),
            Ok(None) => Ok(None),
            Err(e) if e == "ACCESS_LOST" || e == "NEED_REINIT" => {
                unsafe {
                    self.release_dxgi();
                }
                if unsafe { self.init_dxgi().is_err() } {
                    self.use_gdi = true;
                }
                self.force_full = true;
                Ok(None)
            }
            Err(_) => {
                unsafe {
                    self.release_dxgi();
                }
                self.use_gdi = true;
                self.force_full = true;
                self.capture_gdi_frame()
            }
        }
    }

    unsafe fn capture_dxgi_frame(&mut self) -> Result<Option<()>, String> {
        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut desktop_res: *mut core::ffi::c_void = ptr::null_mut();
            let acquire: FnAcquireNextFrame = vt(self.duplication, 8);
            let hr = acquire(self.duplication, 0, &mut frame_info, &mut desktop_res);
            if hr == DXGI_ERROR_WAIT_TIMEOUT {
                return Ok(None);
            }
            if hr == DXGI_ERROR_ACCESS_LOST {
                if !desktop_res.is_null() {
                    com_release(desktop_res);
                }
                return Err("ACCESS_LOST".into());
            }
            if hr < 0 {
                if !desktop_res.is_null() {
                    com_release(desktop_res);
                }
                return Err(format!("Acquire {hr:#x}"));
            }

            // get texture
            let iid_tex = GUID {
                Data1: 0x6f15aaf2,
                Data2: 0xd208,
                Data3: 0x4e89,
                Data4: [0x9a, 0xb4, 0x48, 0x95, 0x35, 0xd3, 0x4f, 0x9c],
            };
            let qi: FnQI = vt(desktop_res, 0);
            let mut tex: *mut core::ffi::c_void = ptr::null_mut();
            let hr = qi(desktop_res, &iid_tex, &mut tex);
            com_release(desktop_res);
            if hr < 0 {
                let rel: FnReleaseFrame = vt(self.duplication, 14);
                rel(self.duplication);
                return Err(format!("QI tex {hr:#x}"));
            }

            let mut tdesc = D3D11_TEXTURE2D_DESC::default();
            let get_tdesc: FnGetDescTex = vt(tex, 10);
            get_tdesc(tex, &mut tdesc);
            let sw = tdesc.Width as usize;
            let sh = tdesc.Height as usize;

            if self.staging.is_null() || self.width != sw || self.height != sh {
                if !self.staging.is_null() {
                    com_release(self.staging);
                    self.staging = ptr::null_mut();
                }
                let mut sdesc = tdesc;
                sdesc.Usage = D3D11_USAGE_STAGING;
                sdesc.BindFlags = 0;
                sdesc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
                sdesc.MiscFlags = 0;
                let create_tex: FnCreateTexture2D = vt(self.device, 5);
                let hr = create_tex(self.device, &sdesc, ptr::null(), &mut self.staging);
                if hr < 0 {
                    com_release(tex);
                    let rel: FnReleaseFrame = vt(self.duplication, 14);
                    rel(self.duplication);
                    return Err(format!("CreateTexture2D {hr:#x}"));
                }
                self.width = sw;
                self.height = sh;
                self.force_full = true;
            }

            let copy: FnCopyResource = vt(self.context, 47);
            copy(self.context, self.staging, tex);
            com_release(tex);

            let dirty = self.get_dirty_rects(frame_info.TotalMetadataBufferSize)?;
            let rel: FnReleaseFrame = vt(self.duplication, 14);
            rel(self.duplication);

            // If no dirty rects and not forced, skip
            if dirty.is_empty() && !self.force_full {
                return Ok(None);
            }

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            let map_fn: FnMap = vt(self.context, 14);
            let hr = map_fn(
                self.context,
                self.staging,
                0,
                D3D11_MAP_READ,
                0,
                &mut mapped,
            );
            if hr < 0 {
                return Err(format!("Map {hr:#x}"));
            }

            let (tw, th) = target_size_even(sw, sh, self.max_width);
            let needed_rgb = tw * th * 3;
            if self.rgb.len() != needed_rgb {
                self.rgb.resize(needed_rgb, 0);
                self.force_full = true;
            }
            if self.prev_rgb.len() != needed_rgb {
                self.prev_rgb.resize(needed_rgb, 0);
                self.force_full = true;
            }

            let src = mapped.pData;
            let pitch = mapped.RowPitch as usize;
            for y in 0..th {
                let sy = y * sh / th;
                let row = src.add(sy * pitch);
                let dst_off = y * tw * 3;
                for x in 0..tw {
                    let sx = x * sw / tw;
                    let si = sx * 4;
                    let di = dst_off + x * 3;
                    self.rgb[di] = *row.add(si + 2);
                    self.rgb[di + 1] = *row.add(si + 1);
                    self.rgb[di + 2] = *row.add(si);
                }
            }
            let unmap_fn: FnUnmap = vt(self.context, 15);
            unmap_fn(self.context, self.staging, 0);

            self.build_jpeg_delta(tw, th, sw, sh, &dirty)
        }
    }

    unsafe fn get_dirty_rects(&mut self, meta_size: u32) -> Result<Vec<RECT>, String> {
        unsafe {
            if meta_size == 0 {
                return Ok(Vec::new());
            }
            let mut buf: Vec<u8> = vec![0u8; meta_size as usize];
            let mut size_needed: UINT = 0;
            let get_dirty: FnGetFrameDirtyRects = vt(self.duplication, 9);
            let hr = get_dirty(
                self.duplication,
                meta_size,
                buf.as_mut_ptr() as *mut RECT,
                &mut size_needed,
            );
            if hr < 0 {
                return Ok(Vec::new());
            }
            let count = (size_needed as usize) / mem::size_of::<RECT>();
            Ok(std::slice::from_raw_parts(buf.as_ptr() as *const RECT, count).to_vec())
        }
    }

    fn build_jpeg_delta(
        &mut self,
        tw: usize,
        th: usize,
        sw: usize,
        sh: usize,
        dirty_rects: &[RECT],
    ) -> Result<Option<()>, String> {
        let tile: usize = 64;
        let tiles_x = (tw + tile - 1) / tile;
        let tiles_y = (th + tile - 1) / tile;
        let total_tiles = tiles_x * tiles_y;
        if self.dirty_tiles.len() != total_tiles {
            self.dirty_tiles.resize(total_tiles, false);
        }
        self.dirty_tiles.fill(false);

        let use_content_compare = self.use_gdi || self.force_full || dirty_rects.is_empty();
        if use_content_compare {
            let force = self.force_full || self.prev_rgb.len() != self.rgb.len();
            for ty in 0..tiles_y {
                for tx in 0..tiles_x {
                    let x = tx * tile;
                    let y = ty * tile;
                    let tw_t = (tw - x).min(tile);
                    let th_t = (th - y).min(tile);
                    let changed =
                        force || tile_changed_rgb(&self.prev_rgb, &self.rgb, tw, x, y, tw_t, th_t);
                    if changed {
                        self.dirty_tiles[ty * tiles_x + tx] = true;
                    }
                }
            }
        } else {
            for r in dirty_rects {
                let l = ((r.left.max(0) as usize * tw) / sw.max(1)).min(tw.saturating_sub(1));
                let t = ((r.top.max(0) as usize * th) / sh.max(1)).min(th.saturating_sub(1));
                let ri = ((r.right.max(0) as usize * tw + sw - 1) / sw.max(1)).min(tw);
                let bo = ((r.bottom.max(0) as usize * th + sh - 1) / sh.max(1)).min(th);
                let tx0 = l / tile;
                let ty0 = t / tile;
                let tx1 = ri.saturating_sub(1) / tile;
                let ty1 = bo.saturating_sub(1) / tile;
                for ty in ty0..=ty1.min(tiles_y - 1) {
                    for tx in tx0..=tx1.min(tiles_x - 1) {
                        self.dirty_tiles[ty * tiles_x + tx] = true;
                    }
                }
            }
            // For dirty rect regions, still ensure tiles actually differ at target resolution
            for i in 0..total_tiles {
                if self.dirty_tiles[i] {
                    let tx = i % tiles_x;
                    let ty = i / tiles_x;
                    let x = tx * tile;
                    let y = ty * tile;
                    let tw_t = (tw - x).min(tile);
                    let th_t = (th - y).min(tile);
                    if !tile_changed_rgb(&self.prev_rgb, &self.rgb, tw, x, y, tw_t, th_t)
                        && !self.force_full
                    {
                        self.dirty_tiles[i] = false;
                    }
                }
            }
        }

        self.out.clear();
        self.out.reserve(13 + 5 + total_tiles / 4 * 512);
        self.out.extend_from_slice(b"NRD1");
        self.out.extend_from_slice(&(tw as u16).to_le_bytes());
        self.out.extend_from_slice(&(th as u16).to_le_bytes());
        self.out.push(4u8);
        let plen_pos = self.out.len();
        self.out.extend_from_slice(&0u32.to_le_bytes());
        let payload_start = self.out.len();
        self.out.push(4u8);
        self.out.extend_from_slice(&(tile as u16).to_le_bytes());
        let count_pos = self.out.len();
        self.out.extend_from_slice(&0u16.to_le_bytes());

        let mut dirty_count: u16 = 0;
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                if !self.dirty_tiles[ty * tiles_x + tx] {
                    continue;
                }
                let x = tx * tile;
                let y = ty * tile;
                let tw_t = (tw - x).min(tile);
                let th_t = (th - y).min(tile);
                self.encode_tile_jpeg(tw, x, y, tw_t, th_t);
                self.out.extend_from_slice(&(x as u16).to_le_bytes());
                self.out.extend_from_slice(&(y as u16).to_le_bytes());
                self.out.extend_from_slice(&(tw_t as u16).to_le_bytes());
                self.out.extend_from_slice(&(th_t as u16).to_le_bytes());
                self.out
                    .extend_from_slice(&(self.tile_jpeg.len() as u32).to_le_bytes());
                self.out.extend_from_slice(&self.tile_jpeg);
                dirty_count = dirty_count.saturating_add(1);
            }
        }

        if dirty_count == 0 {
            return Ok(None);
        }

        self.prev_rgb.copy_from_slice(&self.rgb);
        self.force_full = false;
        self.out[count_pos..count_pos + 2].copy_from_slice(&dirty_count.to_le_bytes());
        let payload_len = (self.out.len() - payload_start) as u32;
        self.out[plen_pos..plen_pos + 4].copy_from_slice(&payload_len.to_le_bytes());
        Ok(Some(()))
    }

    fn encode_tile_jpeg(&mut self, stride_w: usize, x: usize, y: usize, w: usize, h: usize) {
        self.tile_jpeg.clear();
        let mut tile_rgb = Vec::with_capacity(w * h * 3);
        for row in 0..h {
            let start = ((y + row) * stride_w + x) * 3;
            tile_rgb.extend_from_slice(&self.rgb[start..start + w * 3]);
        }
        let enc = Encoder::new(&mut self.tile_jpeg, self.quality);
        let _ = enc.encode(&tile_rgb, w as u16, h as u16, ColorType::Rgb);
    }

    fn capture_gdi_frame(&mut self) -> Result<Option<&[u8]>, String> {
        let (sw, sh) = screen_size()?;
        let (tw, th) = target_size_even(sw, sh, self.max_width);
        self.capture_gdi_rgb(sw, sh, tw, th)?;
        match self.build_jpeg_delta(tw, th, sw, sh, &[])? {
            Some(()) => Ok(Some(&self.out)),
            None => Ok(None),
        }
    }

    fn capture_gdi_rgb(
        &mut self,
        sw: usize,
        sh: usize,
        tw: usize,
        th: usize,
    ) -> Result<(), String> {
        unsafe {
            let hwnd = GetDesktopWindow();
            let screen_dc = GetDC(hwnd);
            if screen_dc.is_null() {
                return Err("GetDC failed".into());
            }
            if self.gdi_mem.is_null() {
                self.gdi_mem = CreateCompatibleDC(screen_dc);
                if self.gdi_mem.is_null() {
                    ReleaseDC(hwnd, screen_dc);
                    return Err("CreateCompatibleDC failed".into());
                }
            }
            if self.gdi_w != tw || self.gdi_h != th || self.gdi_bmp.is_null() {
                if !self.gdi_bmp.is_null() {
                    if !self.gdi_old.is_null() {
                        SelectObject(self.gdi_mem, self.gdi_old);
                    }
                    DeleteObject(self.gdi_bmp as HGDIOBJ);
                    self.gdi_bmp = ptr::null_mut();
                    self.gdi_old = ptr::null_mut();
                }
                self.gdi_bmp = CreateCompatibleBitmap(screen_dc, tw as i32, th as i32);
                if self.gdi_bmp.is_null() {
                    ReleaseDC(hwnd, screen_dc);
                    return Err("CreateCompatibleBitmap failed".into());
                }
                self.gdi_old = SelectObject(self.gdi_mem, self.gdi_bmp as HGDIOBJ);
                self.gdi_w = tw;
                self.gdi_h = th;
                self.force_full = true;
            }
            SetStretchBltMode(self.gdi_mem, HALFTONE);
            SetBrushOrgEx(self.gdi_mem, 0, 0, ptr::null_mut());
            let ok = StretchBlt(
                self.gdi_mem,
                0,
                0,
                tw as i32,
                th as i32,
                screen_dc,
                0,
                0,
                sw as i32,
                sh as i32,
                SRCCOPY | CAPTUREBLT,
            );
            if ok == 0 {
                ReleaseDC(hwnd, screen_dc);
                return Err("StretchBlt failed".into());
            }
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: tw as i32,
                    biHeight: -(th as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let needed_bgra = tw * th * 4;
            if self.gdi_bgra.len() != needed_bgra {
                self.gdi_bgra.resize(needed_bgra, 0);
            }
            let lines = GetDIBits(
                self.gdi_mem,
                self.gdi_bmp,
                0,
                th as UINT,
                self.gdi_bgra.as_mut_ptr() as *mut core::ffi::c_void,
                &mut bmi,
                DIB_RGB_COLORS,
            );
            ReleaseDC(hwnd, screen_dc);
            if lines == 0 {
                return Err("GetDIBits failed".into());
            }
            let needed_rgb = tw * th * 3;
            if self.rgb.len() != needed_rgb {
                self.rgb.resize(needed_rgb, 0);
                self.force_full = true;
            }
            if self.prev_rgb.len() != needed_rgb {
                self.prev_rgb.resize(needed_rgb, 0);
                self.force_full = true;
            }
            let mut si = 0;
            let mut di = 0;
            while si < needed_bgra {
                self.rgb[di] = self.gdi_bgra[si + 2];
                self.rgb[di + 1] = self.gdi_bgra[si + 1];
                self.rgb[di + 2] = self.gdi_bgra[si];
                si += 4;
                di += 3;
            }
            Ok(())
        }
    }
}

impl Drop for Capturer {
    fn drop(&mut self) {
        unsafe {
            self.release_dxgi();
            if !self.gdi_mem.is_null() {
                if !self.gdi_old.is_null() {
                    SelectObject(self.gdi_mem, self.gdi_old);
                }
                if !self.gdi_bmp.is_null() {
                    DeleteObject(self.gdi_bmp as HGDIOBJ);
                }
                DeleteDC(self.gdi_mem);
            }
        }
    }
}

#[inline(always)]
fn tile_changed_rgb(
    prev: &[u8],
    curr: &[u8],
    stride_w: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) -> bool {
    let row_bytes = w * 3;
    for row in 0..h {
        let start = ((y + row) * stride_w + x) * 3;
        let end = start + row_bytes;
        if prev[start..end] != curr[start..end] {
            return true;
        }
    }
    false
}

fn screen_size() -> Result<(usize, usize), String> {
    unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        if w <= 0 || h <= 0 {
            return Err("invalid screen size".into());
        }
        Ok((w as usize, h as usize))
    }
}
fn target_size_even(sw: usize, sh: usize, max_w: usize) -> (usize, usize) {
    let mut tw = if sw <= max_w { sw } else { max_w };
    let mut th = ((sh as u64 * tw as u64) / sw as u64).max(2) as usize;
    if tw % 2 != 0 {
        tw += 1;
    }
    if th % 2 != 0 {
        th += 1;
    }
    (tw, th)
}
