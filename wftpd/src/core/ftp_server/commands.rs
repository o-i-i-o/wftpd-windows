//! FTP command definitions
//!
//! Defines all FTP protocol command enum types

#[derive(Debug, Clone)]
pub enum FtpCommand {
    User(String),
    Pass(Option<String>),
    Quit,
    Syst,
    Feat,
    Noop,
    Pwd,
    Xpwd,
    Cwd(Option<String>),
    Cdup,
    Xcup,
    Type(Option<String>),
    Mode(Option<String>),
    Stru(Option<String>),
    Allo,
    Opts(Option<String>, Option<String>),
    Rest(Option<String>),
    Pasv,
    Epsv,
    Port(Option<String>),
    Eprt(Option<String>),
    Pbsz(Option<String>),
    Prot(Option<String>),
    Auth(Option<String>),
    Ccc,
    Adat(Option<String>),
    Mic(Option<String>),
    Conf(Option<String>),
    Enc(Option<String>),
    Abor,
    Rein,
    Acct,
    Help(Option<String>),
    Stat,
    Site(Option<String>),
    List(Option<String>),
    Nlst(Option<String>),
    Mlsd(Option<String>),
    Mlst(Option<String>),
    Retr(Option<String>),
    Stor(Option<String>),
    Appe(Option<String>),
    Dele(Option<String>),
    Mkd(Option<String>),
    Rmd(Option<String>),
    Rnfr(Option<String>),
    Rnto(Option<String>),
    Size(Option<String>),
    Mdtm(Option<String>),
    Stou,
    Unknown(String),
}

