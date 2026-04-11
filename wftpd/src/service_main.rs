//! WFTPD - FTP/SFTP server daemon
//!
//! Runs as Windows service, manages FTP and SFTP services, and receives IPC commands via named pipe for config reload

#![windows_subsystem = "windows"]
extern crate windows_service;

use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use wftpd::AppState;
use wftpd::core::ipc::{IpcServer, ReloadCommand, ReloadResponse};
use wftpd::core::windows_ipc::PIPE_NAME;
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
        Ok(()) => "Configuration reloaded".to_string(),
        Err(e) => format!("Configuration reload failed: {e}"),
    };

    let users_msg = match state.reload_users() {
        Ok(()) => "User configuration reloaded".to_string(),
        Err(e) => format!("User configuration reload failed: {e}"),
    };

    let message = format!("{config_msg}; {users_msg}");

    if config_msg.contains("failed") || users_msg.contains("failed") {
        ReloadResponse::error(&message)
    } else {
        ReloadResponse::ok()
    }
}

fn handle_command(state: &AppState, cmd: &ReloadCommand) -> ReloadResponse {
    match cmd.action.as_str() {
        "reload" => handle_reload(state),
        _ => ReloadResponse::error("Unknown command"),
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
            return Err(windows_service::Error::Winapi(std::io::Error::other(
                "Failed to initialize",
            )));
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

/// Run main loop (with graceful shutdown support)
fn run_main_loop_with_shutdown(
    state: &Arc<AppState>,
    ipc_server: &IpcServer,
    running: &Arc<AtomicBool>,
) {
    while running.load(Ordering::SeqCst) {
        match ipc_server.accept_timeout(Duration::from_millis(100)) {
            Ok(Some(mut connection)) => {
                let state_clone = Arc::clone(state);
                thread::spawn(move || {
                    if let Ok(cmd) = connection.receive_command() {
                        let response = handle_command(&state_clone, &cmd);
                        if let Err(e) = connection.send_response(&response) {
                            tracing::error!("Failed to send IPC response: {e}");
                        }
                    } else {
                        tracing::warn!("Failed to receive IPC command");
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

fn main() {
    // Set global panic hook to capture unhandled panics and log them
    std::panic::set_hook(Box::new(|panic_info| {
        let location = if let Some(location) = panic_info.location() {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        } else {
            "unknown location".to_string()
        };

        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        // Try to write to log file
        if let Ok(log_dir) = std::env::var("WFTPD_LOG_DIR") {
            let log_file = std::path::Path::new(&log_dir).join("panic.log");
            let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S");
            let panic_msg = format!(
                "\n=== PANIC at {} ===\nLocation: {}\nMessage: {}\nBacktrace:\n{:?}\n",
                timestamp,
                location,
                message,
                std::backtrace::Backtrace::force_capture()
            );
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file)
            {
                use std::io::Write;
                let _ = file.write_all(panic_msg.as_bytes());
            }
        }

        // Also output to stderr
        eprintln!("\n=== FATAL ERROR ===");
        eprintln!("Panic occurred at: {}", location);
        eprintln!("Message: {}", message);
        eprintln!("Please check the log files for details.");
        eprintln!("==================\n");
    }));

    tracing::info!("WFTPD process starting...");

    // Start service directly (no command line install/uninstall support)
    if let Err(_e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        tracing::info!("Service dispatcher failed, running as console application");
        run_console_application();
    }
}

fn run_console_application() {
    tracing::info!("Entering console application mode...");
    let state = match create_app_state() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to initialize: {e}");
            tracing::error!("Application initialization failed: {}", e);
            std::process::exit(1);
        }
    };

    tracing::info!(
        "WFTPD - SFTP/FTP Server Daemon v{}",
        env!("CARGO_PKG_VERSION")
    );

    tracing::info!("Creating IPC server...");
    let ipc_server = match create_ipc_server() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create IPC server: {e}");
            std::process::exit(1);
        }
    };

    tracing::info!("Setting up signal handler...");
    match setup_signal_handler(&state) {
        Ok(()) => tracing::info!("Signal handler installed successfully"),
        Err(e) => {
            tracing::error!("Failed to set up signal handler: {}", e);
            // Don't exit, continue running, just without signal handling
        }
    }

    tracing::info!("Starting enabled services...");
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

fn setup_signal_handler(state: &Arc<AppState>) -> anyhow::Result<()> {
    let state_clone = Arc::clone(state);
    ctrlc::set_handler(move || {
        tracing::info!("Shutting down gracefully...");
        state_clone.stop_all();

        // Wait for active connections to close gracefully (max 10 seconds)
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
    })
    .map_err(|e| anyhow::anyhow!("Failed to set Ctrl-C handler: {}", e))?;

    Ok(())
}

fn start_enabled_services(state: &Arc<AppState>) {
    tracing::info!("Checking enabled services...");
    let (ftp_enabled, sftp_enabled) = get_enabled_services(state);

    if ftp_enabled || sftp_enabled {
        tracing::info!(
            "Starting enabled services: FTP={}, SFTP={}",
            ftp_enabled,
            sftp_enabled
        );
        if let Err(e) = state.start_all() {
            tracing::error!("Failed to start services: {e}");
        } else {
            tracing::info!("Services started successfully");
        }
    } else {
        tracing::info!("No services are enabled in configuration");
    }
}

fn get_enabled_services(state: &Arc<AppState>) -> (bool, bool) {
    if let Some(cfg) = state.config.try_lock() {
        (cfg.ftp.enabled, cfg.sftp.enabled)
    } else {
        tracing::warn!("Failed to acquire config lock for service startup check, retrying...");
        // Brief wait then retry once
        std::thread::sleep(std::time::Duration::from_millis(50));
        match state.config.try_lock() {
            Some(cfg) => (cfg.ftp.enabled, cfg.sftp.enabled),
            None => {
                tracing::error!(
                    "Cannot acquire config lock for service startup, services will not start"
                );
                (false, false)
            }
        }
    }
}

/// Run main loop (no shutdown support)
fn run_main_loop(state: &Arc<AppState>, ipc_server: &IpcServer) {
    loop {
        match ipc_server.accept() {
            Ok(mut connection) => {
                let state_clone = Arc::clone(state);
                thread::spawn(move || {
                    if let Ok(cmd) = connection.receive_command() {
                        let response = handle_command(&state_clone, &cmd);
                        if let Err(e) = connection.send_response(&response) {
                            tracing::error!("Failed to send IPC response: {e}");
                        }
                    } else {
                        tracing::warn!("Failed to receive IPC command");
                    }
                });
            }
            Err(e) => {
                tracing::error!("Failed to accept IPC connection: {e}");
            }
        }
    }
}
