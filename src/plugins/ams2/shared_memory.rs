//! AMS2 shared memory accessor (Windows only).
//!
//! AMS2 exposes the pCars2 shared memory API under the mapping name `$pcars2$`.
//! All field reads use raw byte offsets derived from the pCars2 SDK struct layout.
#![cfg(windows)]
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::{MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ};
use winapi::um::winnt::HANDLE;

pub struct Ams2SharedMemory {
    handle: HANDLE,
    ptr: *const u8,
}

unsafe impl Send for Ams2SharedMemory {}
unsafe impl Sync for Ams2SharedMemory {}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Ams2SharedMemory {
    const MAPPING_NAME: &'static str = "$pcars2$";

    pub fn open() -> Result<Self> {
        unsafe {
            let wide = to_wide(Self::MAPPING_NAME);
            let h = OpenFileMappingW(FILE_MAP_READ, 0, wide.as_ptr());
            if h.is_null() {
                let err = GetLastError();
                return Err(anyhow!(
                    "AMS2 shared memory not found (Windows error {err}) — is AMS2 running with Shared Memory enabled in Options → Gameplay?"
                ));
            }
            let ptr = MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0) as *const u8;
            if ptr.is_null() {
                CloseHandle(h);
                return Err(anyhow!("Failed to map AMS2 shared memory view"));
            }
            Ok(Self { handle: h, ptr })
        }
    }

    pub fn is_available() -> bool {
        unsafe {
            let wide = to_wide(Self::MAPPING_NAME);
            let h = OpenFileMappingW(FILE_MAP_READ, 0, wide.as_ptr());
            if h.is_null() {
                false
            } else {
                CloseHandle(h);
                true
            }
        }
    }

    // ── Typed field readers ────────────────────────────────────────────────

    unsafe fn u32_at(&self, offset: usize) -> u32 {
        (self.ptr.add(offset) as *const u32).read_volatile()
    }

    unsafe fn f32_at(&self, offset: usize) -> f32 {
        (self.ptr.add(offset) as *const f32).read_volatile()
    }

    unsafe fn u8_at(&self, offset: usize) -> u8 {
        self.ptr.add(offset).read_volatile()
    }

    // ── AMS2 field accessors ───────────────────────────────────────────────
    //
    // Offsets computed from SharedMemory.h (MSVC default pack=8, effective pack=4):
    //
    //   ParticipantInfo is 100 bytes:
    //     bool mIsActive(1) + char mName[64](64) + pad(3) + float[3](12) +
    //     float(4) + uint(4) + uint(4) + uint(4) + int(4) = 100 bytes
    //
    //   Main struct header before participant array: 28 bytes
    //   Participant array: 64 × 100 = 6400 bytes → ends at offset 6428
    //
    //   offset    8 = mGameState           (u32)
    //   offset 6428 = mUnfilteredThrottle  (f32)
    //   offset 6432 = mUnfilteredBrake     (f32)
    //   offset 6436 = mUnfilteredSteering  (f32)  [-1..1 normalised]
    //   offset 6440 = mUnfilteredClutch    (f32)
    //   offset 6848 = mSpeed               (f32, m/s)
    //   offset 6876 = mGear                (i32, -1=Reverse 0=Neutral 1=1st …)
    //   offset 6888 = mAntiLockActive      (u8 bool)

    pub unsafe fn game_state(&self) -> u32 {
        self.u32_at(8)
    }
    pub unsafe fn unfiltered_throttle(&self) -> f32 {
        self.f32_at(6428)
    }
    pub unsafe fn unfiltered_brake(&self) -> f32 {
        self.f32_at(6432)
    }
    pub unsafe fn unfiltered_steering(&self) -> f32 {
        self.f32_at(6436)
    }
    pub unsafe fn unfiltered_clutch(&self) -> f32 {
        self.f32_at(6440)
    }
    pub unsafe fn speed(&self) -> f32 {
        self.f32_at(6848)
    }
    /// mGear: -1 = Reverse, 0 = Neutral, 1 = 1st gear, etc.
    pub unsafe fn gear(&self) -> i32 {
        (self.ptr.add(6876) as *const i32).read_volatile()
    }
    pub unsafe fn anti_lock_active(&self) -> bool {
        self.u8_at(6888) != 0
    }
}

impl Drop for Ams2SharedMemory {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.ptr as *mut _);
            CloseHandle(self.handle);
        }
    }
}
