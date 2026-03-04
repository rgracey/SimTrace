//! AMS2 shared memory accessor (Windows only).
//!
//! AMS2 exposes the pCars2 shared memory API under the mapping name `$pcars2$`.
//! All field reads use raw byte offsets derived from the pCars2 SDK struct layout.
#![cfg(windows)]
#![allow(dead_code)]

use anyhow::{anyhow, Result};
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
                return Err(anyhow!(
                    "AMS2 shared memory not found — is AMS2 running?"
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

    // ── pCars2 field accessors ─────────────────────────────────────────────
    //
    // Offsets verified against the pCars2 SDK SharedMemory.h:
    //   offset 8     = mGameState (u32)
    //   offset 10524 = mUnfilteredThrottle (f32)
    //   offset 10528 = mUnfilteredBrake    (f32)
    //   offset 10532 = mUnfilteredSteering (f32)  [-1..1 normalised]
    //   offset 10536 = mUnfilteredClutch   (f32)
    //   offset 10916 = mSpeed              (f32, m/s)
    //   offset 10932 = mGearNumGears       (u32, packed nibbles)
    //   offset 10940 = mAntiLockActive     (u8 bool)

    pub unsafe fn game_state(&self) -> u32 {
        self.u32_at(8)
    }
    pub unsafe fn unfiltered_throttle(&self) -> f32 {
        self.f32_at(10524)
    }
    pub unsafe fn unfiltered_brake(&self) -> f32 {
        self.f32_at(10528)
    }
    pub unsafe fn unfiltered_steering(&self) -> f32 {
        self.f32_at(10532)
    }
    pub unsafe fn unfiltered_clutch(&self) -> f32 {
        self.f32_at(10536)
    }
    pub unsafe fn speed(&self) -> f32 {
        self.f32_at(10916)
    }
    /// Lower nibble = gear encoded as gear+1 (0→R, 1→N, 2→1st …)
    pub unsafe fn gear_num_gears(&self) -> u32 {
        self.u32_at(10932)
    }
    pub unsafe fn anti_lock_active(&self) -> bool {
        self.u8_at(10940) != 0
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
