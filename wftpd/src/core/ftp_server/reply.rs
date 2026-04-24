//! FTP Reply and Reply Code definitions
//!
//! Provides standardized FTP response codes and reply formatting

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ReplyCode {
    RestartMarker = 110,
    ReadyInMinutes = 120,
    AlreadyOpen = 125,
    AboutToSend = 150,

    CommandOkay = 200,
    CommandNotImplemented = 202,
    SystemStatus = 211,
    DirectoryStatus = 212,
    FileStatus = 213,
    HelpMessage = 214,
    SystemType = 215,
    ServiceReady = 220,
    ClosingControl = 221,
    DataConnectionOpen = 225,
    ClosingDataConnection = 226,
    EnteringPassiveMode = 227,
    EnteringExtendedPassiveMode = 229,
    LoggedIn = 230,
    AuthOkay = 234,
    FileActionOkay = 250,
    PathCreated = 257,

    NeedPassword = 331,
    NeedAccount = 332,
    PendingMoreInfo = 350,

    ServiceNotAvailable = 421,
    CantOpenDataConnection = 425,
    ConnectionClosed = 426,
    FileBusy = 450,
    LocalError = 451,
    InsufficientStorage = 452,

    SyntaxError = 500,
    ParameterError = 501,
    NotImplemented = 502,
    BadSequence = 503,
    ParameterNotImplemented = 504,
    NotLoggedIn = 530,
    NeedAccountForStore = 532,
    FtpsRequired = 534,
    FileUnavailable = 550,
    PageTypeUnknown = 551,
    ExceededStorage = 552,
    BadFilename = 553,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Reply {
    None,
    Single { code: ReplyCode, msg: String },
    Multi { code: ReplyCode, lines: Vec<String> },
}

impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reply::None => write!(f, "None"),
            Reply::Single { code, msg } => write!(f, "Single({:?}, {:?})", code, msg),
            Reply::Multi { code, lines } => {
                if lines.len() > 3 {
                    write!(f, "Multi({:?}, {} lines)", code, lines.len())
                } else {
                    write!(f, "Multi({:?}, {:?})", code, lines)
                }
            }
        }
    }
}

impl Reply {
    pub fn new(code: ReplyCode, msg: impl Into<String>) -> Self {
        Reply::Single {
            code,
            msg: msg.into(),
        }
    }

    pub fn multi(code: ReplyCode, lines: Vec<String>) -> Self {
        Reply::Multi { code, lines }
    }

    pub fn none() -> Self {
        Reply::None
    }

    pub fn is_positive(&self) -> bool {
        match self {
            Reply::None => true,
            Reply::Single { code, .. } | Reply::Multi { code, .. } => {
                let code_val = *code as u16;
                (200..=399).contains(&code_val)
            }
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Reply::None => Vec::new(),
            Reply::Single { code, msg } => format!("{} {}\r\n", *code as u16, msg).into_bytes(),
            Reply::Multi { code, lines } => {
                if lines.is_empty() {
                    return format!("{} \r\n", *code as u16).into_bytes();
                }
                if lines.len() == 1 {
                    return format!("{} {}\r\n", *code as u16, &lines[0]).into_bytes();
                }
                let mut result = Vec::new();
                let code_num = *code as u16;
                result.extend_from_slice(format!("{}-{}\r\n", code_num, &lines[0]).as_bytes());
                for line in &lines[1..lines.len() - 1] {
                    result.extend_from_slice(format!(" {}\r\n", line).as_bytes());
                }
                result.extend_from_slice(
                    format!("{} {}\r\n", code_num, lines.last().unwrap()).as_bytes(),
                );
                result
            }
        }
    }
}

impl From<ReplyCode> for Reply {
    fn from(code: ReplyCode) -> Self {
        let msg = match code {
            ReplyCode::CommandOkay => "OK",
            ReplyCode::ServiceReady => "Service ready",
            ReplyCode::ClosingControl => "Goodbye",
            ReplyCode::LoggedIn => "User logged in",
            ReplyCode::NeedPassword => "Password required",
            ReplyCode::NotLoggedIn => "Not logged in",
            ReplyCode::SyntaxError => "Syntax error",
            ReplyCode::NotImplemented => "Command not implemented",
            ReplyCode::FileUnavailable => "File unavailable",
            ReplyCode::EnteringPassiveMode => "Entering passive mode",
            ReplyCode::EnteringExtendedPassiveMode => "Entering extended passive mode",
            ReplyCode::FileActionOkay => "File action completed",
            ReplyCode::PathCreated => "Path created",
            ReplyCode::DataConnectionOpen => "Data connection open",
            ReplyCode::ClosingDataConnection => "Transfer complete",
            ReplyCode::AboutToSend => "About to send file",
            ReplyCode::AlreadyOpen => "Data connection already open",
            ReplyCode::CantOpenDataConnection => "Cannot open data connection",
            ReplyCode::ServiceNotAvailable => "Service not available",
            ReplyCode::LocalError => "Local error",
            _ => "",
        };
        Reply::new(code, msg)
    }
}

