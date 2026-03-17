use anyhow::Result;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::core::config::Config;
use crate::core::logger::Logger;
use crate::core::users::UserManager;
use crate::core::file_logger::{FileLogger, FileLogInfo};
use crate::core::path_utils::safe_resolve_path;

type PassiveListenerMap = Arc<Mutex<HashMap<u16, Arc<Mutex<Option<TcpListener>>>>>>;

pub struct FtpServer {
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    logger: Arc<Mutex<Logger>>,
    file_logger: Arc<Mutex<FileLogger>>,
    running: Arc<Mutex<bool>>,
    listener: Arc<Mutex<Option<TcpListener>>>,
    passive_listeners: PassiveListenerMap,
}

impl FtpServer {
    pub fn new(
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
        logger: Arc<Mutex<Logger>>,
        file_logger: Arc<Mutex<FileLogger>>,
    ) -> Self {
        FtpServer {
            config,
            user_manager,
            logger,
            file_logger,
            running: Arc::new(Mutex::new(false)),
            listener: Arc::new(Mutex::new(None)),
            passive_listeners: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start(&self) -> Result<()> {
        let (bind_ip, ftp_port) = {
            let cfg = match self.config.lock() {
                Ok(guard) => guard,
                Err(e) => return Err(anyhow::anyhow!("获取配置锁失败: {}", e)),
            };
            (cfg.ftp.bind_ip.clone(), cfg.server.ftp_port)
        };
        let bind_addr = format!("{}:{}", bind_ip, ftp_port);
        
        let listener = TcpListener::bind(&bind_addr)?;
        listener.set_nonblocking(true)?;
        
        {
            let mut running = match self.running.lock() {
                Ok(guard) => guard,
                Err(e) => return Err(anyhow::anyhow!("获取运行状态锁失败: {}", e)),
            };
            *running = true;
        }
        
        {
            let mut listener_guard = match self.listener.lock() {
                Ok(guard) => guard,
                Err(e) => return Err(anyhow::anyhow!("获取监听器锁失败: {}", e)),
            };
            *listener_guard = Some(listener.try_clone()?);
        }

        let config = Arc::clone(&self.config);
        let user_manager = Arc::clone(&self.user_manager);
        let logger = Arc::clone(&self.logger);
        let file_logger = Arc::clone(&self.file_logger);
        let running = Arc::clone(&self.running);
        let passive_listeners = Arc::clone(&self.passive_listeners);
        let server_listener = Arc::clone(&self.listener);

        std::thread::spawn(move || {
            loop {
                let is_running = match running.lock() {
                    Ok(guard) => *guard,
                    Err(_) => break,
                };
                if !is_running {
                    break;
                }

                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let config = Arc::clone(&config);
                        let user_manager = Arc::clone(&user_manager);
                        let logger = Arc::clone(&logger);
                        let file_logger = Arc::clone(&file_logger);
                        let passive_listeners = Arc::clone(&passive_listeners);

                        std::thread::spawn(move || {
                            if let Err(e) = handle_ftp_connection(
                                stream,
                                &config,
                                &user_manager,
                                &logger,
                                &file_logger,
                                &passive_listeners,
                            ) && let Ok(mut logger) = logger.lock() {
                                logger.error("FTP", &format!("Connection handler error: {}", e));
                            }
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    Err(e) => {
                        let is_running = match running.lock() {
                            Ok(guard) => *guard,
                            Err(_) => break,
                        };
                        if !is_running {
                            break;
                        }
                        eprintln!("Failed to accept connection: {}", e);
                    }
                }
            }
            
            {
                let mut listener_guard = match server_listener.lock() {
                    Ok(guard) => guard,
                    Err(e) => {
                        log::error!("获取服务器监听器锁失败: {}", e);
                        return;
                    }
                };
                *listener_guard = None;
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        {
            let mut running = match self.running.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    log::error!("获取运行状态锁失败: {}", e);
                    return;
                }
            };
            *running = false;
        }
        
        {
            let mut listener_guard = match self.listener.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    log::error!("获取监听器锁失败: {}", e);
                    return;
                }
            };
            *listener_guard = None;
        }

        let mut listeners = match self.passive_listeners.lock() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!("获取被动监听器锁失败: {}", e);
                return;
            }
        };
        listeners.clear();
    }

    pub fn is_running(&self) -> bool {
        let listener_guard = match self.listener.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        listener_guard.is_some()
    }
}

use std::sync::atomic::{AtomicBool, Ordering};

