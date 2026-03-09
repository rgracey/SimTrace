//! iRacing shared memory accessor (Windows only).
//!
//! iRacing exposes telemetry via a Win32 file mapping named `Local\IRSDKMemMapFileName`.
//! Unlike pCars2/AMS2, iRacing uses a dynamic variable header system: variable offsets
//! within the data buffer are not fixed and must be resolved by scanning the var headers.
//!
//! Reference: iRacing SDK (irsdk_defines.h / irsdk.h)
#![cfg(windows)]
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::{MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ};
use winapi::um::winnt::HANDLE;

// ── irsdk_header layout ────────────────────────────────────────────────────
//
//   int ver;                  // offset 0
//   int status;               // offset 4   ← irsdk_stConnected = 1
//   int tickRate;             // offset 8
//   int sessionInfoUpdate;    // offset 12
//   int sessionInfoLen;       // offset 16
//   int sessionInfoOffset;    // offset 20
//   int numVars;              // offset 24
//   int varHeaderOffset;      // offset 28
//   int numBuf;               // offset 32
//   int bufLen;               // offset 36
//   int pad1[2];              // offset 40
//   irsdk_varBuf varBuf[4];   // offset 48  (each 16 bytes)
//
// irsdk_varBuf:
//   int tickCount;   // +0
//   int bufOffset;   // +4  ← byte offset from start of mapping for this buffer
//   int pad[2];      // +8

const HDR_STATUS: usize = 4;
const HDR_NUM_VARS: usize = 24;
const HDR_VAR_HEADER_OFFSET: usize = 28;
const HDR_NUM_BUF: usize = 32;
const HDR_VAR_BUF_BASE: usize = 48;
const VAR_BUF_STRIDE: usize = 16;

const STATUS_CONNECTED: i32 = 1;

// ── irsdk_varHeader layout (144 bytes each) ────────────────────────────────
//
//   int  type;           // +0
//   int  offset;         // +4  ← byte offset within a data buffer row
//   int  count;          // +8
//   bool countAsTime;    // +12
//   char pad[3];         // +13
//   char name[32];       // +16
//   char desc[64];       // +48
//   char unit[32];       // +112

const VAR_HDR_STRIDE: usize = 144;
const VAR_HDR_OFFSET_FIELD: usize = 4;
const VAR_HDR_NAME_FIELD: usize = 16;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct IracingSharedMemory {
    handle: HANDLE,
    ptr: *const u8,
    num_buf: i32,
    // Cached byte offsets within a data buffer for each variable we care about.
    // None means the variable was not present in the var header list.
    pub(super) throttle_off: Option<i32>,
    pub(super) brake_off: Option<i32>,
    /// iRacing Clutch: 0 = pedal released (clutch engaged), 1 = pedal fully pressed (disengaged)
    pub(super) clutch_off: Option<i32>,
    /// SteeringWheelAngle in radians. Positive = counter-clockwise (left).
    pub(super) steering_off: Option<i32>,
    /// Speed in m/s
    pub(super) speed_off: Option<i32>,
    /// Gear: -1 = reverse, 0 = neutral, 1+ = forward gears
    pub(super) gear_off: Option<i32>,
    /// Engine RPM
    pub(super) rpm_off: Option<i32>,
    /// ABS currently active (bool, 1 byte)
    pub(super) abs_off: Option<i32>,
    /// Lap distance percentage 0.0–1.0
    pub(super) track_pos_off: Option<i32>,
    /// Car yaw angle in radians (positive = left / CCW)
    pub(super) yaw_off: Option<i32>,
}

unsafe impl Send for IracingSharedMemory {}
unsafe impl Sync for IracingSharedMemory {}

impl IracingSharedMemory {
    const MAPPING_NAME: &'static str = "Local\\IRSDKMemMapFileName";

