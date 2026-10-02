//! Intel backend: raw I²C to displays driven by an Intel GPU, through the
//! Intel Graphics Control Library (`ControlLib.dll`, installed with the
//! driver). Layouts follow `igcl_api.h` from intel/drivers.gpu.control-library.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::OnceLock;

use anyhow::{Result, anyhow, bail};
use windows::core::s;

use super::channel::{DisplayTarget, NeedsElevation, RawDdcChannel, RawDdcProvider};
use super::ddcci::Packet;
use super::dylib;

type Handle = *mut c_void;
type Status = u32;

const RESULT_SUCCESS: Status = 0;
const RESULT_ERROR_INSUFFICIENT_PERMISSIONS: Status = 0x4000_0006;
const RESULT_ERROR_UNSUPPORTED_VERSION: Status = 0x4000_0009;
const OPERATION_TYPE_WRITE: u32 = 2;
const DISPLAY_CONFIG_FLAG_DISPLAY_ATTACHED: u32 = 1 << 1;
const I2C_MAX_DATA_SIZE: usize = 0x80;
const AUX_MAX_DATA_SIZE: usize = 132;
const AUX_FLAG_I2C_AUX: u32 = 1 << 1;
const DISPLAY_OUTPUT_TYPE_DISPLAYPORT: u32 = 1;

/// `ctl_init_args_t`.
#[repr(C)]
struct InitArgs {
    size: u32,
    version: u8,
    app_version: u32,
    flags: u32,
    supported_version: u32,
    application_uid: [u8; 16],
}

/// `ctl_device_adapter_properties_t`.
#[repr(C)]
struct AdapterProperties {
    size: u32,
    version: u8,
    /// Points at a LUID on Windows.
    device_id: *mut c_void,
    device_id_size: u32,
    device_type: u32,
    supported_subfunction_flags: u32,
    driver_version: u64,
    firmware_version: [u64; 3],
    pci_vendor_id: u32,
    pci_device_id: u32,
    rev_id: u32,
    num_eus_per_sub_slice: u32,
    num_sub_slices_per_slice: u32,
    num_slices: u32,
    name: [u8; 100],
    graphics_adapter_properties: u32,
    frequency: u32,
    pci_subsys_id: u16,
    pci_subsys_vendor_id: u16,
    adapter_bdf: [u8; 3],
    num_xe_cores: u32,
    reserved: [u8; 108],
}

/// `ctl_os_display_encoder_identifier_t`: a union whose Windows member is
/// the `QueryDisplayConfig` target id.
#[repr(C, align(8))]
struct EncoderId {
    windows_target_id: u32,
    _rest: [u32; 3],
}

/// `ctl_display_timing_t`.
#[repr(C)]
struct Timing {
    size: u32,
    version: u8,
    pixel_clock: u64,
    active_total_blank_sync: [u32; 8],
    refresh_rate: f32,
    signal_standard: u32,
    vic_id: u8,
}

/// `ctl_display_properties_t`.
#[repr(C)]
struct DisplayProperties {
    size: u32,
    version: u8,
    encoder: EncoderId,
    output_type: u32,
    attached_display_mux_type: u32,
    protocol_converter_output: u32,
    supported_spec: [u8; 3],
    supported_output_bpc_flags: u32,
    protocol_converter_type: u32,
    display_config_flags: u32,
    feature_enabled_flags: u32,
    feature_supported_flags: u32,
    advanced_feature_enabled_flags: u32,
    advanced_feature_supported_flags: u32,
    timing: Timing,
    reserved: [u32; 16],
}

/// `ctl_i2c_access_args_t`.
#[repr(C)]
struct I2cAccessArgs {
    size: u32,
    version: u8,
    data_size: u32,
    address: u32,
    op_type: u32,
    offset: u32,
    flags: u32,
    rad: u64,
    data: [u8; I2C_MAX_DATA_SIZE],
}

/// `ctl_aux_access_args_t`.
#[repr(C)]
struct AuxAccessArgs {
    size: u32,
    version: u8,
    op_type: u32,
    flags: u32,
    /// For I²C-over-AUX, the 8-bit I²C address.
    address: u32,
    rad: u64,
    port_id: u32,
    data_size: u32,
    data: [u8; AUX_MAX_DATA_SIZE],
}

