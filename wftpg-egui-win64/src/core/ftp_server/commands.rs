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
    OPTS(Option<String>, Option<String>),  // (option, value)
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
            },
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