    pub fn open() -> Result<Self> {
        unsafe {
            let wide = to_wide(Self::MAPPING_NAME);
            let handle = OpenFileMappingW(FILE_MAP_READ, 0, wide.as_ptr());
            if handle.is_null() {
                let err = GetLastError();
                return Err(anyhow!(
                    "iRacing shared memory not found (Windows error {err}) — is iRacing running?"
                ));
            }

            let ptr = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0) as *const u8;
            if ptr.is_null() {
                CloseHandle(handle);
                return Err(anyhow!("Failed to map iRacing shared memory view"));
            }

            let status = (ptr.add(HDR_STATUS) as *const i32).read_volatile();
            if status & STATUS_CONNECTED == 0 {
                UnmapViewOfFile(ptr as *mut _);
                CloseHandle(handle);
                return Err(anyhow!(
                    "iRacing is running but not in an active session (status={})",
                    status
                ));
            }

            let num_vars = (ptr.add(HDR_NUM_VARS) as *const i32).read_volatile();
            let var_hdr_off =
                (ptr.add(HDR_VAR_HEADER_OFFSET) as *const i32).read_volatile() as usize;
            let num_buf = (ptr.add(HDR_NUM_BUF) as *const i32).read_volatile();

            // Scan var headers, cache offsets for the fields we need.
            let mut throttle_off = None;
            let mut brake_off = None;
            let mut clutch_off = None;
            let mut steering_off = None;
            let mut speed_off = None;
            let mut gear_off = None;
            let mut rpm_off = None;
            let mut abs_off = None;
            let mut track_pos_off = None;
            let mut yaw_off = None;

            for i in 0..num_vars as usize {
                let hdr = ptr.add(var_hdr_off + i * VAR_HDR_STRIDE);
                let off = (hdr.add(VAR_HDR_OFFSET_FIELD) as *const i32).read_volatile();

                // Read null-terminated name (max 32 bytes)
                let name_start = hdr.add(VAR_HDR_NAME_FIELD);
                let name_bytes: Vec<u8> = (0..32)
                    .map(|j| name_start.add(j).read())
                    .take_while(|&b| b != 0)
                    .collect();
                let name = String::from_utf8_lossy(&name_bytes);

                match name.as_ref() {
                    "Throttle" => throttle_off = Some(off),
                    "Brake" => brake_off = Some(off),
                    "Clutch" => clutch_off = Some(off),
                    "SteeringWheelAngle" => steering_off = Some(off),
                    "Speed" => speed_off = Some(off),
                    "Gear" => gear_off = Some(off),
                    "RPM" => rpm_off = Some(off),
                    "ABSactive" => abs_off = Some(off),
                    "LapDistPct" => track_pos_off = Some(off),
                    "Yaw" => yaw_off = Some(off),
                    _ => {}
                }
            }

            Ok(Self {
                handle,
                ptr,
                num_buf,
                throttle_off,
                brake_off,
                clutch_off,
                steering_off,
                speed_off,
                gear_off,
                rpm_off,
                abs_off,
                track_pos_off,
                yaw_off,
            })
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

    pub unsafe fn is_connected(&self) -> bool {
        let status = (self.ptr.add(HDR_STATUS) as *const i32).read_volatile();
        status & STATUS_CONNECTED != 0
    }

    /// Return the byte offset (from the start of the mapping) of the most recent data buffer.
    pub unsafe fn current_buf_offset(&self) -> usize {
        let mut best_tick = i32::MIN;
        let mut best_off = 0usize;
        for i in 0..self.num_buf as usize {
            let base = HDR_VAR_BUF_BASE + i * VAR_BUF_STRIDE;
            let tick = (self.ptr.add(base) as *const i32).read_volatile();
            let buf_off = (self.ptr.add(base + 4) as *const i32).read_volatile() as usize;
            if tick > best_tick {
                best_tick = tick;
                best_off = buf_off;
            }
        }
        best_off
    }

    // ── Typed readers ──────────────────────────────────────────────────────

    pub unsafe fn f32_at(&self, buf_off: usize, var_off: i32) -> f32 {
        (self.ptr.add(buf_off + var_off as usize) as *const f32).read_volatile()
    }

    pub unsafe fn i32_at(&self, buf_off: usize, var_off: i32) -> i32 {
        (self.ptr.add(buf_off + var_off as usize) as *const i32).read_volatile()
    }

    pub unsafe fn bool_at(&self, buf_off: usize, var_off: i32) -> bool {
        self.ptr.add(buf_off + var_off as usize).read_volatile() != 0
    }

    // ── Named field accessors (return defaults when var not present) ────────

    pub unsafe fn throttle(&self, buf: usize) -> f32 {
        self.throttle_off.map_or(0.0, |o| self.f32_at(buf, o))
    }

    pub unsafe fn brake(&self, buf: usize) -> f32 {
        self.brake_off.map_or(0.0, |o| self.f32_at(buf, o))
    }

    /// Raw iRacing clutch: 0 = pedal pressed (disengaged), 1 = pedal released (engaged).
    /// Invert before use if your model wants 1 = pedal fully pressed.
    pub unsafe fn clutch_raw(&self, buf: usize) -> f32 {
        self.clutch_off.map_or(0.0, |o| self.f32_at(buf, o))
    }

    /// Steering wheel angle in radians. Positive = counter-clockwise (left).
    pub unsafe fn steering_wheel_angle_rad(&self, buf: usize) -> f32 {
        self.steering_off.map_or(0.0, |o| self.f32_at(buf, o))
    }

    /// Speed in m/s.
    pub unsafe fn speed(&self, buf: usize) -> f32 {
        self.speed_off.map_or(0.0, |o| self.f32_at(buf, o))
    }

    pub unsafe fn gear(&self, buf: usize) -> i32 {
        self.gear_off.map_or(0, |o| self.i32_at(buf, o))
    }

    pub unsafe fn rpm(&self, buf: usize) -> f32 {
        self.rpm_off.map_or(0.0, |o| self.f32_at(buf, o))
    }

    pub unsafe fn abs_active(&self, buf: usize) -> bool {
        self.abs_off.map_or(false, |o| self.bool_at(buf, o))
    }

    pub unsafe fn lap_dist_pct(&self, buf: usize) -> f32 {
        self.track_pos_off.map_or(0.0, |o| self.f32_at(buf, o))
    }

    /// Car yaw in radians (iRacing "Yaw", positive = CCW / left).
    /// Returns 0.0 if the variable is not present.
    pub unsafe fn yaw(&self, buf: usize) -> f32 {
        self.yaw_off.map_or(0.0, |o| self.f32_at(buf, o))
    }
}

impl Drop for IracingSharedMemory {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.ptr as *mut _);
            CloseHandle(self.handle);
        }
    }
}