pub struct ReplyBuilder;

impl ReplyBuilder {
    pub fn welcome(msg: &str) -> Reply {
        Reply::new(ReplyCode::ServiceReady, msg)
    }

    pub fn goodbye(msg: &str) -> Reply {
        Reply::new(ReplyCode::ClosingControl, msg)
    }

    pub fn logged_in(username: &str, ip: &str) -> Reply {
        Reply::new(
            ReplyCode::LoggedIn,
            format!("User {} logged in from {}", username, ip),
        )
    }

    pub fn need_password(username: &str) -> Reply {
        Reply::new(
            ReplyCode::NeedPassword,
            format!("Password required for {}", username),
        )
    }

    pub fn not_logged_in(reason: &str) -> Reply {
        Reply::new(ReplyCode::NotLoggedIn, reason)
    }

    pub fn entering_passive_mode(ip: [u8; 4], port: u16) -> Reply {
        let p1 = (port >> 8) as u8;
        let p2 = port as u8;
        Reply::new(
            ReplyCode::EnteringPassiveMode,
            format!(
                "Entering Passive Mode ({},{},{},{},{},{}).",
                ip[0], ip[1], ip[2], ip[3], p1, p2
            ),
        )
    }

    pub fn entering_extended_passive_mode(port: u16) -> Reply {
        Reply::new(
            ReplyCode::EnteringExtendedPassiveMode,
            format!("Entering Extended Passive Mode (|||{}|).", port),
        )
    }

    pub fn file_action_ok(msg: &str) -> Reply {
        Reply::new(ReplyCode::FileActionOkay, msg)
    }

    pub fn path_created(path: &str) -> Reply {
        Reply::new(ReplyCode::PathCreated, format!("\"{}\" created", path))
    }

    pub fn pwd(path: &str) -> Reply {
        Reply::new(
            ReplyCode::PathCreated,
            format!("\"{}\" is current directory", path),
        )
    }

    pub fn about_to_send(filename: &str, size: Option<u64>) -> Reply {
        match size {
            Some(s) => Reply::new(
                ReplyCode::AboutToSend,
                format!(
                    "Opening BINARY mode data connection for {} ({} bytes).",
                    filename, s
                ),
            ),
            None => Reply::new(
                ReplyCode::AboutToSend,
                format!("Opening BINARY mode data connection for {}.", filename),
            ),
        }
    }

    pub fn transfer_complete(bytes: u64) -> Reply {
        Reply::new(
            ReplyCode::ClosingDataConnection,
            format!("Transfer complete ({} bytes).", bytes),
        )
    }

    pub fn file_unavailable(reason: &str) -> Reply {
        Reply::new(ReplyCode::FileUnavailable, reason)
    }

    pub fn syntax_error(msg: &str) -> Reply {
        Reply::new(ReplyCode::SyntaxError, msg)
    }

    pub fn not_implemented(cmd: &str) -> Reply {
        Reply::new(
            ReplyCode::NotImplemented,
            format!("Command {} not implemented.", cmd),
        )
    }

    pub fn feature_list(features: &[&str]) -> Reply {
        let mut lines = vec!["Features:".to_string()];
        for feat in features {
            lines.push(format!(" {}", feat));
        }
        lines.push("End.".to_string());
        Reply::multi(ReplyCode::SystemStatus, lines)
    }

    pub fn system_type(systype: &str) -> Reply {
        Reply::new(ReplyCode::SystemType, systype)
    }

    pub fn command_ok(msg: &str) -> Reply {
        Reply::new(ReplyCode::CommandOkay, msg)
    }

    pub fn type_set(mode: &str) -> Reply {
        Reply::new(ReplyCode::CommandOkay, format!("Type set to {}", mode))
    }

    pub fn service_not_available(reason: &str) -> Reply {
        Reply::new(ReplyCode::ServiceNotAvailable, reason)
    }

    pub fn auth_ok(protocol: &str) -> Reply {
        Reply::new(
            ReplyCode::AuthOkay,
            format!("{} authentication successful", protocol),
        )
    }

    pub fn ftps_disabled() -> Reply {
        Reply::new(ReplyCode::NotImplemented, "FTPS is disabled on this server")
    }

    pub fn pbsz_ok(size: u64) -> Reply {
        Reply::new(ReplyCode::CommandOkay, format!("PBSZ={} OK", size))
    }

