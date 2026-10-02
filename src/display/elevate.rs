//! Raw writes that a driver accepts only from an elevated process (Intel
//! IGCL) are re-run in a short-lived elevated copy of tarsier, so the app
//! itself never runs elevated. Each such write shows a UAC prompt.

use std::path::Path;

use anyhow::{Context as _, Result, anyhow, bail};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{HSTRING, PCWSTR, w};

use super::channel::{DisplayTarget, NeedsElevation, NoRetry, RawDdcChannel};
use super::ddcci::Packet;

/// First argument of the helper command line.
pub const HELPER_ARG: &str = "--raw-ddc-write";

/// One raw write for the elevated helper, which reopens the display through
/// the same backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperRequest {
    pub provider: String,
    pub target: DisplayTarget,
    pub packet: Packet,
}

impl HelperRequest {
    /// Arguments after [`HELPER_ARG`]: provider, GDI name, LUID, target id
    /// (`-` when unknown) and the packet as hex.
    pub fn to_args(&self) -> Vec<String> {
        let packet: String = [self.packet.dest, self.packet.source]
            .iter()
            .chain(&self.packet.body)
            .map(|b| format!("{b:02X}"))
            .collect();
        vec![
            self.provider.clone(),
            self.target.gdi_name.clone(),
            self.target.adapter_luid.map_or("-".into(), |l| format!("{l:#x}")),
            self.target.target_id.map_or("-".into(), |t| t.to_string()),
            packet,
        ]
    }

