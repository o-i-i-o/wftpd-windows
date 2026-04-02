//! WFTPD - SFTP/FTP Server Daemon
//!
//! This is the main daemon that runs in the background and manages
//! FTP and SFTP services. It listens on a named pipe for IPC commands
//! to reload configuration files.

#![windows_subsystem = "windows"]
extern crate windows_service;

use wftpd::AppState;
use wftpd::core::ipc::{IpcServer, ReloadCommand, ReloadResponse};
use wftpd::core::windows_ipc::PIPE_NAME;
use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
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

/// 运行主循环（支持优雅关闭）
fn run_main_loop_with_shutdown(state: &Arc<AppState>, ipc_server: &IpcServer, running: &Arc<AtomicBool>) {
    while running.load(Ordering::SeqCst) {
        match ipc_server.accept_timeout(Duration::from_millis(100)) {
            Ok(Some(mut connection)) => {
                let state_clone = Arc::clone(state);
                thread::spawn(move || {
                    if let Ok(cmd) = connection.receive_command() {
                        let response = handle_command(&state_clone, &cmd);
                        if let Err(e) = connection.send_response(&response) {
                            tracing::error!("发送 IPC 响应失败：{e}");
                        }
                    } else {
                        tracing::warn!("接收 IPC 命令失败");
                    }
                });
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!("接受 IPC 连接失败：{e}");
            }
        }
    }
}

fn main() {
    // 直接启动服务（不支持命令行安装/卸载）
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
        tracing::info!("Shutting down gracefully...");
        state_clone.stop_all();

        // 等待活跃连接优雅关闭（最多等待 10 秒）
        let mut waited = 0u32;
        while waited < 100 {
            {
                let cfg = state_clone.config.lock();
                if cfg.server.get_global_count() == 0 {
                    tracing::info!("All connections closed, shutting down");
                    std::process::exit(0);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            waited += 1;
        }

        tracing::warn!("Graceful shutdown timeout reached, forcing exit");
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
    if let Some(cfg) = state.config.try_lock() {
        (cfg.ftp.enabled, cfg.sftp.enabled)
    } else {
        tracing::warn!("Failed to acquire config lock for service startup check, retrying...");
        // 短暂等待后重试一次
        std::thread::sleep(std::time::Duration::from_millis(50));
        match state.config.try_lock() {
            Some(cfg) => (cfg.ftp.enabled, cfg.sftp.enabled),
            None => {
                tracing::error!("Cannot acquire config lock for service startup, services will not start");
                (false, false)
            }
        }
    }
}

/// 运行主循环（不支持关闭）
fn run_main_loop(state: &Arc<AppState>, ipc_server: &IpcServer) {
    loop {
        match ipc_server.accept() {
            Ok(mut connection) => {
                let state_clone = Arc::clone(state);
                thread::spawn(move || {
                    if let Ok(cmd) = connection.receive_command() {
                        let response = handle_command(&state_clone, &cmd);
                        if let Err(e) = connection.send_response(&response) {
                            tracing::error!("发送 IPC 响应失败：{e}");
                        }
                    } else {
                        tracing::warn!("接收 IPC 命令失败");
                    }
                });
            }
            Err(e) => {
                tracing::error!("接受 IPC 连接失败：{e}");
            }
        }
    }
}
