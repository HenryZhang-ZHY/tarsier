//! NVIDIA backend: raw I²C to displays driven by an NVIDIA GPU, through the
//! `nvapi64.dll` the driver installs. Loaded at runtime, so machines without
//! an NVIDIA driver simply get no channel.

use std::ffi::{CString, c_char, c_void};
use std::ptr::null_mut;
use std::sync::OnceLock;

use anyhow::{Result, bail};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{s, w};

use super::channel::{RawDdcChannel, RawDdcProvider};
use super::ddcci::Packet;

type Handle = *mut c_void;
type Status = i32;

// Function ids for `nvapi_QueryInterface`, from the public NVAPI headers.
const ID_INITIALIZE: u32 = 0x0150_E828;
const ID_GET_ASSOCIATED_NVIDIA_DISPLAY_HANDLE: u32 = 0x35C2_9134;
const ID_GET_PHYSICAL_GPUS_FROM_DISPLAY: u32 = 0x34EF_9506;
const ID_GET_ASSOCIATED_DISPLAY_OUTPUT_ID: u32 = 0xD995_937E;
const ID_I2C_WRITE: u32 = 0xE812_EB07;

const MAX_PHYSICAL_GPUS: usize = 64;

/// `NV_I2C_INFO_V3`.
#[repr(C)]
struct I2cInfo {
    version: u32,
    display_mask: u32,
    is_ddc_port: u8,
    /// 8-bit address, i.e. the 7-bit address shifted left.
    i2c_dev_address: u8,
    reg_address: *const u8,
    reg_address_size: u32,
    data: *const u8,
    size: u32,
    /// Deprecated since V2; must be `NVAPI_I2C_SPEED_DEPRECATED`.
    i2c_speed: u32,
    /// `NV_I2C_SPEED`; 0 is the driver default.
    i2c_speed_khz: u32,
    port_id: u8,
    is_port_id_set: u32,
}

const I2C_INFO_VER3: u32 = size_of::<I2cInfo>() as u32 | (3 << 16);
const I2C_SPEED_DEPRECATED: u32 = 0xFFFF;

struct Api {
    get_display: unsafe extern "C" fn(*const c_char, *mut Handle) -> Status,
    gpus_from_display: unsafe extern "C" fn(Handle, *mut Handle, *mut u32) -> Status,
    output_id: unsafe extern "C" fn(Handle, *mut u32) -> Status,
    i2c_write: unsafe extern "C" fn(Handle, *mut I2cInfo) -> Status,
}

/// The loaded driver API, or `None` without an NVIDIA driver.
fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        let api = unsafe { load() };
        if api.is_none() {
            log::info!("NVAPI not available");
        }
        api
    })
    .as_ref()
}

unsafe fn load() -> Option<Api> {
    unsafe {
        // Never freed: the function pointers live for the whole process.
        let lib = if cfg!(target_pointer_width = "64") {
            LoadLibraryW(w!("nvapi64.dll"))
        } else {
            LoadLibraryW(w!("nvapi.dll"))
        }
        .ok()?;
        let query: unsafe extern "C" fn(u32) -> *mut c_void =
            std::mem::transmute(GetProcAddress(lib, s!("nvapi_QueryInterface"))?);
        let get = |id: u32| {
            let f = query(id);
            (!f.is_null()).then_some(f)
        };
        let initialize: unsafe extern "C" fn() -> Status = std::mem::transmute(get(ID_INITIALIZE)?);
        let status = initialize();
        if status != 0 {
            log::warn!("NvAPI_Initialize failed ({status})");
            return None;
        }
        Some(Api {
            get_display: std::mem::transmute(get(ID_GET_ASSOCIATED_NVIDIA_DISPLAY_HANDLE)?),
            gpus_from_display: std::mem::transmute(get(ID_GET_PHYSICAL_GPUS_FROM_DISPLAY)?),
            output_id: std::mem::transmute(get(ID_GET_ASSOCIATED_DISPLAY_OUTPUT_ID)?),
            i2c_write: std::mem::transmute(get(ID_I2C_WRITE)?),
        })
    }
}

/// Opens channels for displays on NVIDIA outputs.
pub struct NvApi;

impl RawDdcProvider for NvApi {
    fn open(&self, gdi_name: &str) -> Option<Box<dyn RawDdcChannel>> {
        let api = api()?;
        let name = CString::new(gdi_name).ok()?;
        let mut display: Handle = null_mut();
        let mut gpus: [Handle; MAX_PHYSICAL_GPUS] = [null_mut(); MAX_PHYSICAL_GPUS];
        let mut gpu_count = 0u32;
        let mut output = 0u32;
        unsafe {
            // Fails for displays on another GPU, e.g. the iGPU of a hybrid laptop.
            if (api.get_display)(name.as_ptr(), &mut display) != 0
                || (api.gpus_from_display)(display, gpus.as_mut_ptr(), &mut gpu_count) != 0
                || gpu_count == 0
                || (api.output_id)(display, &mut output) != 0
            {
                return None;
            }
        }
        Some(Box::new(NvChannel {
            api,
            gpu: gpus[0],
            output,
        }))
    }
}

struct NvChannel {
    api: &'static Api,
    gpu: Handle,
    /// Single-bit output mask of the display.
    output: u32,
}

// SAFETY: GPU handles are opaque driver tokens valid on any thread; `Monitor`
// serialises every call on its bus lock.
unsafe impl Send for NvChannel {}
unsafe impl Sync for NvChannel {}

impl RawDdcChannel for NvChannel {
    fn name(&self) -> &'static str {
        "nvapi"
    }

    fn write(&self, packet: &Packet) -> Result<()> {
        // NVAPI sends the "register address" right after the I²C address,
        // which is where DDC/CI puts the host source address.
        let mut info = I2cInfo {
            version: I2C_INFO_VER3,
            display_mask: self.output,
            is_ddc_port: 1,
            i2c_dev_address: packet.dest,
            reg_address: &packet.source,
            reg_address_size: 1,
            data: packet.body.as_ptr(),
            size: packet.body.len() as u32,
            i2c_speed: I2C_SPEED_DEPRECATED,
            i2c_speed_khz: 0,
            port_id: 0,
            is_port_id_set: 0,
        };
        let status = unsafe { (self.api.i2c_write)(self.gpu, &mut info) };
        if status != 0 {
            bail!("NvAPI_I2CWrite failed ({status})");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn i2c_info_matches_the_c_layout() {
        assert_eq!(size_of::<I2cInfo>(), 64);
        assert_eq!(I2C_INFO_VER3, 0x0003_0040);
    }
}
