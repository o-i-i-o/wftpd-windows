//! WFTPD - SFTP/FTP Server Daemon
//!
//! This is the main daemon that runs in the background and manages
//! FTP and SFTP services. It listens on a named pipe for IPC commands
//! to reload configuration files.

#![windows_subsystem = "windows"]
extern crate windows_service;

use wftpg::AppState;
use wftpg::core::ipc::{IpcServer, ReloadCommand, ReloadResponse};
use wftpg::core::windows_ipc::PIPE_NAME;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::ffi::OsString;
use std::time::Duration;
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_manager::{ServiceManager, ServiceManagerAccess},
    service_dispatcher,
};

fn handle_reload(state: &AppState) -> ReloadResponse {
    let config_msg = match state.reload_config() {
        Ok(()) => "配置已重新加载".to_string(),
        Err(e) => format!("配置重新加载失败：{e}"),
    };

    let users_msg = match state.reload_users() {
        Ok(()) => "用户配置已重新加载".to_string(),
        Err(e) => format!("用户配置重新加载失败：{e}"),
    };

    let message = format!("{config_msg}; {users_msg}");

    if config_msg.contains("失败") || users_msg.contains("失败") {
        ReloadResponse::error(&message)
    } else {
        ReloadResponse::ok()
    }
}

fn handle_command(state: &AppState, cmd: &ReloadCommand) -> ReloadResponse {
    match cmd.action.as_str() {
        "reload" => handle_reload(state),
        _ => ReloadResponse::error("未知命令"),
    }
}

const SERVICE_NAME: &str = "wftpd";
const SERVICE_DISPLAY_NAME: &str = "WFTPD SFTP/FTP Server";
const SERVICE_DESCRIPTION: &str = "SFTP and FTP server daemon with GUI management";

define_windows_service!(ffi_service_main, my_service_main);

fn my_service_main(_arguments: Vec<OsString>) {
    if let Err(e) = run_service() {
        tracing::error!("Service failed: {e}");
    }
}

fn run_service() -> windows_service::Result<()> {
    let state = match create_app_state() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to initialize: {e}");
            return Err(windows_service::Error::Winapi(std::io::Error::other("Failed to initialize")));
        }
    };

    tracing::info!(
        "WFTPD Service - SFTP/FTP Server Daemon v{}",
        env!("CARGO_PKG_VERSION")
    );

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                tracing::info!("Service stopping...");
                running_clone.store(false, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Pause | ServiceControl::Continue | ServiceControl::Interrogate => {
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    let running_status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    };
    status_handle.set_service_status(running_status)?;

    let state_clone = Arc::clone(&state);
    let service_thread = thread::spawn(move || {
        let ipc_server = match create_ipc_server() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Failed to create IPC server: {e}");
                return;
            }
        };

        start_enabled_services(&state_clone);
        tracing::info!("Service ready to accept connections on named pipe: {PIPE_NAME}");

        run_main_loop_with_shutdown(&state_clone, &ipc_server, &running);

        state_clone.stop_all();
        tracing::info!("Service stopped");
    });

    let _ = service_thread.join();

    let stopped_status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    };
    status_handle.set_service_status(stopped_status)?;

    Ok(())
}

fn run_main_loop_with_shutdown(state: &Arc<AppState>, ipc_server: &IpcServer, running: &Arc<AtomicBool>) {
    while running.load(Ordering::SeqCst) {
        match ipc_server.accept_timeout(Duration::from_millis(100)) {
            Ok(Some((stream, cmd))) => {
                let state_clone = Arc::clone(state);
                thread::spawn(move || {
                    let response = handle_command(&state_clone, &cmd);
                    if let Err(e) = IpcServer::send_response(&stream, &response) {
                        tracing::error!("Failed to send response: {e}");
                    }
                });
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!("Failed to accept IPC connection: {e}");
            }
        }
    }
}

fn install_service() -> anyhow::Result<()> {
    let current_exe = std::env::current_exe()?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("无法获取当前程序目录"))?;

    let wftpd_exe = exe_dir.join("wftpd.exe");
    if !wftpd_exe.exists() {
        return Err(anyhow::anyhow!(
            "在当前目录未找到 wftpd.exe，请确保 wftpd.exe 与 wftp-gui.exe 在同一目录"
        ));
    }

    tracing::info!("Using wftpd.exe at {}", wftpd_exe.display());

    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: wftpd_exe,
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };

    let service = service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;
    if let Err(e) = service.set_description(SERVICE_DESCRIPTION) {
        tracing::warn!("设置服务描述失败（可忽略）: {e:?}");
    }

    tracing::info!("Service installed successfully");
    Ok(())
}

fn uninstall_service() -> anyhow::Result<()> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service = service_manager.open_service(SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::DELETE)?;
    service.stop()?;
    service.delete()?;

    tracing::info!("Service uninstalled successfully");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "--install" => {
                if let Err(e) = install_service() {
                    tracing::error!("Failed to install service: {e}");
                    std::process::exit(1);
                }
                return;
            }
            "--uninstall" => {
                if let Err(e) = uninstall_service() {
                    tracing::error!("Failed to uninstall service: {e}");
                    std::process::exit(1);
                }
                return;
            }
            _ => {}
        }
    }

    if let Err(_e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        run_console_application();
    }
}

fn run_console_application() {
    let state = match create_app_state() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to initialize: {e}");
            std::process::exit(1);
        }
    };

    tracing::info!("WFTPD - SFTP/FTP Server Daemon v{}", env!("CARGO_PKG_VERSION"));

    let ipc_server = match create_ipc_server() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create IPC server: {e}");
            std::process::exit(1);
        }
    };

    setup_signal_handler(&state);
    start_enabled_services(&state);

    tracing::info!("Ready to accept connections on named pipe: {PIPE_NAME}");

    run_main_loop(&state, &ipc_server);
}

fn create_app_state() -> anyhow::Result<Arc<AppState>> {
    Ok(Arc::new(AppState::new()?))
}

fn create_ipc_server() -> anyhow::Result<IpcServer> {
    IpcServer::new()
}

fn setup_signal_handler(state: &Arc<AppState>) {
    let state_clone = Arc::clone(state);
    ctrlc::set_handler(move || {
        tracing::info!("Shutting down...");
        state_clone.stop_all();
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");
}

fn start_enabled_services(state: &Arc<AppState>) {
    let (ftp_enabled, sftp_enabled) = get_enabled_services(state);

    if (ftp_enabled || sftp_enabled)
        && let Err(e) = state.start_all() {
            tracing::error!("Failed to start services: {e}");
        }
}

fn get_enabled_services(state: &Arc<AppState>) -> (bool, bool) {
    if let Ok(cfg) = state.config.try_lock() {
        (cfg.ftp.enabled, cfg.sftp.enabled)
    } else {
        (false, false)
    }
}

fn run_main_loop(state: &Arc<AppState>, ipc_server: &IpcServer) {
    loop {
        match ipc_server.accept() {
            Ok((stream, cmd)) => {
                let state_clone = Arc::clone(state);
                thread::spawn(move || {
                    let response = handle_command(&state_clone, &cmd);
                    if let Err(e) = IpcServer::send_response(&stream, &response) {
                        tracing::error!("Failed to send response: {e}");
                    }
                });
            }
            Err(e) => {
                tracing::error!("Failed to accept IPC connection: {e}");
            }
        }
    }
}