fn handle_ftp_connection(
    mut stream: TcpStream,
    config: &Arc<Mutex<Config>>,
    user_manager: &Arc<Mutex<UserManager>>,
    logger: &Arc<Mutex<Logger>>,
    file_logger: &Arc<Mutex<FileLogger>>,
    passive_listeners: &PassiveListenerMap,
) -> Result<()> {
    let remote_addr = stream.peer_addr()?;
    let remote_ip = remote_addr.ip().to_string();

    if let Ok(mut logger) = logger.lock() {
        logger.client_action(
            "FTP",
            &format!("Client connected from {}", remote_ip),
            &remote_ip,
            None,
            "CONNECT",
        );
    }

    {
        let cfg = match config.lock() {
            Ok(guard) => guard,
            Err(_) => {
                let response = b"500 Internal server error\r\n";
                let _ = stream.write_all(response);
                return Ok(());
            }
        };
        if !cfg.is_ip_allowed(&remote_ip) {
            let response = b"530 Connection denied by IP filter\r\n";
            if let Ok(mut log) = logger.try_lock() {
                log.warning("FTP", &format!("Connection rejected from {} by IP filter", remote_ip));
            }
            stream.write_all(response)?;
            return Ok(());
        }
    }

    let welcome_msg;
    {
        let cfg = match config.lock() {
            Ok(guard) => guard,
            Err(_) => {
                let response = b"500 Internal server error\r\n";
                let _ = stream.write_all(response);
                return Ok(());
            }
        };
        welcome_msg = cfg.ftp.welcome_message.clone();
    }
    stream.write_all(format!("220 {}\r\n", welcome_msg).as_bytes())?;

    let mut current_user: Option<String> = None;
    let mut authenticated = false;
    let mut data_port: Option<u16> = None;
    let mut data_addr: Option<String> = None;
    let mut passive_mode;
    let mut cwd;
    let mut home_dir;
    let mut transfer_mode;
    {
        let cfg = match config.lock() {
            Ok(guard) => guard,
            Err(_) => {
                let response = b"500 Internal server error\r\n";
                let _ = stream.write_all(response);
                return Ok(());
            }
        };
        let default_home = PathBuf::from(&cfg.ftp.default_home);
        let home_canon = match default_home.canonicalize() {
            Ok(c) => c.to_string_lossy().to_string(),
            Err(e) => {
                log::error!("Failed to canonicalize home directory: {} - {}", cfg.ftp.default_home, e);
                cfg.ftp.default_home.clone()
            }
        };
        cwd = home_canon.clone();
        home_dir = home_canon;
        transfer_mode = cfg.ftp.default_transfer_mode.clone();
        passive_mode = cfg.ftp.default_passive_mode;
    }

    let mut rest_offset: u64 = 0;
    let mut rename_from: Option<String> = None;
    let abort_flag = Arc::new(AtomicBool::new(false));

    let mut buffer = [0u8; 4096];
    let mut last_timeout = 0u64;

    loop {
        let conn_timeout = match config.lock() {
            Ok(guard) => guard.server.connection_timeout,
            Err(_) => break,
        };
        
        if conn_timeout != last_timeout {
            stream.set_read_timeout(Some(Duration::from_secs(conn_timeout)))?;
            last_timeout = conn_timeout;
        }
        let bytes_read = stream.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        let command = String::from_utf8_lossy(&buffer[..bytes_read])
            .trim()
            .to_string();

        let parts: Vec<&str> = command.splitn(2, ' ').collect();
        let cmd = parts[0].to_uppercase();
        let arg = parts.get(1).map(|s| s.trim());

        let (allow_anonymous, anonymous_home) = {
            match config.lock() {
                Ok(guard) => (guard.ftp.allow_anonymous, guard.ftp.anonymous_home.clone()),
                Err(_) => (false, None),
            }
        };

        match cmd.as_str() {
            "USER" => {
                if let Some(username) = arg {
                    let username_lower = username.to_lowercase();
                    if username_lower == "anonymous" || username_lower == "ftp" {
                        if allow_anonymous {
                            current_user = Some("anonymous".to_string());
                            stream.write_all(b"331 Anonymous login okay, send email as password\r\n")?;
                        } else {
                            stream.write_all(b"530 Anonymous access not allowed\r\n")?;
                        }
                    } else {
                        current_user = Some(username.to_string());
                        stream.write_all(b"331 User name okay, need password\r\n")?;
                    }
                } else {
                    stream.write_all(b"501 Syntax error in parameters or arguments\r\n")?;
                }
            }

            "PASS" => {
                if let Some(ref username) = current_user {
                    if username == "anonymous" {
                        if allow_anonymous {
                            authenticated = true;
                            let user_home = if let Some(ref anon_home) = anonymous_home {
                                anon_home.clone()
                            } else {
                                config.lock()
                                    .map(|g| g.ftp.default_home.clone())
                                    .unwrap_or_else(|_| cwd.clone())
                            };
                            let home_canon = PathBuf::from(&user_home)
                                .canonicalize()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or(user_home);
                            cwd = home_canon.clone();
                            home_dir = home_canon;
                            stream.write_all(b"230 Anonymous user logged in\r\n")?;
                            if let Ok(mut logger_guard) = logger.lock() {
                                logger_guard.client_action(
                                    "FTP",
                                    "Anonymous user logged in",
                                    &remote_ip,
                                    Some("anonymous"),
                                    "LOGIN",
                                );
                            }
                        } else {
                            stream.write_all(b"530 Anonymous access not allowed\r\n")?;
                        }
                    } else {
                        let password = arg.unwrap_or("");
                        let auth_result = match user_manager.lock() {
                            Ok(mut users) => {
                                if users.get_user(username).is_none() {
                                    let _ = users.reload(&Config::get_users_path());
                                }
                                
                                users.authenticate(username, password)
                            }
                            Err(e) => {
                                Err(anyhow::anyhow!("获取用户管理器锁失败：{}", e))
                            }
                        };
                        
                        match auth_result {
                            Ok(true) => {
                                authenticated = true;
                                if let Ok(users) = user_manager.lock()
                                    && let Some(user) = users.get_user(username) {
                                        let home_canon = PathBuf::from(&user.home_dir)
                                            .canonicalize()
                                            .map(|p| p.to_string_lossy().to_string())
                                            .unwrap_or_else(|_| user.home_dir.clone());
                                        cwd = home_canon.clone();
                                        home_dir = home_canon;
                                    }
                                stream.write_all(b"230 User logged in\r\n")?;
                                if let Ok(mut logger_guard) = logger.lock() {
                                    logger_guard.client_action(
                                        "FTP",
                                        &format!("User {} logged in", username),
                                        &remote_ip,
                                        Some(username),
                                        "LOGIN",
                                    );
                                }
                            }
                            Ok(false) => {
                                if let Ok(mut logger_guard) = logger.lock() {
                                    logger_guard.client_action(
                                        "FTP",
                                        &format!("Authentication failed for user {}", username),
                                        &remote_ip,
                                        Some(username),
                                        "AUTH_FAIL",
                                    );
                                }
                                stream.write_all(b"530 Not logged in, user cannot be authenticated\r\n")?;
                            }
                            Err(e) => {
                                if let Ok(mut logger_guard) = logger.lock() {
                                    logger_guard.client_action(
                                        "FTP",
                                        &format!("Authentication error for user {}: {}", username, e),
                                        &remote_ip,
                                        Some(username),
                                        "AUTH_ERROR",
                                    );
                                }
                                stream.write_all(b"530 Not logged in\r\n")?;
                            }
                        }
                    }
                } else {
                    stream.write_all(b"530 Please login with USER and PASS\r\n")?;
                }
            }

            "QUIT" => {
                stream.write_all(b"221 Goodbye\r\n")?;
                break;
            }

            "SYST" => {
                stream.write_all(b"215 UNIX Type: L8\r\n")?;
            }

            "FEAT" => {
                stream.write_all(b"211-Features:\r\n SIZE\r\n MDTM\r\n REST STREAM\r\n PASV\r\n EPSV\r\n EPRT\r\n PORT\r\n MLST\r\n MLSD\r\n MODE S\r\n STRU F\r\n UTF8\r\n TVFS\r\n211 End\r\n")?;
            }

            "HELP" => {
                if let Some(cmd) = arg {
                    let help_text = match cmd.to_uppercase().as_str() {
                        "USER" => "214 USER <username>: Specify user name\r\n",
                        "PASS" => "214 PASS <password>: Specify password\r\n",
                        "CWD" => "214 CWD <directory>: Change working directory\r\n",
                        "CDUP" => "214 CDUP: Change to parent directory\r\n",
                        "PWD" => "214 PWD: Print working directory\r\n",
                        "LIST" => "214 LIST [<path>]: List directory contents\r\n",
                        "NLST" => "214 NLST [<path>]: List directory names\r\n",
                        "RETR" => "214 RETR <filename>: Retrieve file\r\n",
                        "STOR" => "214 STOR <filename>: Store file\r\n",
                        "DELE" => "214 DELE <filename>: Delete file\r\n",
                        "MKD" => "214 MKD <directory>: Create directory\r\n",
                        "RMD" => "214 RMD <directory>: Remove directory\r\n",
                        "RNFR" => "214 RNFR <filename>: Specify rename source\r\n",
                        "RNTO" => "214 RNTO <filename>: Specify rename destination\r\n",
                        "PASV" => "214 PASV: Enter passive mode\r\n",
                        "EPSV" => "214 EPSV: Enter extended passive mode\r\n",
                        "PORT" => "214 PORT <h1,h2,h3,h4,p1,p2>: Enter active mode\r\n",
                        "EPRT" => "214 EPRT |<netproto>|<netaddr>|<tcpport>|: Extended active mode\r\n",
                        "TYPE" => "214 TYPE <type>: Set transfer type (A/I)\r\n",
                        "MODE" => "214 MODE <mode>: Set transfer mode (S/B/C)\r\n",
                        "STRU" => "214 STRU <structure>: Set file structure (F/R/P)\r\n",
                        "REST" => "214 REST <offset>: Set restart marker\r\n",
                        "SIZE" => "214 SIZE <filename>: Get file size\r\n",
                        "MDTM" => "214 MDTM <filename>: Get modification time\r\n",
                        "ABOR" => "214 ABOR: Abort current transfer\r\n",
                        "QUIT" => "214 QUIT: Disconnect from server\r\n",
                        _ => "214 Unknown command\r\n",
                    };
                    stream.write_all(help_text.as_bytes())?;
                } else {
                    stream.write_all(b"214-The following commands are recognized:\r\n")?;
                    stream.write_all(b"214-USER PASS ACCT CWD CDUP PWD LIST NLST RETR STOR\r\n")?;
                    stream.write_all(b"214-DELE MKD RMD RNFR RNTO PASV EPSV PORT EPRT\r\n")?;
                    stream.write_all(b"214-TYPE MODE STRU REST SIZE MDTM ABOR QUIT REIN\r\n")?;
                    stream.write_all(b"214-MLSD MLST SYST FEAT STAT HELP NOOP STOU SITE\r\n")?;
                    stream.write_all(b"214 Direct comments to admin\r\n")?;
                }
            }

            "MODE" => {
                if let Some(mode) = arg {
                    match mode.to_uppercase().as_str() {
                        "S" => {
                            stream.write_all(b"200 Mode set to Stream\r\n")?;
                        }
                        "B" => {
                            stream.write_all(b"504 Block mode not supported\r\n")?;
                        }
                        "C" => {
                            stream.write_all(b"504 Compressed mode not supported\r\n")?;
                        }
                        _ => {
                            stream.write_all(b"501 Unknown mode\r\n")?;
                        }
                    }
                } else {
                    stream.write_all(b"501 Syntax error: MODE requires parameter\r\n")?;
                }
            }

            "STRU" => {
                if let Some(structure) = arg {
                    match structure.to_uppercase().as_str() {
                        "F" => {
                            stream.write_all(b"200 Structure set to File\r\n")?;
                        }
                        "R" => {
                            stream.write_all(b"504 Record structure not supported\r\n")?;
                        }
                        "P" => {
                            stream.write_all(b"504 Page structure not supported\r\n")?;
                        }
                        _ => {
                            stream.write_all(b"501 Unknown structure\r\n")?;
                        }
                    }
                } else {
                    stream.write_all(b"501 Syntax error: STRU requires parameter\r\n")?;
                }
            }

            "ALLO" => {
                stream.write_all(b"200 ALLO command successful\r\n")?;
            }

            "OPTS" => {
                stream.write_all(b"200 Options set\r\n")?;
            }

            "PWD" | "XPWD" => {
                stream.write_all(format!("257 \"{}\"\r\n", cwd).as_bytes())?;
            }

            "CWD" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                if let Some(dir) = arg {
                    let new_path = safe_resolve_path(&cwd, &home_dir, dir);

                    if new_path.exists() && new_path.is_dir() && new_path.starts_with(&home_dir) {
                        cwd = new_path.to_string_lossy().to_string();
                        stream.write_all(b"250 Directory successfully changed\r\n")?;
                    } else {
                        stream.write_all(b"550 Failed to change directory\r\n")?;
                    }
                } else {
                    stream.write_all(b"501 Syntax error: CWD requires directory parameter\r\n")?;
                }
            }

            "CDUP" | "XCUP" => {
                let new_path = safe_resolve_path(&cwd, &home_dir, "..");
                if new_path.starts_with(&home_dir) && new_path.exists() {
                    cwd = new_path.to_string_lossy().to_string();
                    stream.write_all(b"250 Directory changed\r\n")?;
                } else {
                    stream.write_all(b"550 Cannot change to parent directory: Permission denied\r\n")?;
                }
            }

            "TYPE" => {
                if let Some(type_code) = arg {
                    let type_upper = type_code.to_uppercase();
                    let parts: Vec<&str> = type_upper.split_whitespace().collect();
                    let main_type = parts.first().copied().unwrap_or("");
                    let sub_type = parts.get(1).copied().unwrap_or("N");

                    match main_type {
                        "I" => {
                            transfer_mode = "binary".to_string();
                            stream.write_all(b"200 Type set to I (Binary)\r\n")?;
                        }
                        "L" => {
                            if sub_type == "8" {
                                transfer_mode = "binary".to_string();
                                stream.write_all(b"200 Type set to L 8 (Local byte size 8)\r\n")?;
                            } else {
                                stream.write_all(b"504 Only L 8 is supported\r\n")?;
                            }
                        }
                        "A" => {
                            match sub_type {
                                "N" | "" => {
                                    transfer_mode = "ascii".to_string();
                                    stream.write_all(b"200 Type set to A (ASCII Non-print)\r\n")?;
                                }
                                "T" => {
                                    transfer_mode = "ascii".to_string();
                                    stream.write_all(b"200 Type set to A T (ASCII Telnet format)\r\n")?;
                                }
                                "C" => {
                                    stream.write_all(b"504 ASA carriage control not supported\r\n")?;
                                }
                                _ => {
                                    stream.write_all(b"501 Unknown subtype\r\n")?;
                                }
                            }
                        }
                        "E" => {
                            stream.write_all(b"504 EBCDIC not supported, use A or I\r\n")?;
                        }
                        _ => {
                            stream.write_all(b"501 Unknown type\r\n")?;
                        }
                    }
                } else {
                    if transfer_mode == "binary" {
                        stream.write_all(b"200 Type is I (Binary)\r\n")?;
                    } else {
                        stream.write_all(b"200 Type is A (ASCII)\r\n")?;
                    }
                }
            }

            "MLST" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                let target_path = if let Some(path_arg) = arg {
                    safe_resolve_path(&cwd, &home_dir, path_arg)
                } else {
                    Path::new(&cwd).to_path_buf()
                };

                if target_path.exists() && target_path.starts_with(&home_dir) {
                    if let Ok(metadata) = target_path.metadata() {
                        let facts = build_mlst_facts(&metadata);
                        let name = target_path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| target_path.to_string_lossy().to_string());
                        stream.write_all(format!("250-Listing {}\r\n {} {}\r\n250 End\r\n", target_path.display(), facts, name).as_bytes())?;
                    } else {
                        stream.write_all(b"550 Failed to get file info\r\n")?;
                    }
                } else {
                    stream.write_all(b"550 File not found\r\n")?;
                }
            }

            "REST" => {
                if let Some(offset_str) = arg {
                    if let Ok(offset) = offset_str.parse::<u64>() {
                        rest_offset = offset;
                        stream.write_all(format!("350 Restarting at {}\r\n", offset).as_bytes())?;
                        if let Ok(mut log) = logger.lock() {
                            log.client_action(
                                "FTP",
                                &format!("REST command: offset {}", offset),
                                &remote_ip,
                                current_user.as_deref(),
                                "REST",
                            );
                        }
                    } else {
                        stream.write_all(b"501 Syntax error in REST parameter\r\n")?;
                    }
                } else {
                    rest_offset = 0;
                    stream.write_all(b"350 Restarting at 0\r\n")?;
                }
            }

            "PASV" => {
                passive_mode = true;
                let ((port_min, port_max), bind_ip) = 
                    match config.lock() {
                        Ok(guard) => (guard.ftp.passive_ports, guard.ftp.bind_ip.clone()),
                        Err(_) => {
                            stream.write_all(b"500 Internal server error\r\n")?;
                            continue;
                        }
                    };

                let passive_port = match find_available_passive_port(passive_listeners, port_min, port_max) {
                    Ok(port) => port,
                    Err(e) => {
                        stream.write_all(format!("425 Could not enter passive mode: {}\r\n", e).as_bytes())?;
                        continue;
                    }
                };

                let passive_listener = match TcpListener::bind(format!("{}:{}", bind_ip, passive_port)) {
                    Ok(l) => l,
                    Err(e) => {
                        stream.write_all(format!("425 Could not bind passive port: {}\r\n", e).as_bytes())?;
                        continue;
                    }
                };
                if passive_listener.set_nonblocking(true).is_err() {
                    stream.write_all(b"425 Could not set non-blocking mode\r\n")?;
                    continue;
                }

                {
                    let mut listeners = match passive_listeners.lock() {
                        Ok(guard) => guard,
                        Err(_) => {
                            stream.write_all(b"500 Internal server error\r\n")?;
                            continue;
                        }
                    };
                    listeners.insert(passive_port, Arc::new(Mutex::new(Some(passive_listener))));
                }

                data_port = Some(passive_port);

                let response_ip = if bind_ip == "0.0.0.0" || bind_ip.is_empty() {
                    remote_ip.clone()
                } else {
                    bind_ip.clone()
                };
                
                let ip_parts: Vec<&str> = response_ip.split('.').collect();
                if ip_parts.len() != 4 {
                    stream.write_all(b"425 Invalid IP address format\r\n")?;
                    continue;
                }
                
                let p1 = passive_port >> 8;
                let p2 = passive_port & 0xFF;
                
                stream.write_all(
                    format!(
                        "227 Entering Passive Mode ({},{},{},{},{},{}).\r\n",
                        ip_parts[0], ip_parts[1], ip_parts[2], ip_parts[3], p1, p2
                    )
                    .as_bytes(),
                )?;

                if let Ok(mut logger) = logger.lock() {
                    logger.client_action(
                        "FTP",
                        &format!("PASV mode: port {}", passive_port),
                        &remote_ip,
                        current_user.as_deref(),
                        "PASV",
                    );
                }
            }

            "EPSV" => {
                passive_mode = true;
                let ((port_min, port_max), bind_ip) = 
                    match config.lock() {
                        Ok(guard) => (guard.ftp.passive_ports, guard.ftp.bind_ip.clone()),
                        Err(_) => {
                            stream.write_all(b"500 Internal server error\r\n")?;
                            continue;
                        }
                    };

                let passive_port = match find_available_passive_port(passive_listeners, port_min, port_max) {
                    Ok(port) => port,
                    Err(e) => {
                        stream.write_all(format!("425 Could not enter extended passive mode: {}\r\n", e).as_bytes())?;
                        continue;
                    }
                };

                let passive_listener = match TcpListener::bind(format!("{}:{}", bind_ip, passive_port)) {
                    Ok(l) => l,
                    Err(e) => {
                        stream.write_all(format!("425 Could not bind passive port: {}\r\n", e).as_bytes())?;
                        continue;
                    }
                };
                if passive_listener.set_nonblocking(true).is_err() {
                    stream.write_all(b"425 Could not set non-blocking mode\r\n")?;
                    continue;
                }

                {
                    let mut listeners = match passive_listeners.lock() {
                        Ok(guard) => guard,
                        Err(_) => {
                            stream.write_all(b"500 Internal server error\r\n")?;
                            continue;
                        }
                    };
                    listeners.insert(passive_port, Arc::new(Mutex::new(Some(passive_listener))));
                }

                data_port = Some(passive_port);
                stream.write_all(
                    format!("229 Entering Extended Passive Mode (|||{}|)\r\n", passive_port).as_bytes(),
                )?;
            }

            "PORT" => {
                if let Some(data) = arg {
                    let parts: Vec<u16> = data.split(',').filter_map(|s| s.parse().ok()).collect();
                    if parts.len() == 6 {
                        let port = parts[4] * 256 + parts[5];
                        let addr = format!("{}.{}.{}.{}:{}", parts[0], parts[1], parts[2], parts[3], port);
                        data_port = Some(port);
                        data_addr = Some(addr);
                        passive_mode = false;
                        stream.write_all(b"200 PORT command successful\r\n")?;
                    } else {
                        stream.write_all(b"501 Syntax error in parameters or arguments\r\n")?;
                    }
                } else {
                    stream.write_all(b"501 Syntax error: PORT requires parameters\r\n")?;
                }
            }

            "EPRT" => {
                if let Some(data) = arg {
                    let parts: Vec<&str> = data.split('|').collect();
                    if parts.len() >= 4 {
                        let net_proto = parts[1];
                        let net_addr = parts[2];
                        let tcp_port = parts[3];

                        match net_proto {
                            "1" => {
                                if let Ok(port) = tcp_port.parse::<u16>() {
                                    data_port = Some(port);
                                    data_addr = Some(format!("{}:{}", net_addr, port));
                                    passive_mode = false;
                                    stream.write_all(b"200 EPRT command successful\r\n")?;
                                } else {
                                    stream.write_all(b"501 Invalid port number\r\n")?;
                                }
                            }
                            "2" => {
                                if let Ok(port) = tcp_port.parse::<u16>() {
                                    data_port = Some(port);
                                    data_addr = Some(format!("[{}]:{}", net_addr, port));
                                    passive_mode = false;
                                    stream.write_all(b"200 EPRT command successful (IPv6)\r\n")?;
                                } else {
                                    stream.write_all(b"501 Invalid port number\r\n")?;
                                }
                            }
                            _ => {
                                stream.write_all(b"522 Protocol not supported, use (1,2)\r\n")?;
                            }
                        }
                    } else {
                        stream.write_all(b"501 Syntax error in EPRT parameters\r\n")?;
                    }
                } else {
                    stream.write_all(b"501 Syntax error: EPRT requires parameters\r\n")?;
                }
            }

            "PBSZ" => {
                stream.write_all(b"200 PBSZ=0\r\n")?;
            }

            "PROT" => {
                if let Some(level) = arg {
                    match level.to_uppercase().as_str() {
                        "P" => {
                            stream.write_all(b"200 PROT Private\r\n")?;
                        }
                        "C" => {
                            stream.write_all(b"200 PROT Clear\r\n")?;
                        }
                        _ => {
                            stream.write_all(b"504 PROT level not supported\r\n")?;
                        }
                    }
                }
            }

            "LIST" | "NLST" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                {
                    let users = match user_manager.lock() {
                        Ok(guard) => guard,
                        Err(_) => {
                            stream.write_all(b"500 Internal server error\r\n")?;
                            continue;
                        }
                    };
                    let user = current_user.as_ref().and_then(|u| users.get_user(u));
                    if let Some(user) = user
                        && !user.permissions.can_list {
                            stream.write_all(b"550 Permission denied\r\n")?;
                            continue;
                        }
                }

                stream.write_all(b"150 Here comes the directory listing\r\n")?;

                if let Ok(mut data_stream) = get_data_connection(passive_mode, data_port, &data_addr, &remote_ip, passive_listeners) {
                    let path = Path::new(&cwd);
                    if let Ok(entries) = std::fs::read_dir(path) {
                        for entry in entries.flatten() {
                            if let Ok(metadata) = entry.metadata() {
                                let name = entry.file_name().to_string_lossy().to_string();
                                let perms = if metadata.is_dir() {
                                    "drwxr-xr-x"
                                } else {
                                    "-rw-r--r--"
                                };
                                let size = metadata.len();
                                let mtime = get_file_mtime(&metadata);
                                let line = format!(
                                    "{} 1 user user {:>10} {} {}\r\n",
                                    perms, size, mtime, name
                                );
                                let _ = data_stream.write_all(line.as_bytes());
                            }
                        }
                    }
                }

                if passive_mode
                    && let Some(port) = data_port {
                        let mut listeners = match passive_listeners.lock() {
                            Ok(guard) => guard,
                            Err(e) => {
                                if let Ok(mut logger) = logger.try_lock() {
                                    logger.warning("FTP", &format!("Failed to lock passive listeners: {}", e));
                                }
                                continue;
                            }
                        };
                        listeners.remove(&port);
                    }

                stream.write_all(b"226 Transfer complete\r\n")?;
            }

            "MLSD" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                {
                    let users = user_manager.lock().unwrap();
                    let user = current_user.as_ref().and_then(|u| users.get_user(u));
                    if let Some(user) = user
                        && !user.permissions.can_list {
                            stream.write_all(b"550 Permission denied\r\n")?;
                            continue;
                        }
                }

                stream.write_all(b"150 Here comes the directory listing\r\n")?;

                if let Ok(mut data_stream) = get_data_connection(passive_mode, data_port, &data_addr, &remote_ip, passive_listeners) {
                    let path = Path::new(&cwd);
                    if let Ok(entries) = std::fs::read_dir(path) {
                        for entry in entries.flatten() {
                            if let Ok(metadata) = entry.metadata() {
                                let name = entry.file_name().to_string_lossy().to_string();
                                let facts = build_mlst_facts(&metadata);
                                let line = format!("{} {}\r\n", facts, name);
                                let _ = data_stream.write_all(line.as_bytes());
                            }
                        }
                    }
                }

                if passive_mode
                    && let Some(port) = data_port {
                        let mut listeners = passive_listeners.lock().unwrap();
                        listeners.remove(&port);
                    }

                stream.write_all(b"226 Transfer complete\r\n")?;
            }

            "RETR" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                if let Some(filename) = arg {
                    let file_path = safe_resolve_path(&cwd, &home_dir, filename);

                    if !file_path.exists() || !file_path.is_file() || !file_path.starts_with(&home_dir) {
                        stream.write_all(b"550 File not found\r\n")?;
                        continue;
                    }

                    {
                        let users = user_manager.lock().unwrap();
                        let user = current_user.as_ref().and_then(|u| users.get_user(u));

                        if let Some(user) = user
                            && !user.permissions.can_read {
                                stream.write_all(b"550 Permission denied\r\n")?;
                                continue;
                            }
                    }

                    let file_size = std::fs::metadata(&file_path)?.len();
                    let remaining = if rest_offset > 0 && rest_offset < file_size {
                        file_size - rest_offset
                    } else {
                        file_size
                    };

                    stream.write_all(
                        format!("150 Opening BINARY mode data connection ({} bytes)\r\n", remaining)
                            .as_bytes(),
                    )?;

                    if let Ok(mut data_stream) = get_data_connection(passive_mode, data_port, &data_addr, &remote_ip, passive_listeners) {
                        let abort = Arc::clone(&abort_flag);
                        if let Ok(mut file) = std::fs::File::open(&file_path) {
                            use std::io::Seek;
                            if rest_offset > 0 {
                                let _ = file.seek(std::io::SeekFrom::Start(rest_offset));
                            }

                            let mut buf = [0u8; 8192];
                            loop {
                                if abort.load(Ordering::Relaxed) {
                                    break;
                                }
                                match file.read(&mut buf) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if data_stream.write_all(&buf[..n]).is_err() {
                                            break;
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                    }

                    if passive_mode
                        && let Some(port) = data_port {
                            let mut listeners = passive_listeners.lock().unwrap();
                            listeners.remove(&port);
                        }

                    stream.write_all(b"226 Transfer complete\r\n")?;

                    let file_size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(remaining);
                    file_logger.lock().unwrap().log_download(
                        current_user.as_deref().unwrap_or("anonymous"),
                        &remote_ip,
                        &file_path.to_string_lossy(),
                        file_size,
                        "FTP",
                    );

                    logger.lock().unwrap().client_action(
                        "FTP",
                        &format!(
                            "Downloaded: {} ({} bytes from offset {})",
                            filename, remaining, rest_offset
                        ),
                        &remote_ip,
                        current_user.as_deref(),
                        "DOWNLOAD",
                    );

                    rest_offset = 0;
                }
            }

            "STOR" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                if let Some(filename) = arg {
                    {
                        let users = user_manager.lock().unwrap();
                        let user = current_user.as_ref().and_then(|u| users.get_user(u));

                        if let Some(user) = user
                            && !user.permissions.can_write {
                                stream.write_all(b"550 Permission denied\r\n")?;
                                continue;
                            }
                    }

                    let file_path = safe_resolve_path(&cwd, &home_dir, filename);
                    if !file_path.starts_with(&home_dir) {
                        stream.write_all(b"550 Permission denied\r\n")?;
                        continue;
                    }
                    let file_existed = file_path.exists();
                    stream.write_all(b"150 Opening BINARY mode data connection\r\n")?;

                    if let Ok(mut data_stream) = get_data_connection(passive_mode, data_port, &data_addr, &remote_ip, passive_listeners) {
                        let abort = Arc::clone(&abort_flag);
                        let file_result = if rest_offset > 0 {
                            std::fs::OpenOptions::new()
                                .write(true)
                                .create(true)
                                .truncate(false)
                                .open(&file_path)
                        } else {
                            std::fs::File::create(&file_path)
                        };

                        if let Ok(mut file) = file_result {
                            use std::io::Seek;
                            if rest_offset > 0 {
                                let _ = file.seek(std::io::SeekFrom::Start(rest_offset));
                            }

                            let mut buf = [0u8; 8192];
                            loop {
                                if abort.load(Ordering::Relaxed) {
                                    break;
                                }
                                match data_stream.read(&mut buf) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if file.write_all(&buf[..n]).is_err() {
                                            break;
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                    }

                    if passive_mode
                        && let Some(port) = data_port {
                            let mut listeners = passive_listeners.lock().unwrap();
                            listeners.remove(&port);
                        }

                    stream.write_all(b"226 Transfer complete\r\n")?;

                    let uploaded_size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
                    if file_existed {
                        file_logger.lock().unwrap().log_update(
                            current_user.as_deref().unwrap_or("anonymous"),
                            &remote_ip,
                            &file_path.to_string_lossy(),
                            uploaded_size,
                            "FTP",
                        );
                    } else {
                        file_logger.lock().unwrap().log_upload(
                            current_user.as_deref().unwrap_or("anonymous"),
                            &remote_ip,
                            &file_path.to_string_lossy(),
                            uploaded_size,
                            "FTP",
                        );
                    }

                    logger.lock().unwrap().client_action(
                        "FTP",
                        &format!("Uploaded: {} at offset {}", filename, rest_offset),
                        &remote_ip,
                        current_user.as_deref(),
                        "UPLOAD",
                    );

                    rest_offset = 0;
                }
            }

            "APPE" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                if let Some(filename) = arg {
                    {
                        let users = user_manager.lock().unwrap();
                        let user = current_user.as_ref().and_then(|u| users.get_user(u));

                        if let Some(user) = user
                            && !user.permissions.can_append {
                                stream.write_all(b"550 Permission denied\r\n")?;
                                continue;
                            }
                    }

                    let file_path = safe_resolve_path(&cwd, &home_dir, filename);
                    if !file_path.starts_with(&home_dir) {
                        stream.write_all(b"550 Permission denied\r\n")?;
                        continue;
                    }
                    stream.write_all(b"150 Opening BINARY mode data connection for append\r\n")?;

                    if let Ok(mut data_stream) = get_data_connection(passive_mode, data_port, &data_addr, &remote_ip, passive_listeners) {
                        let abort = Arc::clone(&abort_flag);
                        if let Ok(mut file) = std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(&file_path)
                        {
                            let mut buf = [0u8; 8192];
                            loop {
                                if abort.load(Ordering::Relaxed) {
                                    break;
                                }
                                match data_stream.read(&mut buf) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if file.write_all(&buf[..n]).is_err() {
                                            break;
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                    }

                    if passive_mode
                        && let Some(port) = data_port {
                            let mut listeners = passive_listeners.lock().unwrap();
                            listeners.remove(&port);
                        }

                    stream.write_all(b"226 Transfer complete\r\n")?;

                    let appended_size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
                    file_logger.lock().unwrap().log(FileLogInfo {
                        username: current_user.as_deref().unwrap_or("anonymous"),
                        client_ip: &remote_ip,
                        operation: "APPEND",
                        file_path: &file_path.to_string_lossy(),
                        file_size: appended_size,
                        protocol: "FTP",
                        success: true,
                        message: "文件追加成功",
                    });

                    logger.lock().unwrap().client_action(
                        "FTP",
                        &format!("Appended: {}", filename),
                        &remote_ip,
                        current_user.as_deref(),
                        "APPEND",
                    );
                }
            }

            "DELE" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                {
                    let users = user_manager.lock().unwrap();
                    let user = current_user.as_ref().and_then(|u| users.get_user(u));

                    if let Some(user) = user
                        && !user.permissions.can_delete {
                            stream.write_all(b"550 Permission denied\r\n")?;
                            continue;
                        }
                }

                if let Some(filename) = arg {
                    let file_path = safe_resolve_path(&cwd, &home_dir, filename);
                    if !file_path.starts_with(&home_dir) {
                        stream.write_all(b"550 Permission denied\r\n")?;
                        continue;
                    }
                    if std::fs::remove_file(&file_path).is_ok() {
                        stream.write_all(b"250 File deleted\r\n")?;
                        file_logger.lock().unwrap().log_delete(
                            current_user.as_deref().unwrap_or("anonymous"),
                            &remote_ip,
                            &file_path.to_string_lossy(),
                            "FTP",
                        );
                        logger.lock().unwrap().client_action(
                            "FTP",
                            &format!("Deleted: {}", filename),
                            &remote_ip,
                            current_user.as_deref(),
                            "DELETE",
                        );
                    } else {
                        stream.write_all(b"550 Delete operation failed\r\n")?;
                    }
                }
            }

            "MKD" | "XMKD" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                {
                    let users = user_manager.lock().unwrap();
                    let user = current_user.as_ref().and_then(|u| users.get_user(u));

                    if let Some(user) = user
                        && !user.permissions.can_mkdir {
                            stream.write_all(b"550 Permission denied\r\n")?;
                            continue;
                        }
                }

                if let Some(dirname) = arg {
                    let dir_path = safe_resolve_path(&cwd, &home_dir, dirname);
                    if !dir_path.starts_with(&home_dir) {
                        stream.write_all(b"550 Permission denied\r\n")?;
                        continue;
                    }
                    if std::fs::create_dir_all(&dir_path).is_ok() {
                        stream.write_all(format!("257 \"{}\" created\r\n", dir_path.display()).as_bytes())?;
                        file_logger.lock().unwrap().log_mkdir(
                            current_user.as_deref().unwrap_or("anonymous"),
                            &remote_ip,
                            &dir_path.to_string_lossy(),
                            "FTP",
                        );
                        logger.lock().unwrap().client_action(
                            "FTP",
                            &format!("Created directory: {}", dirname),
                            &remote_ip,
                            current_user.as_deref(),
                            "MKDIR",
                        );
                    } else {
                        stream.write_all(b"550 Create directory operation failed\r\n")?;
                    }
                }
            }

            "RMD" | "XRMD" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                {
                    let users = user_manager.lock().unwrap();
                    let user = current_user.as_ref().and_then(|u| users.get_user(u));

                    if let Some(user) = user
                        && !user.permissions.can_rmdir {
                            stream.write_all(b"550 Permission denied\r\n")?;
                            continue;
                        }
                }

                if let Some(dirname) = arg {
                    let dir_path = safe_resolve_path(&cwd, &home_dir, dirname);
                    if !dir_path.starts_with(&home_dir) {
                        stream.write_all(b"550 Permission denied\r\n")?;
                        continue;
                    }
                    if std::fs::remove_dir_all(&dir_path).is_ok() {
                        stream.write_all(b"250 Directory removed\r\n")?;
                        file_logger.lock().unwrap().log_rmdir(
                            current_user.as_deref().unwrap_or("anonymous"),
                            &remote_ip,
                            &dir_path.to_string_lossy(),
                            "FTP",
                        );
                        logger.lock().unwrap().client_action(
                            "FTP",
                            &format!("Removed directory: {}", dirname),
                            &remote_ip,
                            current_user.as_deref(),
                            "RMDIR",
                        );
                    } else {
                        stream.write_all(b"550 Remove directory operation failed\r\n")?;
                    }
                }
            }

            "RNFR" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                {
                    let users = user_manager.lock().unwrap();
                    let user = current_user.as_ref().and_then(|u| users.get_user(u));

                    if let Some(user) = user
                        && !user.permissions.can_rename {
                            stream.write_all(b"550 Permission denied\r\n")?;
                            continue;
                        }
                }

                if let Some(from_name) = arg {
                    let from_path = safe_resolve_path(&cwd, &home_dir, from_name);
                    if from_path.exists() && from_path.starts_with(&home_dir) {
                        rename_from = Some(from_path.to_string_lossy().to_string());
                        stream.write_all(b"350 File exists, ready for destination name\r\n")?;
                    } else {
                        stream.write_all(b"550 File not found\r\n")?;
                    }
                }
            }

            "RNTO" => {
                if let Some(ref from_path) = rename_from {
                    if let Some(to_name) = arg {
                        let to_path = safe_resolve_path(&cwd, &home_dir, to_name);
                        if !to_path.starts_with(&home_dir) {
                            stream.write_all(b"550 Permission denied\r\n")?;
                            rename_from = None;
                            continue;
                        }
                        if std::fs::rename(from_path, &to_path).is_ok() {
                            stream.write_all(b"250 Rename successful\r\n")?;
                            file_logger.lock().unwrap().log_rename(
                                current_user.as_deref().unwrap_or("anonymous"),
                                &remote_ip,
                                from_path,
                                &to_path.to_string_lossy(),
                                "FTP",
                            );
                            logger.lock().unwrap().client_action(
                                "FTP",
                                &format!("Renamed: {} -> {}", from_path, to_path.display()),
                                &remote_ip,
                                current_user.as_deref(),
                                "RENAME",
                            );
                        } else {
                            stream.write_all(b"550 Rename failed\r\n")?;
                        }
                    }
                } else {
                    stream.write_all(b"503 Bad sequence of commands\r\n")?;
                }
                rename_from = None;
            }

            "SIZE" => {
                if let Some(filename) = arg {
                    let file_path = safe_resolve_path(&cwd, &home_dir, filename);
                    if file_path.starts_with(&home_dir) {
                        if let Ok(metadata) = std::fs::metadata(&file_path) {
                            stream.write_all(format!("213 {}\r\n", metadata.len()).as_bytes())?;
                        } else {
                            stream.write_all(b"550 File not found\r\n")?;
                        }
                    } else {
                        stream.write_all(b"550 Permission denied\r\n")?;
                    }
                }
            }

            "MDTM" => {
                if let Some(filename) = arg {
                    let file_path = safe_resolve_path(&cwd, &home_dir, filename);
                    if file_path.starts_with(&home_dir) {
                        if let Ok(metadata) = std::fs::metadata(&file_path) {
                            let mtime = get_file_mtime_raw(&metadata);
                            stream.write_all(format!("213 {}\r\n", mtime).as_bytes())?;
                        } else {
                            stream.write_all(b"550 File not found\r\n")?;
                        }
                    } else {
                        stream.write_all(b"550 Permission denied\r\n")?;
                    }
                }
            }

            "NOOP" => {
                stream.write_all(b"200 OK\r\n")?;
            }

            "STAT" => {
                if let Some(ref username) = current_user {
                    stream.write_all(b"211-FTP server status:\r\n")?;
                    stream.write_all(format!("211-Connected to: {}\r\n", remote_ip).as_bytes())?;
                    stream.write_all(format!("211-Logged in as: {}\r\n", username).as_bytes())?;
                    stream.write_all(format!("211-Current directory: {}\r\n", cwd).as_bytes())?;
                    stream.write_all(format!("211-Transfer mode: {}\r\n", if passive_mode { "Passive" } else { "Active" }).as_bytes())?;
                    stream.write_all(b"211 End\r\n")?;
                } else {
                    stream.write_all(b"211 FTP server status - Not logged in\r\n")?;
                }
            }

            "ABOR" => {
                abort_flag.store(true, Ordering::Relaxed);
                stream.write_all(b"426 Connection closed; transfer aborted\r\n")?;
                stream.write_all(b"226 Abort successful\r\n")?;
            }

            "STOU" => {
                if !authenticated {
                    stream.write_all(b"530 Not logged in\r\n")?;
                    continue;
                }

                {
                    let users = user_manager.lock().unwrap();
                    let user = current_user.as_ref().and_then(|u| users.get_user(u));

                    if let Some(user) = user
                        && !user.permissions.can_write {
                            stream.write_all(b"550 Permission denied\r\n")?;
                            continue;
                        }
                }

                // Generate unique filename
                let unique_name = format!("stou_{}_{}", 
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    rand::random::<u32>()
                );
                
                let file_path = safe_resolve_path(&cwd, &home_dir, &unique_name);
                if !file_path.starts_with(&home_dir) {
                    stream.write_all(b"550 Permission denied\r\n")?;
                    continue;
                }

                stream.write_all(format!("150 FILE: {}\r\n", unique_name).as_bytes())?;

                if let Ok(mut data_stream) = get_data_connection(passive_mode, data_port, &data_addr, &remote_ip, passive_listeners) {
                    let abort = Arc::clone(&abort_flag);
                    if let Ok(mut file) = std::fs::File::create(&file_path) {
                        let mut buf = [0u8; 8192];
                        loop {
                            if abort.load(Ordering::Relaxed) {
                                break;
                            }
                            match data_stream.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    if file.write_all(&buf[..n]).is_err() {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }

                if passive_mode
                    && let Some(port) = data_port {
                        let mut listeners = passive_listeners.lock().unwrap();
                        listeners.remove(&port);
                    }

                stream.write_all(b"226 Transfer complete\r\n")?;

                let uploaded_size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
                file_logger.lock().unwrap().log_upload(
                    current_user.as_deref().unwrap_or("anonymous"),
                    &remote_ip,
                    &file_path.to_string_lossy(),
                    uploaded_size,
                    "FTP",
                );

                logger.lock().unwrap().client_action(
                    "FTP",
                    &format!("Uploaded unique file: {}", unique_name),
                    &remote_ip,
                    current_user.as_deref(),
                    "UPLOAD",
                );
            }

            "REIN" => {
                authenticated = false;
                current_user = None;
                {
                    let cfg = match config.lock() {
                        Ok(guard) => guard,
                        Err(_) => {
                            stream.write_all(b"500 Internal server error\r\n")?;
                            continue;
                        }
                    };
                    let home_canon = PathBuf::from(&cfg.ftp.default_home)
                        .canonicalize()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| cfg.ftp.default_home.clone());
                    cwd = home_canon.clone();
                    home_dir = home_canon;
                }
                data_port = None;
                data_addr = None;
                passive_mode = false;
                rest_offset = 0;
                rename_from = None;
                stream.write_all(b"220 Service ready for new user\r\n")?;
            }

            "ACCT" => {
                // Account information - not required for this implementation
                stream.write_all(b"202 Account not required\r\n")?;
            }

            "SITE" => {
                if let Some(site_cmd) = arg {
                    let site_parts: Vec<&str> = site_cmd.splitn(2, ' ').collect();
                    let site_action = site_parts[0].to_uppercase();
                    let site_arg = site_parts.get(1).map(|s| s.trim());

                    match site_action.as_str() {
                        "HELP" => {
                            stream.write_all(b"214-The following SITE commands are recognized:\r\n")?;
                            stream.write_all(b"214-CHMOD IDLE HELP\r\n")?;
                            stream.write_all(b"214 End\r\n")?;
                        }
                        "IDLE" => {
                            if let Some(secs_str) = site_arg {
                                if let Ok(secs) = secs_str.parse::<u64>() {
                                    stream.write_all(format!("200 Idle timeout set to {} seconds\r\n", secs).as_bytes())?;
                                } else {
                                    stream.write_all(b"501 Invalid idle time\r\n")?;
                                }
                            } else {
                                stream.write_all(b"501 SITE IDLE requires time parameter\r\n")?;
                            }
                        }
                        "CHMOD" => {
                            stream.write_all(b"502 CHMOD not implemented\r\n")?;
                        }
                        _ => {
                            stream.write_all(format!("500 Unknown SITE command: {}\r\n", site_action).as_bytes())?;
                        }
                    }
                } else {
                    stream.write_all(b"501 SITE command requires parameter\r\n")?;
                }
            }

            _ => {
                stream.write_all(b"202 Command not implemented\r\n")?;
            }
        }
    }

    Ok(())
}

fn find_available_passive_port(
    passive_listeners: &PassiveListenerMap,
    port_min: u16,
    port_max: u16,
) -> Result<u16> {
    let listeners = passive_listeners.lock().unwrap();

    for port in port_min..=port_max {
        if !listeners.contains_key(&port) {
            return Ok(port);
        }
    }

    anyhow::bail!(
        "No available passive ports in range {}-{}",
        port_min,
        port_max
    )
}

fn get_data_connection(
    passive_mode: bool,
    data_port: Option<u16>,
    data_addr: &Option<String>,
    remote_ip: &str,
    passive_listeners: &PassiveListenerMap,
) -> Result<TcpStream> {
    let port = match data_port {
        Some(p) => p,
        None => anyhow::bail!("No data port specified"),
    };

    if passive_mode {
        let listener_arc = {
            let listeners = passive_listeners.lock().unwrap();
            listeners.get(&port).cloned()
        };

        if let Some(listener_arc) = listener_arc {
            let mut listener_guard = listener_arc.lock().unwrap();
            if let Some(listener) = listener_guard.take() {
                listener.set_nonblocking(false)?;
                let result = match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        Ok(stream)
                    }
                    Err(e) => Err(anyhow::anyhow!("Failed to accept passive connection: {}", e))
                };
                if result.is_err() {
                    let mut listeners = passive_listeners.lock().unwrap();
                    listeners.remove(&port);
                }
                result
            } else {
                let mut listeners = passive_listeners.lock().unwrap();
                listeners.remove(&port);
                anyhow::bail!("No passive listener")
            }
        } else {
            anyhow::bail!("No passive listener")
        }
    } else if let Some(addr) = data_addr {
        let socket_addr = addr.to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("Invalid address {}: {}", addr, e))?
            .next()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve address: {}", addr))?;
        let stream = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(30))?;
        Ok(stream)
    } else {
        let addr = format!("{}:{}", remote_ip, port);
        let socket_addr = addr.to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("Invalid address {}: {}", addr, e))?
            .next()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve address: {}", addr))?;
        let stream = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(30))?;
        Ok(stream)
    }
}

