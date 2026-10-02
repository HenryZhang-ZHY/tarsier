//! Windows backend: monitor enumeration and the `dxva2` Monitor Configuration
//! API as a [`VcpChannel`]. Every call talks to the monitor over I²C and is
//! slow (tens of ms, the capabilities string can take seconds), so call these
//! off the UI thread.

use std::collections::HashMap;

use anyhow::{Context as _, Result, bail};
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{HANDLE, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW,
};
use windows::core::BOOL;

use super::channel::{DisplayTarget, Feature, VcpChannel};

/// A physical monitor handle from `dxva2`.
pub struct Dxva2Channel(HANDLE);

// SAFETY: physical monitor handles are plain kernel handles usable from any
// thread; `Monitor` serialises every call on its bus lock.
unsafe impl Send for Dxva2Channel {}
unsafe impl Sync for Dxva2Channel {}

impl Drop for Dxva2Channel {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyPhysicalMonitor(self.0);
        }
    }
}

impl VcpChannel for Dxva2Channel {
    fn name(&self) -> &'static str {
        "dxva2"
    }

    fn get(&self, code: u8) -> Result<Feature> {
        let mut current = 0u32;
        let mut max = 0u32;
        if unsafe { GetVCPFeatureAndVCPFeatureReply(self.0, code, None, &mut current, Some(&mut max)) } == 0 {
            bail!("GetVCPFeature 0x{code:02X} failed");
        }
        Ok(Feature { current, max })
    }

    fn set(&self, code: u8, value: u32) -> Result<()> {
        if unsafe { SetVCPFeature(self.0, code, value) } == 0 {
            bail!("SetVCPFeature 0x{code:02X}={value} failed");
        }
        Ok(())
    }

    fn capabilities(&self) -> Result<String> {
        let mut len = 0u32;
        if unsafe { GetCapabilitiesStringLength(self.0, &mut len) } == 0 || len == 0 {
            bail!("GetCapabilitiesStringLength failed");
        }
        let mut buf = vec![0u8; len as usize];
        if unsafe { CapabilitiesRequestAndCapabilitiesReply(self.0, &mut buf) } == 0 {
            bail!("CapabilitiesRequestAndCapabilitiesReply failed");
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// A physical monitor found by enumeration, before any DDC/CI traffic.
pub struct Found {
    pub channel: Dxva2Channel,
    pub target: DisplayTarget,
    /// Monitor device path (stable across reboots), or a positional fallback.
    pub id: String,
    /// The display adapter (GPU) driving this output.
    pub adapter: Option<String>,
    /// EDID friendly name, if Windows knows one.
    pub friendly_name: Option<String>,
    pub description: String,
}

/// Enumerate all monitors that expose a physical-monitor handle.
pub fn enumerate() -> Result<Vec<Found>> {
    let names = display_names();
    let adapters = adapters();
    let mut hmonitors: Vec<HMONITOR> = Vec::new();
    unsafe extern "system" fn collect(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        list.push(hmon);
        true.into()
    }
    unsafe {
        EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut hmonitors as *mut _ as isize))
            .ok()
            .context("EnumDisplayMonitors")?;
    }

    let mut found = Vec::new();
    for hmon in hmonitors {
        let gdi_name = gdi_device_name(hmon).unwrap_or_default();
        let mut count = 0u32;
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) }.is_err() || count == 0 {
            continue;
        }
        let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
        if unsafe { GetPhysicalMonitorsFromHMONITOR(hmon, &mut physical) }.is_err() {
            continue;
        }
        for (i, pm) in physical.into_iter().enumerate() {
            let channel = Dxva2Channel(pm.hPhysicalMonitor);
            // PHYSICAL_MONITOR is packed; copy the array out before borrowing it.
            let description = wide_to_string(&{ pm.szPhysicalMonitorDescription });
            let display = names.get(&gdi_name);
            found.push(Found {
                channel,
                id: display
                    .map(|d| d.device_path.clone())
                    .filter(|p| !p.is_empty())
                    .unwrap_or_else(|| format!("{gdi_name}#{i}")),
                friendly_name: display.map(|d| d.friendly.clone()).filter(|n| !n.is_empty()),
                adapter: adapters.get(&gdi_name).cloned(),
                target: DisplayTarget {
                    gdi_name: gdi_name.clone(),
                    adapter_luid: display.map(|d| d.adapter_luid),
                    target_id: display.map(|d| d.target_id),
                },
                description,
            });
        }
    }
    Ok(found)
}

/// Maps GDI device names to the adapter that drives them.
fn adapters() -> HashMap<String, String> {
    let mut map = HashMap::new();
    for i in 0.. {
        let mut device = DISPLAY_DEVICEW {
            cb: size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if !unsafe { EnumDisplayDevicesW(None, i, &mut device, 0) }.as_bool() {
            break;
        }
        map.insert(wide_to_string(&device.DeviceName), wide_to_string(&device.DeviceString));
    }
    map
}

fn gdi_device_name(hmon: HMONITOR) -> Option<String> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    unsafe { GetMonitorInfoW(hmon, &mut info as *mut _ as *mut MONITORINFO) }
        .as_bool()
        .then(|| wide_to_string(&info.szDevice))
}

struct DisplayName {
    friendly: String,
    device_path: String,
    adapter_luid: u64,
    target_id: u32,
}

/// Maps GDI device names (`\\.\DISPLAY1`) to the EDID friendly name and device path.
fn display_names() -> HashMap<String, DisplayName> {
    let mut map = HashMap::new();
    let mut path_count = 0u32;
    let mut mode_count = 0u32;
    unsafe {
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count).is_err() {
            return map;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return map;
        }
        paths.truncate(path_count as usize);

        for path in paths {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 {
                continue;
            }
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut target.header) != 0 {
                continue;
            }
            map.insert(
                wide_to_string(&source.viewGdiDeviceName),
                DisplayName {
                    friendly: wide_to_string(&target.monitorFriendlyDeviceName),
                    device_path: wide_to_string(&target.monitorDevicePath),
                    adapter_luid: luid(path.targetInfo.adapterId),
                    target_id: path.targetInfo.id,
                },
            );
        }
    }
    map
}

fn luid(id: windows::Win32::Foundation::LUID) -> u64 {
    ((id.HighPart as u32 as u64) << 32) | id.LowPart as u64
}

fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}