    pub fn from_args(args: &[String]) -> Result<Self> {
        let [provider, gdi_name, luid, target_id, packet] = args else {
            bail!("expected 5 arguments, got {}", args.len());
        };
        fn optional(s: &str) -> Option<&str> {
            (s != "-").then_some(s)
        }
        let bytes = (0..packet.len())
            .step_by(2)
            .map(|i| packet.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
            .collect::<Option<Vec<u8>>>()
            .context("packet is not hex")?;
        let [dest, source, body @ ..] = bytes.as_slice() else {
            bail!("packet too short");
        };
        Ok(HelperRequest {
            provider: provider.clone(),
            target: DisplayTarget {
                gdi_name: gdi_name.clone(),
                adapter_luid: optional(luid)
                    .map(|l| u64::from_str_radix(l.trim_start_matches("0x"), 16))
                    .transpose()
                    .context("bad LUID")?,
                target_id: optional(target_id)
                    .map(str::parse)
                    .transpose()
                    .context("bad target id")?,
            },
            packet: Packet {
                dest: *dest,
                source: *source,
                body: body.to_vec(),
            },
        })
    }
}

/// Runs a request elevated and reports how it went.
pub type Runner = fn(&HelperRequest) -> Result<()>;

/// Decorator: when the inner backend needs elevation and this process is
/// not elevated, hands the write to the elevated helper instead.
pub struct Elevating {
    pub inner: Box<dyn RawDdcChannel>,
    pub provider: &'static str,
    pub target: DisplayTarget,
    pub elevated: bool,
    pub run: Runner,
}

impl RawDdcChannel for Elevating {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn write(&self, packet: &Packet) -> Result<()> {
        match self.inner.write(packet) {
            Err(e) if !self.elevated && e.is::<NeedsElevation>() => {
                let request = HelperRequest {
                    provider: self.provider.to_string(),
                    target: self.target.clone(),
                    packet: packet.clone(),
                };
                // The helper retries on its own; retrying here would prompt again.
                (self.run)(&request).map_err(|e| anyhow!(NoRetry(format!("{e:#}"))))
            }
            result => result,
        }
    }
}

/// Whether this process runs with administrator rights.
pub fn process_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// [`Runner`] that starts `tarsier.exe --raw-ddc-write ...` through UAC and
/// waits for it. The helper leaves its error message in a temp file.
pub fn run_elevated(request: &HelperRequest) -> Result<()> {
    let exe = std::env::current_exe()?;
    let result_file = std::env::temp_dir().join(format!("tarsier-ddc-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&result_file);
    let mut args = vec![HELPER_ARG.to_string()];
    args.extend(request.to_args());
    args.push(result_file.display().to_string());
    let params = HSTRING::from(command_line(&args));
    let file = HSTRING::from(exe.as_os_str());

    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // Fails with ERROR_CANCELLED when the user declines the prompt.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|_| anyhow!("没有获得管理员授权，已取消切换"))?;
    let mut code = 1u32;
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
    }
    let message = std::fs::read_to_string(&result_file).ok();
    let _ = std::fs::remove_file(&result_file);
    if code == 0 {
        log::info!("elevated helper wrote {:?}", request.packet);
        Ok(())
    } else {
        bail!(
            "elevated helper failed: {}",
            message.unwrap_or_else(|| format!("exit code {code}"))
        )
    }
}

/// The elevated side: performs the write and records the outcome.
pub fn helper_main(args: &[String], write: impl FnOnce(&HelperRequest) -> Result<()>) -> i32 {
    let Some((result_file, request_args)) = args.split_last() else {
        return 2;
    };
    let result = HelperRequest::from_args(request_args).and_then(|request| write(&request));
    let message = match &result {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("{e:#}"),
    };
    let _ = std::fs::write(Path::new(result_file), &message);
    i32::from(result.is_err())
}

/// Joins arguments into a Windows command line, quoting as `CommandLineToArgvW` expects.
fn command_line(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
                return arg.clone();
            }
            let mut quoted = String::from('"');
            let mut backslashes = 0;
            for c in arg.chars() {
                match c {
                    '\\' => backslashes += 1,
                    '"' => {
                        quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                        backslashes = 0;
                    }
                    _ => backslashes = 0,
                }
                quoted.push(c);
            }
            quoted.extend(std::iter::repeat_n('\\', backslashes));
            quoted.push('"');
            quoted
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn request() -> HelperRequest {
        HelperRequest {
            provider: "igcl".into(),
            target: DisplayTarget {
                gdi_name: r"\\.\DISPLAY1".into(),
                adapter_luid: Some(0x1b7d9),
                target_id: Some(41030),
            },
            packet: Packet::set_vcp(0x50, 0xF4, 0x91),
        }
    }

    #[test]
    fn request_round_trips_through_args() {
        let args = request().to_args();
        assert_eq!(args[4], "6E508403F40091DC");
        assert_eq!(HelperRequest::from_args(&args).unwrap(), request());

        let unknown = HelperRequest {
            target: DisplayTarget {
                gdi_name: r"\\.\DISPLAY2".into(),
                ..Default::default()
            },
            ..request()
        };
        assert_eq!(HelperRequest::from_args(&unknown.to_args()).unwrap(), unknown);
    }

    #[test]
    fn rejects_malformed_args() {
        assert!(HelperRequest::from_args(&[]).is_err());
        let mut args = request().to_args();
        args[4] = "6E5".into();
        assert!(HelperRequest::from_args(&args).is_err());
    }

    #[test]
    fn quotes_like_command_line_to_argv() {
        let args = ["plain", "with space", r"C:\Temp Dir\", ""].map(String::from);
        assert_eq!(command_line(&args), r#"plain "with space" "C:\Temp Dir\\" """#);
    }

    struct Fails(fn() -> anyhow::Error);

    impl RawDdcChannel for Fails {
        fn name(&self) -> &'static str {
            "igcl-aux"
        }
        fn write(&self, _: &Packet) -> Result<()> {
            Err((self.0)())
        }
    }

    static RAN: Mutex<Vec<HelperRequest>> = Mutex::new(Vec::new());

    fn elevating(inner: fn() -> anyhow::Error, elevated: bool, run: Runner) -> Elevating {
        Elevating {
            inner: Box::new(Fails(inner)),
            provider: "igcl",
            target: request().target,
            elevated,
            run,
        }
    }

    #[test]
    fn hands_permission_failures_to_the_helper() {
        let ch = elevating(
            || NeedsElevation("ctlAUXAccess".into()).into(),
            false,
            |r| {
                RAN.lock().unwrap().push(r.clone());
                Ok(())
            },
        );
        ch.write(&request().packet).unwrap();
        assert_eq!(RAN.lock().unwrap().last(), Some(&request()));
    }

    #[test]
    fn helper_failures_are_not_retried() {
        let ch = elevating(
            || NeedsElevation("ctlAUXAccess".into()).into(),
            false,
            |_| bail!("UAC cancelled"),
        );
        let err = ch.write(&request().packet).unwrap_err();
        assert!(err.is::<NoRetry>());
    }

    #[test]
    fn other_failures_and_elevated_processes_pass_through() {
        let must_not_run: Runner = |_| panic!("helper must not run");
        let other = elevating(|| anyhow!("KMD_CALL"), false, must_not_run);
        assert_eq!(other.write(&request().packet).unwrap_err().to_string(), "KMD_CALL");
        let already = elevating(|| NeedsElevation("x".into()).into(), true, must_not_run);
        assert!(already.write(&request().packet).unwrap_err().is::<NeedsElevation>());
    }

    #[test]
    fn helper_main_reports_through_exit_code_and_file() {
        let file = std::env::temp_dir().join(format!("tarsier-helper-test-{}.txt", std::process::id()));
        let mut args = request().to_args();
        args.push(file.display().to_string());

        assert_eq!(
            helper_main(&args, |r| {
                assert_eq!(r, &request());
                Ok(())
            }),
            0
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "ok");

        assert_eq!(helper_main(&args, |_| bail!("ctlAUXAccess failed")), 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "ctlAUXAccess failed");
        let _ = std::fs::remove_file(file);
    }
}