fn get_file_mtime(metadata: &std::fs::Metadata) -> String {
    if let Ok(time) = metadata.modified() {
        let dt: chrono::DateTime<chrono::Local> = time.into();
        return dt.format("%Y-%m-%d %H:%M").to_string();
    }
    "1970-01-01 00:00".to_string()
}

fn get_file_mtime_raw(metadata: &std::fs::Metadata) -> String {
    use std::time::UNIX_EPOCH;
    if let Ok(time) = metadata.modified()
        && let Ok(d) = time.duration_since(UNIX_EPOCH) {
            // MDTM format: YYYYMMDDHHmmss
            let dt: chrono::DateTime<chrono::Local> = time.into();
            let _ = d; // suppress unused warning
            return dt.format("%Y%m%d%H%M%S").to_string();
        }
    "19700101000000".to_string()
}

fn build_mlst_facts(metadata: &std::fs::Metadata) -> String {
    let mut facts: Vec<String> = Vec::new();

    if metadata.is_dir() {
        facts.push("type=dir".to_string());
    } else {
        facts.push("type=file".to_string());
    }

    facts.push(format!("size={}", metadata.len()));

    if let Ok(time) = metadata.modified() {
        let dt: chrono::DateTime<chrono::Utc> = time.into();
        facts.push(format!("modify={}", dt.format("%Y%m%d%H%M%S")));
    }

    format!("{};", facts.join(";"))
}
