//! ACC shared memory struct layout (must match ACC SDK exactly)
//!
//! File mapping names: "Local\\acpmf_physics", "Local\\acpmf_graphics", "Local\\acpmf_static"
//! wchar_t → u16 (2 bytes on Windows)

/// Game status from SPageFileGraphic.status
pub mod status {
    pub const OFF: i32 = 0;
    pub const REPLAY: i32 = 1;
    pub const LIVE: i32 = 2;
    pub const PAUSE: i32 = 3;
}

/// Real-time physics data — read every telemetry frame
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SPageFilePhysics {
    pub packet_id: i32,
    pub gas: f32,               // throttle 0.0–1.0
    pub brake: f32,             // brake 0.0–1.0
    pub fuel: f32,
    pub gear: i32,              // 0=R, 1=N, 2=1st … 8=7th
    pub rpms: i32,
    pub steer_angle: f32,       // steering wheel angle in radians
    pub speed_kmh: f32,
    pub velocity: [f32; 3],
    pub acc_g: [f32; 3],
    pub wheel_slip: [f32; 4],
    pub wheel_load: [f32; 4],       // deprecated
    pub wheels_pressure: [f32; 4],
    pub wheel_angular_speed: [f32; 4],
    pub tyre_wear: [f32; 4],        // deprecated
    pub tyre_dirty_level: [f32; 4], // deprecated
    pub tyre_core_temperature: [f32; 4],
    pub camber_rad: [f32; 4],       // deprecated
    pub suspension_travel: [f32; 4],
    pub drs: f32,                   // deprecated
    pub tc: f32,                    // TC actuation 0.0–1.0
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    pub cg_height: f32,             // deprecated
    pub car_damage: [f32; 5],
    pub number_of_tyres_out: i32,   // deprecated
    pub pit_limiter_on: i32,
    pub abs: f32,                   // ABS actuation 0.0–1.0
    pub kers_charge: f32,           // deprecated
    pub kers_input: f32,            // deprecated
    pub auto_shifter_on: i32,
    pub ride_height: [f32; 2],      // deprecated
    pub turbo_boost: f32,
    pub ballast: f32,               // deprecated
    pub air_density: f32,           // deprecated
    pub air_temp: f32,
    pub road_temp: f32,
    pub local_angular_vel: [f32; 3],
    pub final_ff: f32,
    pub performance_meter: f32,     // deprecated
    pub engine_brake: i32,          // deprecated
    pub ers_recovery_level: i32,    // deprecated
    pub ers_power_level: i32,       // deprecated
    pub ers_heat_charging: i32,     // deprecated
    pub ers_is_charging: i32,       // deprecated
    pub kers_current_kj: f32,       // deprecated
    pub drs_available: i32,         // deprecated
    pub drs_enabled: i32,           // deprecated
    pub brake_temp: [f32; 4],
    pub clutch: f32,                // clutch pedal 0.0–1.0
    pub tyre_temp_i: [f32; 4],      // deprecated
    pub tyre_temp_m: [f32; 4],      // deprecated
    pub tyre_temp_o: [f32; 4],      // deprecated
    pub is_ai_controlled: i32,
    pub tyre_contact_point: [[f32; 3]; 4],
    pub tyre_contact_normal: [[f32; 3]; 4],
    pub tyre_contact_heading: [[f32; 3]; 4],
    pub brake_bias: f32,
    pub local_velocity: [f32; 3],
    pub p2p_activations: i32,       // deprecated
    pub p2p_status: i32,            // deprecated
    pub current_max_rpm: i32,
    pub mz: [f32; 4],               // deprecated
    pub fx: [f32; 4],               // deprecated
    pub fy: [f32; 4],               // deprecated
    pub slip_ratio: [f32; 4],
    pub slip_angle: [f32; 4],
    pub tcin_action: i32,           // deprecated
    pub abs_in_action: i32,         // deprecated
    pub suspension_damage: [f32; 4],// deprecated
    pub tyre_temp: [f32; 4],        // deprecated
    pub water_temp: f32,
    pub brake_pressure: [f32; 4],   // deprecated
    pub front_brake_compound: i32,
    pub rear_brake_compound: i32,
    pub pad_life: [f32; 4],         // deprecated
    pub disc_life: [f32; 4],        // deprecated
    pub ignition_on: i32,
    pub starter_engine_on: i32,
    pub is_engine_running: i32,
    pub kerb_vibration: f32,
    pub slip_vibrations: f32,
    pub g_vibrations: f32,
    pub abs_vibrations: f32,
}