/// Zero-initialises one of the plain C structs above.
fn zeroed<T>() -> T {
    // SAFETY: only used for the repr(C) structs in this file, for which all
    // zero bytes (null pointers, zero numbers) is a valid value.
    unsafe { std::mem::zeroed() }
}

struct Api {
    handle: Handle,
    enumerate_devices: unsafe extern "C" fn(Handle, *mut u32, *mut Handle) -> Status,
    device_properties: unsafe extern "C" fn(Handle, *mut AdapterProperties) -> Status,
    enumerate_outputs: unsafe extern "C" fn(Handle, *mut u32, *mut Handle) -> Status,
    display_properties: unsafe extern "C" fn(Handle, *mut DisplayProperties) -> Status,
    i2c_access: unsafe extern "C" fn(Handle, *mut I2cAccessArgs) -> Status,
    aux_access: unsafe extern "C" fn(Handle, *mut AuxAccessArgs) -> Status,
}

// SAFETY: IGCL handles are opaque driver tokens usable from any thread.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

/// The loaded library, or why it is unavailable.
fn api() -> Result<&'static Api> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    match API.get_or_init(|| unsafe { load() }) {
        Ok(api) => Ok(api),
        Err(e) => Err(anyhow!("{e}")),
    }
}

unsafe fn load() -> Result<Api, String> {
    type Init = unsafe extern "C" fn(*mut InitArgs, *mut Handle) -> Status;
    let dll = if cfg!(target_pointer_width = "64") {
        "ControlLib.dll"
    } else {
        "ControlLib32.dll"
    };
    let lib = dylib::load_system_library(dll).map_err(|e| format!("{e:#} (no Intel graphics driver)"))?;
    macro_rules! export {
        ($name:literal) => {
            unsafe { dylib::export(lib, s!($name)) }.ok_or(concat!($name, " missing from ControlLib.dll"))?
        };
    }
    let init: Init = export!("ctlInit");
    let mut args = InitArgs {
        size: size_of::<InitArgs>() as u32,
        app_version: (1 << 16) | 1, // CTL_IMPL_VERSION 1.1
        ..zeroed()
    };
    let mut handle: Handle = null_mut();
    check("ctlInit", unsafe { init(&mut args, &mut handle) }).map_err(|e| e.to_string())?;
    Ok(Api {
        handle,
        enumerate_devices: export!("ctlEnumerateDevices"),
        device_properties: export!("ctlGetDeviceProperties"),
        enumerate_outputs: export!("ctlEnumerateDisplayOutputs"),
        display_properties: export!("ctlGetDisplayProperties"),
        i2c_access: export!("ctlI2CAccess"),
        aux_access: export!("ctlAUXAccess"),
    })
}

/// Turns a `ctl_result_t` into an error naming the call and the result.
fn check(call: &str, status: Status) -> Result<()> {
    if status == RESULT_SUCCESS {
        return Ok(());
    }
    if status == RESULT_ERROR_INSUFFICIENT_PERMISSIONS {
        // Intel only allows I²C/AUX writes from elevated processes.
        return Err(NeedsElevation(format!("{call} failed: {status:#X} (INSUFFICIENT_PERMISSIONS)")).into());
    }
    let name = match status {
        0x4000_0001 => "NOT_INITIALIZED",
        0x4000_0003 => "DEVICE_LOST",
        0x4000_0007 => "NOT_AVAILABLE",
        0x4000_0009 => "UNSUPPORTED_VERSION",
        0x4000_000a => "UNSUPPORTED_FEATURE",
        0x4000_000b => "INVALID_ARGUMENT",
        0x4000_000f => "INVALID_SIZE",
        0x4000_0010 => "UNSUPPORTED_SIZE",
        0x4000_0013 => "DATA_WRITE",
        0x4000_0015 => "NOT_IMPLEMENTED",
        0x4000_0016 => "OS_CALL",
        0x4000_0017 => "KMD_CALL",
        0x4000_001e => "WAIT_TIMEOUT",
        0x4000_0020 => "PLATFORM_NOT_SUPPORTED",
        0x4000_0026 => "LOAD",
        0x4000_0027 => "DEVICE_UNAVAILABLE",
        _ => "?",
    };
    bail!("{call} failed: {status:#X} ({name})")
}

