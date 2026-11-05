//! Elevated background service. Runs as LocalSystem, reads drives through the
//! NTFS master file table and hands snapshots to the unelevated app and MCP
//! server over a local named pipe, so they never trigger UAC.

#[cfg(not(windows))]
pub fn run(_args: &[String]) -> anyhow::Result<()> {
    anyhow::bail!("the Gigamap service is Windows-only")
}

#[cfg(windows)]
pub use win::run;

#[cfg(windows)]
mod win {
    use anyhow::{bail, Context, Result};
    use gigamap_core::{ipc, mft};
    use std::ffi::OsString;
    use std::io::{BufRead, BufReader};
    use std::os::windows::io::FromRawHandle;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_service::service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode, ServiceInfo,
        ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    use windows_service::{define_windows_service, service_dispatcher};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    const NAME: &str = "Gigamap";

    pub fn run(args: &[String]) -> Result<()> {
        match args.first().map(String::as_str) {
            None => {
                service_dispatcher::start(NAME, ffi_service_main).context("not started by the service manager; use `gigamap service --console` to run in a terminal")?;
                Ok(())
            }
            Some("--console") => {
                eprintln!("serving {} (Ctrl+C to stop)", ipc::PIPE);
                serve_pipe();
                Ok(())
            }
            Some("install") => install(),
            Some("uninstall") => uninstall(),
            Some(other) => bail!("unknown service option {other}"),
        }
    }

    define_windows_service!(ffi_service_main, service_main);

    fn service_main(_args: Vec<OsString>) {
        let _ = run_service();
    }

    fn status(state: ServiceState) -> ServiceStatus {
        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running {
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
            } else {
                ServiceControlAccept::empty()
            },
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        }
    }

    fn run_service() -> windows_service::Result<()> {
        let (tx, rx) = mpsc::channel::<()>();
        let handle = service_control_handler::register(NAME, move |ev| match ev {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })?;
        handle.set_service_status(status(ServiceState::Running))?;
        std::thread::spawn(serve_pipe);
        let _ = rx.recv();
        handle.set_service_status(status(ServiceState::Stopped))?;
        Ok(())
    }

    fn serve_pipe() {
        let name: Vec<u16> = ipc::PIPE.encode_utf16().chain(Some(0)).collect();
        // SYSTEM and Administrators full control, local authenticated users read/write.
        let sddl: Vec<u16> = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)".encode_utf16().chain(Some(0)).collect();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut());
        }
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        loop {
            let h = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES,
                    1 << 20,
                    1 << 20,
                    0,
                    &sa,
                )
            };
            if h == INVALID_HANDLE_VALUE {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            let ok = unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) };
            if ok == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
                unsafe { CloseHandle(h) };
                continue;
            }
            let file = unsafe { std::fs::File::from_raw_handle(h as _) };
            std::thread::spawn(move || {
                let _ = handle_client(file);
            });
        }
    }

    fn handle_client(file: std::fs::File) -> Result<()> {
        let writer = file.try_clone()?;
        let mut line = String::new();
        BufReader::new(file).read_line(&mut line)?;
        let req: serde_json::Value = serde_json::from_str(line.trim())?;
        let result = match req["cmd"].as_str() {
            Some("scan") => {
                let drive = req["drive"].as_str().and_then(|d| d.chars().next()).filter(|c| c.is_ascii_alphabetic());
                match drive {
                    Some(d) => mft::scan_drive(d),
                    None => Err(anyhow::anyhow!("drive letter required")),
                }
            }
            other => Err(anyhow::anyhow!("unknown command {other:?}")),
        };
        ipc::write_reply(&writer, result.as_ref().map_err(|e| anyhow::anyhow!("{e:#}")))?;
        writer.sync_all()?; // FlushFileBuffers: let the client read everything before we close
        Ok(())
    }

    fn install() -> Result<()> {
        let exe = std::env::current_exe()?;
        let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
            .context("run this from an elevated terminal")?;
        let info = ServiceInfo {
            name: NAME.into(),
            display_name: NAME.into(),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe,
            launch_arguments: vec!["service".into()],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };
        let svc = mgr.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)?;
        svc.set_description("Reads the NTFS master file table so Gigamap can map drives in seconds without asking for admin rights each time.")?;
        svc.start::<&str>(&[])?;
        println!("Gigamap service installed and started");
        Ok(())
    }

    fn uninstall() -> Result<()> {
        let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let svc = mgr.open_service(NAME, ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS)?;
        if svc.query_status()?.current_state != ServiceState::Stopped {
            let _ = svc.stop();
            std::thread::sleep(Duration::from_secs(1));
        }
        svc.delete()?;
        println!("Gigamap service removed");
        Ok(())
    }
}