/// Session / display data — updated each graphics frame
///
/// Layout notes:
///   offset  0: packet_id (i32)
///   offset  4: status (i32)
///   offset  8: session (i32)
///   offset 12: current_time [u16;15] = 30 bytes  → ends 42
///   offset 42: last_time   [u16;15] = 30 bytes  → ends 72
///   offset 72: best_time   [u16;15] = 30 bytes  → ends 102
///   offset102: split       [u16;15] = 30 bytes  → ends 132  (4-byte aligned ✓)
///   offset132: completed_laps … number_of_laps  (10 × i32/f32 = 40 bytes) → ends 172 (wait, let me recount)
///   completed_laps(4)+position(4)+i_current_time(4)+i_last_time(4)+i_best_time(4)+
///   session_time_left(4)+distance_traveled(4)+is_in_pit(4)+current_sector_index(4)+
///   last_sector_time(4)+number_of_laps(4) = 44 bytes → offset 132+44=176
///   offset176: tyre_compound [u16;33] = 66 bytes → ends 242
///   ** 2-byte padding → offset 244 **
///   offset244: replay_time_multiplier (f32)
///   offset248: normalized_car_position (f32)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SPageFileGraphic {
    pub packet_id: i32,
    pub status: i32,   // see status:: constants
    pub session: i32,
    pub current_time: [u16; 15],
    pub last_time: [u16; 15],
    pub best_time: [u16; 15],
    pub split: [u16; 15],
    pub completed_laps: i32,
    pub position: i32,
    pub i_current_time: i32,
    pub i_last_time: i32,
    pub i_best_time: i32,
    pub session_time_left: f32,
    pub distance_traveled: f32,
    pub is_in_pit: i32,
    pub current_sector_index: i32,
    pub last_sector_time: i32,
    pub number_of_laps: i32,
    pub tyre_compound: [u16; 33],
    // repr(C) inserts 2 bytes padding here to align f32 to 4 bytes
    pub replay_time_multiplier: f32, // deprecated
    pub normalized_car_position: f32,
}

/// Static / per-session info
///
/// Layout:
///   sm_version   [u16;15] → ends 30
///   ac_version   [u16;15] → ends 60   (4-byte aligned ✓)
///   number_of_sessions (i32) at 60
///   num_cars           (i32) at 64
///   car_model   [u16;33] → starts 68, ends 134
///   track       [u16;33] → starts 134, ends 200
///   player_name [u16;33] → starts 200, ends 266
///   player_surname [u16;33] → starts 266, ends 332
///   player_nick [u16;33] → starts 332, ends 398
///   ** 2 bytes padding → 400 **
///   sector_count  (i32) at 400
///   max_torque    (f32) at 404 (deprecated)
///   max_power     (f32) at 408 (deprecated)
///   max_rpm       (i32) at 412
///   max_fuel      (f32) at 416
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SPageFileStatic {
    pub sm_version: [u16; 15],
    pub ac_version: [u16; 15],
    pub number_of_sessions: i32,
    pub num_cars: i32,
    pub car_model: [u16; 33],
    pub track: [u16; 33],
    pub player_name: [u16; 33],
    pub player_surname: [u16; 33],
    pub player_nick: [u16; 33],
    // repr(C) inserts 2 bytes padding here to align i32 to 4 bytes
    pub sector_count: i32,
    pub max_torque: f32,  // deprecated
    pub max_power: f32,   // deprecated
    pub max_rpm: i32,
    pub max_fuel: f32,
}

/// Decode a null-terminated UTF-16 slice to a String
pub fn decode_wstring(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