/// IGCL's two-call enumeration: ask for the count, then fill the handles.
fn enumerate(call: &str, f: impl Fn(*mut u32, *mut Handle) -> Status) -> Result<Vec<Handle>> {
    let mut count = 0u32;
    check(call, f(&mut count, null_mut()))?;
    let mut handles = vec![null_mut(); count as usize];
    check(call, f(&mut count, handles.as_mut_ptr()))?;
    handles.truncate(count as usize);
    Ok(handles)
}

impl Api {
    fn adapter_luid(&self, adapter: Handle) -> Result<u64> {
        let mut luid = 0u64;
        let mut props = AdapterProperties {
            size: size_of::<AdapterProperties>() as u32,
            version: 3,
            device_id: &mut luid as *mut u64 as *mut c_void,
            device_id_size: size_of::<u64>() as u32,
            ..zeroed()
        };
        let mut status = unsafe { (self.device_properties)(adapter, &mut props) };
        if status == RESULT_ERROR_UNSUPPORTED_VERSION {
            props.version = 0;
            status = unsafe { (self.device_properties)(adapter, &mut props) };
        }
        check("ctlGetDeviceProperties", status)?;
        // The LUID is {u32 low, i32 high}, i.e. a little-endian u64.
        Ok(luid)
    }

    /// The attached output with Windows target id `target_id`, and how to
    /// reach its DDC/CI endpoint.
    fn find_output(&self, adapter: Handle, target_id: u32) -> Result<Option<(Handle, Bus)>> {
        let outputs = enumerate("ctlEnumerateDisplayOutputs", |count, handles| unsafe {
            (self.enumerate_outputs)(adapter, count, handles)
        })?;
        for output in outputs {
            let mut props = DisplayProperties {
                size: size_of::<DisplayProperties>() as u32,
                ..zeroed()
            };
            check("ctlGetDisplayProperties", unsafe {
                (self.display_properties)(output, &mut props)
            })?;
            let attached = props.display_config_flags & DISPLAY_CONFIG_FLAG_DISPLAY_ATTACHED != 0;
            if attached && props.encoder.windows_target_id == target_id {
                // DisplayPort carries DDC/CI as I²C-over-AUX; HDMI and DVI
                // have real DDC pins.
                let bus = if props.output_type == DISPLAY_OUTPUT_TYPE_DISPLAYPORT {
                    Bus::Aux
                } else {
                    Bus::I2c
                };
                return Ok(Some((output, bus)));
            }
        }
        Ok(None)
    }
}

/// Opens channels for displays on Intel outputs.
pub struct Igcl;

impl RawDdcProvider for Igcl {
    fn name(&self) -> &'static str {
        "igcl"
    }

    fn open(&self, target: &DisplayTarget) -> Result<Box<dyn RawDdcChannel>> {
        let (Some(luid), Some(target_id)) = (target.adapter_luid, target.target_id) else {
            bail!("Windows did not report this display's adapter and target id");
        };
        let api = api()?;
        let adapters = enumerate("ctlEnumerateDevices", |count, handles| unsafe {
            (api.enumerate_devices)(api.handle, count, handles)
        })?;
        let mut seen = Vec::new();
        for adapter in adapters {
            let adapter_luid = api.adapter_luid(adapter)?;
            seen.push(format!("{adapter_luid:#x}"));
            if adapter_luid != luid {
                continue;
            }
            return match api.find_output(adapter, target_id)? {
                Some((output, bus)) => Ok(Box::new(IgclChannel { api, output, bus })),
                None => bail!("Intel adapter {luid:#x} has no attached output with target id {target_id}"),
            };
        }
        // Expected when another vendor's GPU drives this display.
        bail!("display is on adapter {luid:#x}, not an Intel one (Intel adapters: {seen:?})")
    }
}

