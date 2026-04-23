//! FTP command definitions
//!
//! Defines all FTP protocol command enum types

#![allow(clippy::upper_case_acronyms)]

#[derive(Debug, Clone)]
pub enum FtpCommand {
    USER(String),
    PASS(Option<String>),
    QUIT,
    SYST,
    FEAT,
    NOOP,
    PWD,
    XPWD,
    CWD(Option<String>),
    CDUP,
    XCUP,
    TYPE(Option<String>),
    MODE(Option<String>),
    STRU(Option<String>),
    ALLO,
    OPTS(Option<String>, Option<String>), // (option, value)
    REST(Option<String>),
    PASV,
    EPSV,
    PORT(Option<String>),
    EPRT(Option<String>),
    PBSZ(Option<String>),
    PROT(Option<String>),
    AUTH(Option<String>),
    CCC,
    // RFC 2228 Security Commands
    ADAT(Option<String>),
    MIC(Option<String>),
    CONF(Option<String>),
    ENC(Option<String>),
    ABOR,
    REIN,
    ACCT,
    HELP(Option<String>),
    STAT,
    SITE(Option<String>),
    LIST(Option<String>),
    NLST(Option<String>),
    MLSD(Option<String>),
    MLST(Option<String>),
    RETR(Option<String>),
    STOR(Option<String>),
    APPE(Option<String>),
    DELE(Option<String>),
    MKD(Option<String>),
    RMD(Option<String>),
    RNFR(Option<String>),
    RNTO(Option<String>),
    SIZE(Option<String>),
    MDTM(Option<String>),
    STOU,
    Unknown(String),
}