    pub fn prot_ok(level: &str) -> Reply {
        Reply::new(ReplyCode::CommandOkay, format!("PROT={} OK", level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reply_code_values() {
        assert_eq!(ReplyCode::ServiceReady as u16, 220);
        assert_eq!(ReplyCode::ClosingControl as u16, 221);
        assert_eq!(ReplyCode::LoggedIn as u16, 230);
        assert_eq!(ReplyCode::NeedPassword as u16, 331);
        assert_eq!(ReplyCode::NotLoggedIn as u16, 530);
        assert_eq!(ReplyCode::CantOpenDataConnection as u16, 425);
        assert_eq!(ReplyCode::AboutToSend as u16, 150);
        assert_eq!(ReplyCode::ClosingDataConnection as u16, 226);
        assert_eq!(ReplyCode::EnteringPassiveMode as u16, 227);
        assert_eq!(ReplyCode::EnteringExtendedPassiveMode as u16, 229);
        assert_eq!(ReplyCode::BadSequence as u16, 503);
    }

    #[test]
    fn test_reply_single_to_bytes() {
        let reply = Reply::new(ReplyCode::ServiceReady, "FTP Server Ready");
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("220 "));
        assert!(s.contains("FTP Server Ready"));
        assert!(s.ends_with("\r\n"));
    }

    #[test]
    fn test_reply_none_to_bytes() {
        let reply = Reply::none();
        assert!(reply.to_bytes().is_empty());
    }

    #[test]
    fn test_reply_multi_to_bytes() {
        let reply = Reply::multi(
            ReplyCode::SystemStatus,
            vec![
                "Features:".to_string(),
                " UTF8".to_string(),
                "End.".to_string(),
            ],
        );
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.contains("211-Features:"));
        assert!(s.contains("211 End."));
    }

    #[test]
    fn test_reply_multi_single_line() {
        let reply = Reply::multi(ReplyCode::SystemStatus, vec!["Only line".to_string()]);
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("211 Only line"));
    }

    #[test]
    fn test_reply_multi_empty() {
        let reply = Reply::multi(ReplyCode::SystemStatus, vec![]);
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("211 "));
    }

    #[test]
    fn test_reply_is_positive() {
        assert!(Reply::new(ReplyCode::ServiceReady, "ok").is_positive());
        assert!(Reply::new(ReplyCode::LoggedIn, "ok").is_positive());
        assert!(Reply::new(ReplyCode::NeedPassword, "ok").is_positive());
        assert!(!Reply::new(ReplyCode::NotLoggedIn, "fail").is_positive());
        assert!(!Reply::new(ReplyCode::CantOpenDataConnection, "fail").is_positive());
        assert!(Reply::none().is_positive());
    }

    #[test]
    fn test_reply_from_code() {
        let reply: Reply = ReplyCode::LoggedIn.into();
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("230 "));

        let reply: Reply = ReplyCode::NeedPassword.into();
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("331 "));
    }

    #[test]
    fn test_reply_builder_welcome() {
        let reply = ReplyBuilder::welcome("Test Server");
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("220 "));
        assert!(s.contains("Test Server"));
    }

    #[test]
    fn test_reply_builder_goodbye() {
        let reply = ReplyBuilder::goodbye("Bye");
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("221 "));
    }

    #[test]
    fn test_reply_builder_logged_in() {
        let reply = ReplyBuilder::logged_in("testuser", "127.0.0.1");
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("230 "));
        assert!(s.contains("testuser"));
        assert!(s.contains("127.0.0.1"));
    }

    #[test]
    fn test_reply_builder_entering_passive_mode() {
        let reply = ReplyBuilder::entering_passive_mode([192, 168, 1, 1], 1025);
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("227 "));
        assert!(s.contains("192,168,1,1,4,1"));
    }

    #[test]
    fn test_reply_builder_entering_extended_passive_mode() {
        let reply = ReplyBuilder::entering_extended_passive_mode(50000);
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("229 "));
        assert!(s.contains("50000"));
    }

    #[test]
    fn test_reply_builder_about_to_send_with_size() {
        let reply = ReplyBuilder::about_to_send("test.txt", Some(1024));
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("150 "));
        assert!(s.contains("1024"));
    }

    #[test]
    fn test_reply_builder_about_to_send_without_size() {
        let reply = ReplyBuilder::about_to_send("test.txt", None);
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("150 "));
        assert!(!s.contains("bytes"));
    }

    #[test]
    fn test_reply_builder_transfer_complete() {
        let reply = ReplyBuilder::transfer_complete(2048);
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("226 "));
        assert!(s.contains("2048"));
    }

    #[test]
    fn test_reply_builder_feature_list() {
        let reply = ReplyBuilder::feature_list(&["UTF8", "PASV"]);
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.contains("UTF8"));
        assert!(s.contains("PASV"));
        assert!(s.contains("211-"));
        assert!(s.contains("211 End."));
    }

    #[test]
    fn test_reply_builder_not_logged_in() {
        let reply = ReplyBuilder::not_logged_in("Access denied");
        let bytes = reply.to_bytes();
        let s = String::from_utf8(bytes).expect("valid UTF-8");
        assert!(s.starts_with("530 "));
    }

    #[test]
    fn test_reply_equality() {
        let r1 = Reply::new(ReplyCode::LoggedIn, "OK");
        let r2 = Reply::new(ReplyCode::LoggedIn, "OK");
        assert_eq!(r1, r2);

        let r3 = Reply::new(ReplyCode::LoggedIn, "Different");
        assert_ne!(r1, r3);

        assert_eq!(Reply::none(), Reply::none());
    }
}
