//! FTP 基础命令处理
//!
//! 处理 QUIT、NOOP、OPTS 等基础 FTP 命令

use super::commands::FtpCommand;
use super::session_state::{ControlStream, FileStructure, SessionState, TransferModeType};
use anyhow::Result;

pub async fn handle_basic_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    ctx: &super::session_auth::CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    match cmd {
        QUIT => {
            control_stream
                .write_response(b"221 Goodbye\r\n", "FTP response")
                .await;
            return Ok(false);
        }

        SYST => {
            let hide_version = {
                let cfg = ctx.config.lock();
                cfg.ftp.hide_version_info
            };
            if hide_version {
                control_stream
                    .write_response(b"215 Type: L8\r\n", "FTP response")
                    .await;
            } else {
                control_stream
                    .write_response(b"215 UNIX Type: L8\r\n", "FTP response")
                    .await;
            }
        }

        FEAT => {
            let hide_version = {
                let cfg = ctx.config.lock();
                cfg.ftp.hide_version_info
            };
            let mut features = if hide_version {
                "211-Features:\r\n SIZE\r\n MDTM\r\n REST STREAM\r\n PASV\r\n EPSV\r\n EPRT\r\n MLST\r\n MLSD\r\n MODE S\r\n STRU F\r\n TVFS\r\n".to_string()
            } else {
                "211-Features:\r\n SIZE\r\n MDTM\r\n REST STREAM\r\n PASV\r\n EPSV\r\n EPRT\r\n PORT\r\n MLST\r\n MLSD\r\n MODE S\r\n STRU F\r\n UTF8\r\n TVFS\r\n".to_string()
            };
            if ctx.tls_config.is_tls_available() {
                features.push_str(" AUTH TLS\r\n PBSZ\r\n PROT\r\n");
                features.push_str(" MIC\r\n CONF\r\n ENC\r\n");
            }
            features.push_str(" SITE SYMLINK\r\n SITE WHO\r\n");
            features.push_str("211 End\r\n");
            control_stream
                .write_response(features.as_bytes(), "FTP response")
                .await;
        }

        NOOP => {
            control_stream
                .write_response(b"200 OK\r\n", "FTP response")
                .await;
        }

        OPTS(opt_cmd, _opt_value) => {
            if let Some(cmd) = opt_cmd {
                match cmd.to_uppercase().as_str() {
                    "UTF8" => {
                        state.encoding = "UTF-8".to_string();
                        control_stream
                            .write_response(
                                b"200 OPTS UTF8 command successful - UTF8 encoding on\r\n",
                                "FTP response",
                            )
                            .await;
                    }
                    _ => {
                        control_stream
                            .write_response(b"501 Unsupported OPTS option\r\n", "FTP response")
                            .await;
                    }
                }
            } else {
                control_stream
                    .write_response(b"501 Syntax error in OPTS command\r\n", "FTP response")
                    .await;
            }
        }

        TYPE(type_code) => {
            if let Some(type_code) = type_code {
                let type_upper = type_code.to_uppercase();
                let parts: Vec<&str> = type_upper.split_whitespace().collect();
                let main_type = parts.first().copied().unwrap_or("");
                let sub_type = parts.get(1).copied().unwrap_or("N");

                match main_type {
                    "I" => {
                        state.transfer_mode = "binary".to_string();
                        control_stream
                            .write_response(b"200 Type set to I (Binary)\r\n", "FTP response")
                            .await;
                    }
                    "L" => {
                        if sub_type == "8" {
                            state.transfer_mode = "binary".to_string();
                            control_stream
                                .write_response(
                                    b"200 Type set to L 8 (Local byte size 8)\r\n",
                                    "FTP response",
                                )
                                .await;
                        } else {
                            control_stream
                                .write_response(b"504 Only L 8 is supported\r\n", "FTP response")
                                .await;
                        }
                    }
                    "A" => match sub_type {
                        "N" | "" => {
                            state.transfer_mode = "ascii".to_string();
                            control_stream
                                .write_response(
                                    b"200 Type set to A (ASCII Non-print)\r\n",
                                    "FTP response",
                                )
                                .await;
                        }
                        "T" => {
                            state.transfer_mode = "ascii".to_string();
                            control_stream
                                .write_response(
                                    b"200 Type set to A T (ASCII Telnet format)\r\n",
                                    "FTP response",
                                )
                                .await;
                        }
                        "C" => {
                            control_stream
                                .write_response(
                                    b"504 ASA carriage control not supported\r\n",
                                    "FTP response",
                                )
                                .await;
                        }
                        _ => {
                            control_stream
                                .write_response(b"501 Unknown subtype\r\n", "FTP response")
                                .await;
                        }
                    },
                    "E" => {
                        control_stream
                            .write_response(
                                b"504 EBCDIC not supported, use A or I\r\n",
                                "FTP response",
                            )
                            .await;
                    }
                    _ => {
                        control_stream
                            .write_response(b"501 Unknown type\r\n", "FTP response")
                            .await;
                    }
                }
            } else {
                let type_str = match state.transfer_mode.as_str() {
                    "binary" => "200 Type is I (Binary)\r\n",
                    "ascii" => "200 Type is A (ASCII)\r\n",
                    _ => "200 Type set\r\n",
                };
                control_stream
                    .write_response(type_str.as_bytes(), "FTP response")
                    .await;
            }
        }

        MODE(mode) => {
            if let Some(mode) = mode {
                match mode.to_uppercase().as_str() {
                    "S" => {
                        state.transfer_mode_type = TransferModeType::Stream;
                        control_stream
                            .write_response(b"200 Mode set to Stream\r\n", "FTP response")
                            .await;
                    }
                    "B" => {
                        state.transfer_mode_type = TransferModeType::Block;
                        control_stream
                            .write_response(b"200 Mode set to Block\r\n", "FTP response")
                            .await;
                    }
                    "C" => {
                        state.transfer_mode_type = TransferModeType::Compressed;
                        control_stream
                            .write_response(b"200 Mode set to Compressed\r\n", "FTP response")
                            .await;
                    }
                    _ => {
                        control_stream
                            .write_response(b"501 Unknown mode\r\n", "FTP response")
                            .await;
                    }
                }
            } else {
                control_stream
                    .write_response(
                        b"501 Syntax error: MODE requires parameter\r\n",
                        "FTP response",
                    )
                    .await;
            }
        }

        STRU(structure) => {
            if let Some(structure) = structure {
                match structure.to_uppercase().as_str() {
                    "F" => {
                        state.file_structure = FileStructure::File;
                        control_stream
                            .write_response(b"200 Structure set to File\r\n", "FTP response")
                            .await;
                    }
                    "R" => {
                        state.file_structure = FileStructure::Record;
                        control_stream
                            .write_response(b"200 Structure set to Record\r\n", "FTP response")
                            .await;
                    }
                    "P" => {
                        state.file_structure = FileStructure::Page;
                        control_stream
                            .write_response(b"200 Structure set to Page\r\n", "FTP response")
                            .await;
                    }
                    _ => {
                        control_stream
                            .write_response(b"501 Unknown structure\r\n", "FTP response")
                            .await;
                    }
                }
            } else {
                control_stream
                    .write_response(
                        b"501 Syntax error: STRU requires parameter\r\n",
                        "FTP response",
                    )
                    .await;
            }
        }

        ALLO => {
            control_stream
                .write_response(b"200 ALLO command successful\r\n", "FTP response")
                .await;
        }

        REST(offset_str) => {
            if let Some(offset_str) = offset_str {
                if let Ok(offset) = offset_str.parse::<u64>() {
                    state.rest_offset = offset;
                    control_stream
                        .write_response(
                            format!("350 Restarting at {}\r\n", offset).as_bytes(),
                            "FTP response",
                        )
                        .await;
                    tracing::debug!(
                        client_ip = %ctx.client_ip,
                        username = ?state.current_user.as_deref(),
                        action = "REST",
                        "REST command: offset {}", offset
                    );
                } else {
                    control_stream
                        .write_response(b"501 Syntax error in REST parameter\r\n", "FTP response")
                        .await;
                }
            } else {
                state.rest_offset = 0;
                control_stream
                    .write_response(b"350 Restarting at 0\r\n", "FTP response")
                    .await;
            }
        }

        ACCT => {
            control_stream
                .write_response(b"202 Account not required\r\n", "FTP response")
                .await;
        }

        REIN => {
            let tls_was_enabled = state.tls_enabled;
            let tls_config_preserved = state.data_protection;

            state.authenticated = false;
            state.current_user = None;
            state.cwd = String::new();
            state.home_dir = String::new();
            state.data_port = None;
            state.data_addr = None;
            state.rest_offset = 0;
            state.rename_from = None;
            state
                .abort_flag
                .store(false, std::sync::atomic::Ordering::Relaxed);

            state.file_structure = FileStructure::File;
            state.transfer_mode_type = TransferModeType::Stream;
            state.transfer_mode = state.encoding.clone();

            state.tls_enabled = tls_was_enabled;
            state.data_protection = tls_config_preserved;
            state.pbsz_set = false;

            control_stream
                .write_response(b"220 Service ready for new user\r\n", "FTP response")
                .await;

            tracing::info!(
                client_ip = %ctx.client_ip,
                previous_user = ?state.current_user,
                tls_preserved = tls_was_enabled,
                action = "REIN",
                protocol = "FTP",
                "Connection reinitialized"
            );
        }

        ABOR => {
            state
                .abort_flag
                .store(true, std::sync::atomic::Ordering::Relaxed);

            if let Some(port) = state.data_port {
                state.passive_manager.remove_listener(port);
                tracing::debug!("ABOR: Removed passive listener on port {}", port);
            }

            state.rest_offset = 0;
            state.data_port = None;
            state.data_addr = None;

            control_stream
                .write_response(
                    b"426 Connection closed; transfer aborted\r\n",
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(b"226 Abort successful\r\n", "FTP response")
                .await;

            tracing::info!(
                client_ip = %ctx.client_ip,
                username = ?state.current_user.as_deref(),
                action = "ABOR",
                protocol = "FTP",
                "Data transfer aborted"
            );
        }

        _ => return Ok(true),
    }

    Ok(true)
}