#[derive(Clone, Copy)]
enum Bus {
    /// `ctlI2CAccess` on the DDC pins.
    I2c,
    /// `ctlAUXAccess` with I²C-over-AUX.
    Aux,
}

struct IgclChannel {
    api: &'static Api,
    output: Handle,
    bus: Bus,
}

// SAFETY: see `Api`; `Monitor` serialises every call on its bus lock.
unsafe impl Send for IgclChannel {}
unsafe impl Sync for IgclChannel {}

impl RawDdcChannel for IgclChannel {
    fn name(&self) -> &'static str {
        match self.bus {
            Bus::I2c => "igcl-i2c",
            Bus::Aux => "igcl-aux",
        }
    }

    // Fails with `NeedsElevation` unless the process is elevated.
    fn write(&self, packet: &Packet) -> Result<()> {
        match self.bus {
            // `offset` goes out right after the I²C address, which is where
            // DDC/CI puts the host source address.
            Bus::I2c => {
                let mut args = I2cAccessArgs {
                    size: size_of::<I2cAccessArgs>() as u32,
                    address: packet.dest as u32,
                    op_type: OPERATION_TYPE_WRITE,
                    offset: packet.source as u32,
                    ..zeroed()
                };
                args.data_size = fill(&mut args.data, &[&packet.body])?;
                check("ctlI2CAccess", unsafe { (self.api.i2c_access)(self.output, &mut args) })
            }
            // No offset field: the source address is the first data byte.
            Bus::Aux => {
                let mut args = AuxAccessArgs {
                    size: size_of::<AuxAccessArgs>() as u32,
                    op_type: OPERATION_TYPE_WRITE,
                    flags: AUX_FLAG_I2C_AUX,
                    address: packet.dest as u32,
                    ..zeroed()
                };
                args.data_size = fill(&mut args.data, &[&[packet.source], &packet.body])?;
                check("ctlAUXAccess", unsafe { (self.api.aux_access)(self.output, &mut args) })
            }
        }
    }
}

/// Copies `parts` into a fixed data array, returning the length used.
fn fill(data: &mut [u8], parts: &[&[u8]]) -> Result<u32> {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    if len > data.len() {
        bail!("DDC/CI packet too long ({len} bytes)");
    }
    let mut at = 0;
    for part in parts {
        data[at..at + part.len()].copy_from_slice(part);
        at += part.len();
    }
    Ok(len as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn layouts_match_igcl_api_h() {
        assert_eq!(size_of::<InitArgs>(), 36);
        assert_eq!(size_of::<AdapterProperties>(), 320);
        assert_eq!(size_of::<Timing>(), 64);
        assert_eq!(size_of::<DisplayProperties>(), 200);
        assert_eq!(std::mem::offset_of!(DisplayProperties, display_config_flags), 48);
        assert_eq!(size_of::<I2cAccessArgs>(), 168);
        assert_eq!(std::mem::offset_of!(I2cAccessArgs, data), 40);
        assert_eq!(size_of::<AuxAccessArgs>(), 176);
        assert_eq!(std::mem::offset_of!(AuxAccessArgs, data), 40);
    }

    #[test]
    fn insufficient_permissions_asks_for_elevation() {
        let err = check("ctlAUXAccess", RESULT_ERROR_INSUFFICIENT_PERMISSIONS).unwrap_err();
        assert!(err.is::<NeedsElevation>());
        assert!(!check("ctlAUXAccess", 0x4000_000f).unwrap_err().is::<NeedsElevation>());
    }

    #[test]
    fn aux_payload_starts_with_the_source_address() {
        let packet = Packet::set_vcp(0x50, 0xF4, 0xD2);
        let mut data = [0u8; AUX_MAX_DATA_SIZE];
        let len = fill(&mut data, &[&[packet.source], &packet.body]).unwrap();
        assert_eq!(&data[..len as usize], [0x50, 0x84, 0x03, 0xF4, 0x00, 0xD2, 0x9F]);
        assert!(fill(&mut [0u8; 2], &[&[1, 2, 3]]).is_err());
    }
}