impl FtpCommand {
    pub fn parse(cmd: &str, arg: Option<&str>) -> Self {
        match cmd {
            "USER" => FtpCommand::User(arg.unwrap_or("").to_string()),
            "PASS" => FtpCommand::Pass(arg.map(|s| s.to_string())),
            "QUIT" => FtpCommand::Quit,
            "SYST" => FtpCommand::Syst,
            "FEAT" => FtpCommand::Feat,
            "NOOP" => FtpCommand::Noop,
            "PWD" => FtpCommand::Pwd,
            "XPWD" => FtpCommand::Xpwd,
            "CWD" => FtpCommand::Cwd(arg.map(|s| s.to_string())),
            "CDUP" => FtpCommand::Cdup,
            "XCUP" => FtpCommand::Xcup,
            "TYPE" => FtpCommand::Type(arg.map(|s| s.to_string())),
            "MODE" => FtpCommand::Mode(arg.map(|s| s.to_string())),
            "STRU" => FtpCommand::Stru(arg.map(|s| s.to_string())),
            "ALLO" => FtpCommand::Allo,
            "OPTS" => {
                if let Some(arg_str) = arg {
                    let parts: Vec<&str> = arg_str.splitn(2, ' ').collect();
                    let opt = parts.first().map(|s| s.to_string());
                    let val = parts.get(1).map(|s| s.to_string());
                    FtpCommand::Opts(opt, val)
                } else {
                    FtpCommand::Opts(None, None)
                }
            }
            "REST" => FtpCommand::Rest(arg.map(|s| s.to_string())),
            "PASV" => FtpCommand::Pasv,
            "EPSV" => FtpCommand::Epsv,
            "PORT" => FtpCommand::Port(arg.map(|s| s.to_string())),
            "EPRT" => FtpCommand::Eprt(arg.map(|s| s.to_string())),
            "PBSZ" => FtpCommand::Pbsz(arg.map(|s| s.to_string())),
            "PROT" => FtpCommand::Prot(arg.map(|s| s.to_string())),
            "AUTH" => FtpCommand::Auth(arg.map(|s| s.to_string())),
            "CCC" => FtpCommand::Ccc,
            "ADAT" => FtpCommand::Adat(arg.map(|s| s.to_string())),
            "MIC" => FtpCommand::Mic(arg.map(|s| s.to_string())),
            "CONF" => FtpCommand::Conf(arg.map(|s| s.to_string())),
            "ENC" => FtpCommand::Enc(arg.map(|s| s.to_string())),
            "ABOR" => FtpCommand::Abor,
            "REIN" => FtpCommand::Rein,
            "ACCT" => FtpCommand::Acct,
            "HELP" => FtpCommand::Help(arg.map(|s| s.to_string())),
            "STAT" => FtpCommand::Stat,
            "SITE" => FtpCommand::Site(arg.map(|s| s.to_string())),
            "LIST" => FtpCommand::List(arg.map(|s| s.to_string())),
            "NLST" => FtpCommand::Nlst(arg.map(|s| s.to_string())),
            "MLSD" => FtpCommand::Mlsd(arg.map(|s| s.to_string())),
            "MLST" => FtpCommand::Mlst(arg.map(|s| s.to_string())),
            "RETR" => FtpCommand::Retr(arg.map(|s| s.to_string())),
            "STOR" => FtpCommand::Stor(arg.map(|s| s.to_string())),
            "APPE" => FtpCommand::Appe(arg.map(|s| s.to_string())),
            "DELE" => FtpCommand::Dele(arg.map(|s| s.to_string())),
            "MKD" | "XMKD" => FtpCommand::Mkd(arg.map(|s| s.to_string())),
            "RMD" | "XRMD" => FtpCommand::Rmd(arg.map(|s| s.to_string())),
            "RNFR" => FtpCommand::Rnfr(arg.map(|s| s.to_string())),
            "RNTO" => FtpCommand::Rnto(arg.map(|s| s.to_string())),
            "SIZE" => FtpCommand::Size(arg.map(|s| s.to_string())),
            "MDTM" => FtpCommand::Mdtm(arg.map(|s| s.to_string())),
            "STOU" => FtpCommand::Stou,
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
            FtpCommand::User(u) => assert_eq!(u, "testuser"),
            other => panic!("Expected User, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_user_no_arg() {
        match FtpCommand::parse("USER", None) {
            FtpCommand::User(u) => assert_eq!(u, ""),
            other => panic!("Expected User, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_pass() {
        match FtpCommand::parse("PASS", Some("secret")) {
            FtpCommand::Pass(Some(p)) => assert_eq!(p, "secret"),
            other => panic!("Expected Pass(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_pass_no_arg() {
        match FtpCommand::parse("PASS", None) {
            FtpCommand::Pass(None) => {}
            other => panic!("Expected Pass(None), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_quit() {
        assert!(matches!(FtpCommand::parse("QUIT", None), FtpCommand::Quit));
    }

    #[test]
    fn test_parse_syst() {
        assert!(matches!(FtpCommand::parse("SYST", None), FtpCommand::Syst));
    }

    #[test]
    fn test_parse_feat() {
        assert!(matches!(FtpCommand::parse("FEAT", None), FtpCommand::Feat));
    }

    #[test]
    fn test_parse_noop() {
        assert!(matches!(FtpCommand::parse("NOOP", None), FtpCommand::Noop));
    }

    #[test]
    fn test_parse_pwd() {
        assert!(matches!(FtpCommand::parse("PWD", None), FtpCommand::Pwd));
    }

    #[test]
    fn test_parse_cwd() {
        match FtpCommand::parse("CWD", Some("/home")) {
            FtpCommand::Cwd(Some(p)) => assert_eq!(p, "/home"),
            other => panic!("Expected Cwd(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_cdup() {
        assert!(matches!(FtpCommand::parse("CDUP", None), FtpCommand::Cdup));
    }

    #[test]
    fn test_parse_type() {
        match FtpCommand::parse("TYPE", Some("I")) {
            FtpCommand::Type(Some(t)) => assert_eq!(t, "I"),
            other => panic!("Expected Type(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_pasv() {
        assert!(matches!(FtpCommand::parse("PASV", None), FtpCommand::Pasv));
    }

    #[test]
    fn test_parse_epsv() {
        assert!(matches!(FtpCommand::parse("EPSV", None), FtpCommand::Epsv));
    }

    #[test]
    fn test_parse_port() {
        match FtpCommand::parse("PORT", Some("192,168,1,1,4,1")) {
            FtpCommand::Port(Some(p)) => assert_eq!(p, "192,168,1,1,4,1"),
            other => panic!("Expected Port(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_opts_with_value() {
        match FtpCommand::parse("OPTS", Some("UTF8 ON")) {
            FtpCommand::Opts(Some(opt), Some(val)) => {
                assert_eq!(opt, "UTF8");
                assert_eq!(val, "ON");
            }
            other => panic!("Expected Opts(Some, Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_opts_without_value() {
        match FtpCommand::parse("OPTS", Some("UTF8")) {
            FtpCommand::Opts(Some(opt), None) => assert_eq!(opt, "UTF8"),
            other => panic!("Expected Opts(Some, None), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_opts_no_arg() {
        match FtpCommand::parse("OPTS", None) {
            FtpCommand::Opts(None, None) => {}
            other => panic!("Expected Opts(None, None), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_rest() {
        match FtpCommand::parse("REST", Some("1024")) {
            FtpCommand::Rest(Some(r)) => assert_eq!(r, "1024"),
            other => panic!("Expected Rest(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_retr() {
        match FtpCommand::parse("RETR", Some("file.txt")) {
            FtpCommand::Retr(Some(f)) => assert_eq!(f, "file.txt"),
            other => panic!("Expected Retr(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_stor() {
        match FtpCommand::parse("STOR", Some("upload.txt")) {
            FtpCommand::Stor(Some(f)) => assert_eq!(f, "upload.txt"),
            other => panic!("Expected Stor(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_list() {
        match FtpCommand::parse("LIST", Some("-la")) {
            FtpCommand::List(Some(a)) => assert_eq!(a, "-la"),
            other => panic!("Expected List(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_list_no_arg() {
        match FtpCommand::parse("LIST", None) {
            FtpCommand::List(None) => {}
            other => panic!("Expected List(None), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_mkd_xmkd() {
        assert!(matches!(
            FtpCommand::parse("MKD", Some("dir")),
            FtpCommand::Mkd(Some(_))
        ));
        assert!(matches!(
            FtpCommand::parse("XMKD", Some("dir")),
            FtpCommand::Mkd(Some(_))
        ));
    }

    #[test]
    fn test_parse_rmd_xrmd() {
        assert!(matches!(
            FtpCommand::parse("RMD", Some("dir")),
            FtpCommand::Rmd(Some(_))
        ));
        assert!(matches!(
            FtpCommand::parse("XRMD", Some("dir")),
            FtpCommand::Rmd(Some(_))
        ));
    }

    #[test]
    fn test_parse_rnfr_rnto() {
        assert!(matches!(
            FtpCommand::parse("RNFR", Some("old")),
            FtpCommand::Rnfr(Some(_))
        ));
        assert!(matches!(
            FtpCommand::parse("RNTO", Some("new")),
            FtpCommand::Rnto(Some(_))
        ));
    }

    #[test]
    fn test_parse_dele() {
        match FtpCommand::parse("DELE", Some("file.txt")) {
            FtpCommand::Dele(Some(f)) => assert_eq!(f, "file.txt"),
            other => panic!("Expected Dele(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_size() {
        match FtpCommand::parse("SIZE", Some("file.txt")) {
            FtpCommand::Size(Some(f)) => assert_eq!(f, "file.txt"),
            other => panic!("Expected Size(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_stou() {
        assert!(matches!(FtpCommand::parse("STOU", None), FtpCommand::Stou));
    }

    #[test]
    fn test_parse_abor() {
        assert!(matches!(FtpCommand::parse("ABOR", None), FtpCommand::Abor));
    }

    #[test]
    fn test_parse_rein() {
        assert!(matches!(FtpCommand::parse("REIN", None), FtpCommand::Rein));
    }

    #[test]
    fn test_parse_auth() {
        match FtpCommand::parse("AUTH", Some("TLS")) {
            FtpCommand::Auth(Some(a)) => assert_eq!(a, "TLS"),
            other => panic!("Expected Auth(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_pbsz() {
        match FtpCommand::parse("PBSZ", Some("0")) {
            FtpCommand::Pbsz(Some(p)) => assert_eq!(p, "0"),
            other => panic!("Expected Pbsz(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_prot() {
        match FtpCommand::parse("PROT", Some("P")) {
            FtpCommand::Prot(Some(p)) => assert_eq!(p, "P"),
            other => panic!("Expected Prot(Some), got {:?}", other),
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
            FtpCommand::Mlsd(Some(p)) => assert_eq!(p, "/data"),
            other => panic!("Expected Mlsd(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_mlst() {
        match FtpCommand::parse("MLST", Some("file.txt")) {
            FtpCommand::Mlst(Some(p)) => assert_eq!(p, "file.txt"),
            other => panic!("Expected Mlst(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_appe() {
        match FtpCommand::parse("APPE", Some("log.txt")) {
            FtpCommand::Appe(Some(f)) => assert_eq!(f, "log.txt"),
            other => panic!("Expected Appe(Some), got {:?}", other),
        }
    }

    #[test]
    fn test_parse_site() {
        match FtpCommand::parse("SITE", Some("HELP")) {
            FtpCommand::Site(Some(s)) => assert_eq!(s, "HELP"),
            other => panic!("Expected Site(Some), got {:?}", other),
        }
    }
}