pub async fn handle_help_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    if let HELP(opt_cmd) = cmd {
        if let Some(cmd) = opt_cmd {
            let help_text = match cmd.to_uppercase().as_str() {
                "USER" => {
                    "214 USER <username>: Specify user name for authentication. Use 'anonymous' or 'ftp' for anonymous access.\r\n"
                }
                "PASS" => {
                    "214 PASS <password>: Specify password for authentication. For anonymous access, use email as password.\r\n"
                }
                "ACCT" => {
                    "214 ACCT <account>: Send account information (not required by this server).\r\n"
                }
                "CWD" => {
                    "214 CWD <directory>: Change working directory to the specified path. Supports relative and absolute paths.\r\n"
                }
                "CDUP" => "214 CDUP: Change to parent directory (same as CWD ..).\r\n",
                "XCUP" => "214 XCUP: Change to parent directory (deprecated, use CDUP).\r\n",
                "PWD" => "214 PWD: Print current working directory path.\r\n",
                "XPWD" => "214 XPWD: Print current working directory (deprecated, use PWD).\r\n",
                "LIST" => {
                    "214 LIST [<path>]: List directory contents in Unix format. If no path specified, lists current directory.\r\n"
                }
                "NLST" => {
                    "214 NLST [<path>]: List directory names only (no details). Useful for automated scripts.\r\n"
                }
                "MLSD" => {
                    "214 MLSD [<path>]: List directory contents with machine-readable facts (RFC 3659).\r\n"
                }
                "MLST" => {
                    "214 MLST [<path>]: Show facts for a single file/directory (RFC 3659).\r\n"
                }
                "RETR" => {
                    "214 RETR <filename>: Retrieve/download a file from the server. Supports REST for resume.\r\n"
                }
                "STOR" => {
                    "214 STOR <filename>: Store/upload a file to the server. Overwrites existing files.\r\n"
                }
                "STOU" => {
                    "214 STOU: Store file with unique name (server generates filename). Returns the generated name.\r\n"
                }
                "APPE" => {
                    "214 APPE <filename>: Append data to existing file, or create if not exists.\r\n"
                }
                "DELE" => "214 DELE <filename>: Delete a file from the server.\r\n",
                "MKD" => "214 MKD <directory>: Create a new directory.\r\n",
                "XMKD" => "214 MKD <directory>: Create directory (deprecated, use MKD).\r\n",
                "RMD" => "214 RMD <directory>: Remove an empty directory.\r\n",
                "XRMD" => "214 XRMD: Remove directory (deprecated, use RMD).\r\n",
                "RNFR" => {
                    "214 RNFR <filename>: Specify rename-from filename (first part of rename sequence).\r\n"
                }
                "RNTO" => {
                    "214 RNTO <filename>: Specify rename-to filename (second part of rename sequence).\r\n"
                }
                "PASV" => {
                    "214 PASV: Enter passive mode for data transfer. Server opens a port for client to connect.\r\n"
                }
                "EPSV" => "214 EPSV: Enter extended passive mode (supports IPv6, RFC 2428).\r\n",
                "PORT" => {
                    "214 PORT <h1,h2,h3,h4,p1,p2>: Enter active mode. Client IP must match control connection.\r\n"
                }
                "EPRT" => {
                    "214 EPRT |<netproto>|<netaddr>|<tcpport>|: Extended active mode (supports IPv6, RFC 2428).\r\n"
                }
                "TYPE" => {
                    "214 TYPE <type>: Set transfer type. A=ASCII, I=Binary(Image), L 8=Local byte size 8.\r\n"
                }
                "MODE" => {
                    "214 MODE <mode>: Set transfer mode. S=Stream, B=Block, C=Compressed.\r\n"
                }
                "STRU" => "214 STRU <structure>: Set file structure. F=File, R=Record, P=Page.\r\n",
                "REST" => {
                    "214 REST <offset>: Set restart marker for resuming transfers. Use before RETR or STOR.\r\n"
                }
                "SIZE" => "214 SIZE <filename>: Get file size in bytes (RFC 3659).\r\n",
                "MDTM" => {
                    "214 MDTM <filename>: Get file modification time in YYYYMMDDHHMMSS format (RFC 3659).\r\n"
                }
                "ABOR" => "214 ABOR: Abort current data transfer and close data connection.\r\n",
                "QUIT" => "214 QUIT: Disconnect from server and close control connection.\r\n",
                "REIN" => {
                    "214 REIN: Reinitialize connection, reset all parameters (stay connected).\r\n"
                }
                "SYST" => "214 SYST: Return system type (returns 'UNIX Type: L8').\r\n",
                "FEAT" => "214 FEAT: List server-supported features and extensions.\r\n",
                "STAT" => {
                    "214 STAT [<path>]: Without parameter: show server status. With parameter: show file/directory info.\r\n"
                }
                "HELP" => {
                    "214 HELP [<command>]: Show help information. Without parameter: list all commands.\r\n"
                }
                "NOOP" => {
                    "214 NOOP: No operation, returns 200 OK. Used to keep connection alive.\r\n"
                }
                "SITE" => {
                    "214 SITE <command>: Execute server-specific commands (CHMOD, IDLE, HELP, WHO, WHOIS, SYMLINK).\r\n"
                }
                "AUTH" => {
                    "214 AUTH <type>: Initiate TLS/SSL authentication. Type can be TLS, TLS-C, or SSL.\r\n"
                }
                "PBSZ" => {
                    "214 PBSZ <size>: Set protection buffer size (must be 0 for TLS). Use after AUTH.\r\n"
                }
                "PROT" => {
                    "214 PROT <level>: Set data channel protection level. C=Clear, P=Private(encrypted).\r\n"
                }
                "CCC" => {
                    "214 CCC: Clear command channel (revert to unencrypted control connection).\r\n"
                }
                "ADAT" => {
                    "214 ADAT <data>: Authentication/Security Data (RFC 2228). Used for Kerberos/GSSAPI.\r\n"
                }
                "MIC" => {
                    "214 MIC <data>: Integrity Protected Command (RFC 2228). Command with integrity protection.\r\n"
                }
                "CONF" => {
                    "214 CONF <data>: Confidentiality Protected Command (RFC 2228). Encrypted command.\r\n"
                }
                "ENC" => {
                    "214 ENC <data>: Privacy Protected Command (RFC 2228). Fully encrypted command.\r\n"
                }
                "OPTS" => "214 OPTS <option>: Set options (e.g., OPTS UTF8 ON).\r\n",
                "ALLO" => {
                    "214 ALLO <size>: Allocate storage space (no-op on this server, returns success).\r\n"
                }
                _ => "214 Unknown command or no help available\r\n",
            };
            control_stream
                .write_response(help_text.as_bytes(), "FTP response")
                .await;
        } else {
            control_stream
                .write_response(
                    b"214-The following commands are recognized:\r\n",
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(b"214-Connection and Authentication:\r\n", "FTP response")
                .await;
            control_stream
                .write_response(
                    b"214-  USER PASS ACCT AUTH PBSZ PROT CCC QUIT REIN\r\n",
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(
                    b"214-RFC 2228 Security Extensions (requires TLS):\r\n",
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(b"214-  ADAT MIC CONF ENC\r\n", "FTP response")
                .await;
            control_stream
                .write_response(b"214-Directory Operations:\r\n", "FTP response")
                .await;
            control_stream
                .write_response(
                    b"214-  CWD CDUP XCUP PWD XPWD MKD XMKD RMD XRMD\r\n",
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(b"214-File Operations:\r\n", "FTP response")
                .await;
            control_stream
                .write_response(
                    b"214-  RETR STOR STOU APPE DELE RNFR RNTO REST SIZE MDTM\r\n",
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(b"214-Directory Listing:\r\n", "FTP response")
                .await;
            control_stream
                .write_response(b"214-  LIST NLST MLSD MLST STAT\r\n", "FTP response")
                .await;
            control_stream
                .write_response(b"214-Transfer Settings:\r\n", "FTP response")
                .await;
            control_stream
                .write_response(
                    b"214-  TYPE MODE STRU PASV EPSV PORT EPRT\r\n",
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(b"214-Miscellaneous:\r\n", "FTP response")
                .await;
            control_stream
                .write_response(
                    b"214-  SYST FEAT HELP NOOP SITE OPTS ALLO ABOR\r\n",
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(
                    b"214-Use 'HELP <command>' for detailed information on a specific command.\r\n",
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(b"214 Direct comments to admin\r\n", "FTP response")
                .await;
        }
    }

    Ok(true)
}

pub async fn handle_stat_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &SessionState,
    ctx: &super::session_auth::CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    if let STAT = cmd {
        if let Some(ref username) = state.current_user {
            control_stream
                .write_response(b"211-FTP server status:\r\n", "FTP response")
                .await;
            control_stream
                .write_response(
                    format!("211-Connected to: {}\r\n", ctx.client_ip).as_bytes(),
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(
                    format!("211-Logged in as: {}\r\n", username).as_bytes(),
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(
                    format!("211-Current directory: {}\r\n", state.cwd).as_bytes(),
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(
                    format!(
                        "211-Transfer mode: {}\r\n",
                        if state.passive_mode {
                            "Passive"
                        } else {
                            "Active"
                        }
                    )
                    .as_bytes(),
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(
                    format!(
                        "211-Transfer type: {}\r\n",
                        state.transfer_mode.to_uppercase()
                    )
                    .as_bytes(),
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(
                    format!("211-File structure: {:?}\r\n", state.file_structure).as_bytes(),
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(
                    format!("211-Transfer mode type: {:?}\r\n", state.transfer_mode_type)
                        .as_bytes(),
                    "FTP response",
                )
                .await;
            control_stream
                .write_response(
                    format!(
                        "211-TLS: {}\r\n",
                        if state.tls_enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        }
                    )
                    .as_bytes(),
                    "FTP response",
                )
                .await;
            if state.tls_enabled {
                control_stream
                    .write_response(
                        format!(
                            "211-Data protection: {}\r\n",
                            if state.data_protection {
                                "Private (encrypted)"
                            } else {
                                "Clear"
                            }
                        )
                        .as_bytes(),
                        "FTP response",
                    )
                    .await;
            }
            if state.rest_offset > 0 {
                control_stream
                    .write_response(
                        format!("211-Restart offset: {}\r\n", state.rest_offset).as_bytes(),
                        "FTP response",
                    )
                    .await;
            }
            if let Some(data_port) = state.data_port {
                control_stream
                    .write_response(
                        format!("211-Data port: {}\r\n", data_port).as_bytes(),
                        "FTP response",
                    )
                    .await;
            }
            if let Some(ref rename_from) = state.rename_from {
                control_stream
                    .write_response(
                        format!("211-Rename from: {}\r\n", rename_from).as_bytes(),
                        "FTP response",
                    )
                    .await;
            }
            control_stream
                .write_response(b"211 End\r\n", "FTP response")
                .await;
        } else {
            control_stream
                .write_response(b"211 FTP server status - Not logged in\r\n", "FTP response")
                .await;
        }
    }

    Ok(true)
}