impl FtpCommand {
    pub fn parse(cmd: &str, arg: Option<&str>) -> Self {
        match cmd {
            "USER" => FtpCommand::USER(arg.unwrap_or("").to_string()),
            "PASS" => FtpCommand::PASS(arg.map(|s| s.to_string())),
            "QUIT" => FtpCommand::QUIT,
            "SYST" => FtpCommand::SYST,
            "FEAT" => FtpCommand::FEAT,
            "NOOP" => FtpCommand::NOOP,
            "PWD" => FtpCommand::PWD,
            "XPWD" => FtpCommand::XPWD,
            "CWD" => FtpCommand::CWD(arg.map(|s| s.to_string())),
            "CDUP" => FtpCommand::CDUP,
            "XCUP" => FtpCommand::XCUP,
            "TYPE" => FtpCommand::TYPE(arg.map(|s| s.to_string())),
            "MODE" => FtpCommand::MODE(arg.map(|s| s.to_string())),
            "STRU" => FtpCommand::STRU(arg.map(|s| s.to_string())),
            "ALLO" => FtpCommand::ALLO,
            "OPTS" => {
                // Parse OPTS command with optional value (e.g., "OPTS UTF8 ON")
                if let Some(arg_str) = arg {
                    let parts: Vec<&str> = arg_str.splitn(2, ' ').collect();
                    let opt = parts.first().map(|s| s.to_string());
                    let val = parts.get(1).map(|s| s.to_string());
                    FtpCommand::OPTS(opt, val)
                } else {
                    FtpCommand::OPTS(None, None)
                }
            }
            "REST" => FtpCommand::REST(arg.map(|s| s.to_string())),
            "PASV" => FtpCommand::PASV,
            "EPSV" => FtpCommand::EPSV,
            "PORT" => FtpCommand::PORT(arg.map(|s| s.to_string())),
            "EPRT" => FtpCommand::EPRT(arg.map(|s| s.to_string())),
            "PBSZ" => FtpCommand::PBSZ(arg.map(|s| s.to_string())),
            "PROT" => FtpCommand::PROT(arg.map(|s| s.to_string())),
            "AUTH" => FtpCommand::AUTH(arg.map(|s| s.to_string())),
            "CCC" => FtpCommand::CCC,
            // RFC 2228 Security Commands
            "ADAT" => FtpCommand::ADAT(arg.map(|s| s.to_string())),
            "MIC" => FtpCommand::MIC(arg.map(|s| s.to_string())),
            "CONF" => FtpCommand::CONF(arg.map(|s| s.to_string())),
            "ENC" => FtpCommand::ENC(arg.map(|s| s.to_string())),
            "ABOR" => FtpCommand::ABOR,
            "REIN" => FtpCommand::REIN,
            "ACCT" => FtpCommand::ACCT,
            "HELP" => FtpCommand::HELP(arg.map(|s| s.to_string())),
            "STAT" => FtpCommand::STAT,
            "SITE" => FtpCommand::SITE(arg.map(|s| s.to_string())),
            "LIST" => FtpCommand::LIST(arg.map(|s| s.to_string())),
            "NLST" => FtpCommand::NLST(arg.map(|s| s.to_string())),
            "MLSD" => FtpCommand::MLSD(arg.map(|s| s.to_string())),
            "MLST" => FtpCommand::MLST(arg.map(|s| s.to_string())),
            "RETR" => FtpCommand::RETR(arg.map(|s| s.to_string())),
            "STOR" => FtpCommand::STOR(arg.map(|s| s.to_string())),
            "APPE" => FtpCommand::APPE(arg.map(|s| s.to_string())),
            "DELE" => FtpCommand::DELE(arg.map(|s| s.to_string())),
            "MKD" | "XMKD" => FtpCommand::MKD(arg.map(|s| s.to_string())),
            "RMD" | "XRMD" => FtpCommand::RMD(arg.map(|s| s.to_string())),
            "RNFR" => FtpCommand::RNFR(arg.map(|s| s.to_string())),
            "RNTO" => FtpCommand::RNTO(arg.map(|s| s.to_string())),
            "SIZE" => FtpCommand::SIZE(arg.map(|s| s.to_string())),
            "MDTM" => FtpCommand::MDTM(arg.map(|s| s.to_string())),
            "STOU" => FtpCommand::STOU,
            _ => FtpCommand::Unknown(cmd.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user() {
        match FtpCommand::parse("USER", Some("testuser")) {
            FtpCommand::USER(u) => assert_eq!(u, "testuser"),
            other => panic!("Expected USER, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_user_no_arg() {
        match FtpCommand::parse("USER", None) {
            FtpCommand::USER(u) => assert_eq!(u, ""),
            other => panic!("Expected USER, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_pass() {
        match FtpCommand::parse("PASS", Some("secret")) {
            FtpCommand::PASS(Some(p)) => assert_eq!(p, "secret"),
            other => panic!("Expected PASS(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_pass_no_arg() {
        match FtpCommand::parse("PASS", None) {
            FtpCommand::PASS(None) => {}
            other => panic!("Expected PASS(None), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_quit() {
        assert!(matches!(FtpCommand::parse("QUIT", None), FtpCommand::QUIT));
    }

    #[test]
    fn test_parse_syst() {
        assert!(matches!(FtpCommand::parse("SYST", None), FtpCommand::SYST));
    }

    #[test]
    fn test_parse_feat() {
        assert!(matches!(FtpCommand::parse("FEAT", None), FtpCommand::FEAT));
    }

    #[test]
    fn test_parse_noop() {
        assert!(matches!(FtpCommand::parse("NOOP", None), FtpCommand::NOOP));
    }

    #[test]
    fn test_parse_pwd() {
        assert!(matches!(FtpCommand::parse("PWD", None), FtpCommand::PWD));
    }

    #[test]
    fn test_parse_cwd() {
        match FtpCommand::parse("CWD", Some("/home")) {
            FtpCommand::CWD(Some(p)) => assert_eq!(p, "/home"),
            other => panic!("Expected CWD(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_cdup() {
        assert!(matches!(FtpCommand::parse("CDUP", None), FtpCommand::CDUP));
    }

    #[test]
    fn test_parse_type() {
        match FtpCommand::parse("TYPE", Some("I")) {
            FtpCommand::TYPE(Some(t)) => assert_eq!(t, "I"),
            other => panic!("Expected TYPE(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_pasv() {
        assert!(matches!(FtpCommand::parse("PASV", None), FtpCommand::PASV));
    }

    #[test]
    fn test_parse_epsv() {
        assert!(matches!(FtpCommand::parse("EPSV", None), FtpCommand::EPSV));
    }

    #[test]
    fn test_parse_port() {
        match FtpCommand::parse("PORT", Some("192,168,1,1,4,1")) {
            FtpCommand::PORT(Some(p)) => assert_eq!(p, "192,168,1,1,4,1"),
            other => panic!("Expected PORT(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_opts_with_value() {
        match FtpCommand::parse("OPTS", Some("UTF8 ON")) {
            FtpCommand::OPTS(Some(opt), Some(val)) => {
                assert_eq!(opt, "UTF8");
                assert_eq!(val, "ON");
            }
            other => panic!("Expected OPTS(Some, Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_opts_without_value() {
        match FtpCommand::parse("OPTS", Some("UTF8")) {
            FtpCommand::OPTS(Some(opt), None) => assert_eq!(opt, "UTF8"),
            other => panic!("Expected OPTS(Some, None), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_opts_no_arg() {
        match FtpCommand::parse("OPTS", None) {
            FtpCommand::OPTS(None, None) => {}
            other => panic!("Expected OPTS(None, None), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_rest() {
        match FtpCommand::parse("REST", Some("1024")) {
            FtpCommand::REST(Some(r)) => assert_eq!(r, "1024"),
            other => panic!("Expected REST(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_retr() {
        match FtpCommand::parse("RETR", Some("file.txt")) {
            FtpCommand::RETR(Some(f)) => assert_eq!(f, "file.txt"),
            other => panic!("Expected RETR(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_stor() {
        match FtpCommand::parse("STOR", Some("upload.txt")) {
            FtpCommand::STOR(Some(f)) => assert_eq!(f, "upload.txt"),
            other => panic!("Expected STOR(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_list() {
        match FtpCommand::parse("LIST", Some("-la")) {
            FtpCommand::LIST(Some(a)) => assert_eq!(a, "-la"),
            other => panic!("Expected LIST(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_list_no_arg() {
        match FtpCommand::parse("LIST", None) {
            FtpCommand::LIST(None) => {}
            other => panic!("Expected LIST(None), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_mkd_xmkd() {
        assert!(matches!(FtpCommand::parse("MKD", Some("dir")), FtpCommand::MKD(Some(_))));
        assert!(matches!(FtpCommand::parse("XMKD", Some("dir")), FtpCommand::MKD(Some(_))));
    }

    #[test]
    fn test_parse_rmd_xrmd() {
        assert!(matches!(FtpCommand::parse("RMD", Some("dir")), FtpCommand::RMD(Some(_))));
        assert!(matches!(FtpCommand::parse("XRMD", Some("dir")), FtpCommand::RMD(Some(_))));
    }

    #[test]
    fn test_parse_rnfr_rnto() {
        assert!(matches!(FtpCommand::parse("RNFR", Some("old")), FtpCommand::RNFR(Some(_))));
        assert!(matches!(FtpCommand::parse("RNTO", Some("new")), FtpCommand::RNTO(Some(_))));
    }

    #[test]
    fn test_parse_dele() {
        match FtpCommand::parse("DELE", Some("file.txt")) {
            FtpCommand::DELE(Some(f)) => assert_eq!(f, "file.txt"),
            other => panic!("Expected DELE(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_size() {
        match FtpCommand::parse("SIZE", Some("file.txt")) {
            FtpCommand::SIZE(Some(f)) => assert_eq!(f, "file.txt"),
            other => panic!("Expected SIZE(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_stou() {
        assert!(matches!(FtpCommand::parse("STOU", None), FtpCommand::STOU));
    }

    #[test]
    fn test_parse_abor() {
        assert!(matches!(FtpCommand::parse("ABOR", None), FtpCommand::ABOR));
    }

    #[test]
    fn test_parse_rein() {
        assert!(matches!(FtpCommand::parse("REIN", None), FtpCommand::REIN));
    }

    #[test]
    fn test_parse_auth() {
        match FtpCommand::parse("AUTH", Some("TLS")) {
            FtpCommand::AUTH(Some(a)) => assert_eq!(a, "TLS"),
            other => panic!("Expected AUTH(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_pbsz() {
        match FtpCommand::parse("PBSZ", Some("0")) {
            FtpCommand::PBSZ(Some(p)) => assert_eq!(p, "0"),
            other => panic!("Expected PBSZ(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_prot() {
        match FtpCommand::parse("PROT", Some("P")) {
            FtpCommand::PROT(Some(p)) => assert_eq!(p, "P"),
            other => panic!("Expected PROT(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_unknown() {
        match FtpCommand::parse("XYZZY", None) {
            FtpCommand::Unknown(cmd) => assert_eq!(cmd, "XYZZY"),
            other => panic!("Expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_mlsd() {
        match FtpCommand::parse("MLSD", Some("/data")) {
            FtpCommand::MLSD(Some(p)) => assert_eq!(p, "/data"),
            other => panic!("Expected MLSD(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_mlst() {
        match FtpCommand::parse("MLST", Some("file.txt")) {
            FtpCommand::MLST(Some(p)) => assert_eq!(p, "file.txt"),
            other => panic!("Expected MLST(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_appe() {
        match FtpCommand::parse("APPE", Some("log.txt")) {
            FtpCommand::APPE(Some(f)) => assert_eq!(f, "log.txt"),
            other => panic!("Expected APPE(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_site() {
        match FtpCommand::parse("SITE", Some("HELP")) {
            FtpCommand::SITE(Some(s)) => assert_eq!(s, "HELP"),
            other => panic!("Expected SITE(Some), got {:?}", other),
        }
    }
}
