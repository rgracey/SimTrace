//! Windows shared memory accessor for ACC
//!
//! Opens the three ACC memory-mapped files and provides typed read-only references.

use anyhow::{anyhow, Result};
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::{MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ};
use winapi::um::winnt::HANDLE;

use super::mapping::{SPageFileGraphic, SPageFilePhysics, SPageFileStatic};

pub struct AccSharedMemory {
    physics_handle: HANDLE,
    graphics_handle: HANDLE,
    static_handle: HANDLE,
    physics_ptr: *const SPageFilePhysics,
    graphics_ptr: *const SPageFileGraphic,
    static_ptr: *const SPageFileStatic,
}

// The shared memory pages are read-only and valid for the lifetime of AccSharedMemory.
unsafe impl Send for AccSharedMemory {}
unsafe impl Sync for AccSharedMemory {}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn open_mapping(name: &str) -> Option<HANDLE> {
    let wide = to_wide(name);
    let h = OpenFileMappingW(FILE_MAP_READ, 0, wide.as_ptr());
    if h.is_null() { None } else { Some(h) }
}

unsafe fn map_view<T>(handle: HANDLE) -> Option<*const T> {
    let p = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0) as *const T;
    if p.is_null() { None } else { Some(p) }
}

impl AccSharedMemory {
    /// Open all three ACC shared memory pages.
    /// Returns an error if ACC is not running.
    pub fn open() -> Result<Self> {
        unsafe {
            let ph = open_mapping("Local\\acpmf_physics")
                .ok_or_else(|| anyhow!("ACC physics shared memory not found — is ACC running?"))?;

            let gh = open_mapping("Local\\acpmf_graphics").ok_or_else(|| {
                CloseHandle(ph);
                anyhow!("ACC graphics shared memory not found")
            })?;

            let sh = open_mapping("Local\\acpmf_static").ok_or_else(|| {
                CloseHandle(ph);
                CloseHandle(gh);
                anyhow!("ACC static shared memory not found")
            })?;

            let pp = map_view::<SPageFilePhysics>(ph);
            let gp = map_view::<SPageFileGraphic>(gh);
            let sp = map_view::<SPageFileStatic>(sh);

            if pp.is_none() || gp.is_none() || sp.is_none() {
                if let Some(p) = pp { UnmapViewOfFile(p as *mut _); }
                if let Some(p) = gp { UnmapViewOfFile(p as *mut _); }
                if let Some(p) = sp { UnmapViewOfFile(p as *mut _); }
                CloseHandle(ph);
                CloseHandle(gh);
                CloseHandle(sh);
                return Err(anyhow!("Failed to map ACC shared memory views"));
            }

            Ok(Self {
                physics_handle: ph,
                graphics_handle: gh,
                static_handle: sh,
                physics_ptr: pp.unwrap(),
                graphics_ptr: gp.unwrap(),
                static_ptr: sp.unwrap(),
            })
        }
    }

    /// Check if ACC shared memory exists without keeping a connection open.
    pub fn is_available() -> bool {
        unsafe {
            let wide = to_wide("Local\\acpmf_physics");
            let h = OpenFileMappingW(FILE_MAP_READ, 0, wide.as_ptr());
            if h.is_null() {
                false
            } else {
                CloseHandle(h);
                true
            }
        }
    }

    /// Read a snapshot of physics data.
    ///
    /// # Safety
    /// The pointer is valid while this struct is alive and ACC is running.
    pub unsafe fn physics(&self) -> SPageFilePhysics {
        self.physics_ptr.read_volatile()
    }

    /// Read a snapshot of graphics/session data.
    pub unsafe fn graphics(&self) -> SPageFileGraphic {
        self.graphics_ptr.read_volatile()
    }

    /// Read a snapshot of static/session info.
    pub unsafe fn static_info(&self) -> SPageFileStatic {
        self.static_ptr.read_volatile()
    }
}

impl Drop for AccSharedMemory {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.physics_ptr as *mut _);
            UnmapViewOfFile(self.graphics_ptr as *mut _);
            UnmapViewOfFile(self.static_ptr as *mut _);
            CloseHandle(self.physics_handle);
            CloseHandle(self.graphics_handle);
            CloseHandle(self.static_handle);
        }
    }
}
