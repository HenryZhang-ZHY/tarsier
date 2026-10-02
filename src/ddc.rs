//! DDC/CI monitor control through the Windows `dxva2` Monitor Configuration API.
//!
//! Every call talks to the monitor over I²C and is slow (tens of ms, the
//! capabilities string can take seconds), so call these off the UI thread.

use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{HANDLE, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::core::BOOL;

use crate::mccs::{self, Capabilities};

const RETRIES: usize = 3;
/// MCCS asks hosts to wait at least 50ms between commands.
const COMMAND_GAP: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    pub current: u32,
    pub max: u32,
}

/// One physical monitor with an open DDC/CI handle.
pub struct Monitor {
    handle: Mutex<SendHandle>,
    /// Stable identifier (device path when available).
    pub id: String,
    pub name: String,
    pub caps: Option<Capabilities>,
}

struct SendHandle(HANDLE);
// SAFETY: physical monitor handles are plain kernel handles usable from any
// thread; access is serialised through the surrounding Mutex.
unsafe impl Send for SendHandle {}

impl Drop for SendHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyPhysicalMonitor(self.0);
        }
    }
}

impl Monitor {
    pub fn get(&self, code: u8) -> Result<Feature> {
        let handle = self.handle.lock().unwrap();
        retry(|| {
            let mut current = 0u32;
            let mut max = 0u32;
            let ok = unsafe { GetVCPFeatureAndVCPFeatureReply(handle.0, code, None, &mut current, Some(&mut max)) };
            thread::sleep(COMMAND_GAP);
            if ok == 0 {
                bail!("GetVCPFeature 0x{code:02X} failed");
            }
            Ok(Feature { current, max })
        })
    }

    pub fn set(&self, code: u8, value: u32) -> Result<()> {
        let handle = self.handle.lock().unwrap();
        retry(|| {
            let ok = unsafe { SetVCPFeature(handle.0, code, value) };
            thread::sleep(COMMAND_GAP);
            if ok == 0 {
                bail!("SetVCPFeature 0x{code:02X}={value} failed");
            }
            Ok(())
        })
    }

    /// Input sources the monitor claims to support.
    pub fn input_sources(&self) -> Vec<u8> {
        self.caps.as_ref().map(Capabilities::input_sources).unwrap_or_default()
    }

    pub fn current_input(&self) -> Option<u8> {
        // Some monitors put garbage in the high byte.
        self.get(mccs::VCP_INPUT_SOURCE).ok().map(|f| (f.current & 0xFF) as u8)
    }
}

fn retry<T>(mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let mut last = None;
    for _ in 0..RETRIES {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap())
}

/// Enumerate all monitors that expose a physical-monitor handle.
pub fn enumerate() -> Result<Vec<Monitor>> {
    let names = display_names();
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

    let mut monitors = Vec::new();
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
            let handle = SendHandle(pm.hPhysicalMonitor);
            // PHYSICAL_MONITOR is packed; copy the array out before borrowing it.
            let description = wide_to_string(&{ pm.szPhysicalMonitorDescription });
            let caps = capabilities(handle.0);
            let display = names.get(&gdi_name);
            let name = display
                .map(|d| d.friendly.clone())
                .filter(|n| !n.is_empty())
                .or_else(|| caps.as_ref().and_then(|c| c.model.clone()))
                .unwrap_or(description);
            let id = display
                .map(|d| d.device_path.clone())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| format!("{gdi_name}#{i}"));
            monitors.push(Monitor {
                handle: Mutex::new(handle),
                id,
                name,
                caps,
            });
        }
    }
    Ok(monitors)
}

fn capabilities(handle: HANDLE) -> Option<Capabilities> {
    retry(|| {
        let mut len = 0u32;
        if unsafe { GetCapabilitiesStringLength(handle, &mut len) } == 0 || len == 0 {
            thread::sleep(COMMAND_GAP);
            bail!("GetCapabilitiesStringLength failed");
        }
        let mut buf = vec![0u8; len as usize];
        let ok = unsafe { CapabilitiesRequestAndCapabilitiesReply(handle, &mut buf) };
        thread::sleep(COMMAND_GAP);
        if ok == 0 {
            bail!("CapabilitiesRequestAndCapabilitiesReply failed");
        }
        Ok(mccs::parse_capabilities(&String::from_utf8_lossy(&buf)))
    })
    .ok()
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
                },
            );
        }
    }
    map
}

fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}
