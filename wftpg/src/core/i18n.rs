use parking_lot::RwLock;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Zh,
}

impl Language {
    pub fn code(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Zh => "zh",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "zh" | "zh-CN" | "zh_cn" | "zh-Hans" => Language::Zh,
            _ => Language::En,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Zh => "中文",
        }
    }

    pub fn all() -> &'static [Language] {
        &[Language::En, Language::Zh]
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}

type TranslationMap = HashMap<String, String>;
type LogMap = HashMap<String, String>;

struct I18nState {
    language: Language,
    translations: HashMap<Language, TranslationMap>,
    log_map: HashMap<Language, LogMap>,
}

static I18N: OnceLock<RwLock<I18nState>> = OnceLock::new();

fn load_json_map(json_str: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_str)
        && let Some(obj) = value.as_object()
    {
        for (key, val) in obj {
            if let Some(s) = val.as_str() {
                map.insert(key.clone(), s.to_string());
            }
        }
    }
    map
}

fn i18n() -> &'static RwLock<I18nState> {
    I18N.get_or_init(|| {
        let en_ui: HashMap<String, String> = load_json_map(include_str!("../../i18n/en/ui.json"));
        let en_logs: HashMap<String, String> =
            load_json_map(include_str!("../../i18n/en/logs.json"));
        let zh_ui: HashMap<String, String> = load_json_map(include_str!("../../i18n/zh/ui.json"));
        let zh_logs: HashMap<String, String> =
            load_json_map(include_str!("../../i18n/zh/logs.json"));

        let mut translations = HashMap::new();
        let mut log_map = HashMap::new();

        translations.insert(Language::En, en_ui);
        log_map.insert(Language::En, en_logs);

        translations.insert(Language::Zh, zh_ui);
        log_map.insert(Language::Zh, zh_logs);

        RwLock::new(I18nState {
            language: Language::Zh,
            translations,
            log_map,
        })
    })
}

pub fn init(language: Language) {
    set_language(language);
}

pub fn set_language(lang: Language) {
    i18n().write().language = lang;
}

pub fn current_language() -> Language {
    i18n().read().language
}

pub fn t(key: &str) -> String {
    let state = i18n().read();
    if let Some(trans) = state.translations.get(&state.language)
        && let Some(value) = trans.get(key)
    {
        return value.clone();
    }
    if state.language != Language::En
        && let Some(trans) = state.translations.get(&Language::En)
        && let Some(value) = trans.get(key)
    {
        return value.clone();
    }
    key.to_string()
}

pub fn t_fmt(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    let template = t(key);
    let mut result = template;
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{}}}", i), &arg.to_string());
    }
    result
}

pub fn map_log(msg: &str) -> String {
    let state = i18n().read();
    if state.language == Language::En {
        return msg.to_string();
    }
    if let Some(log_map) = state.log_map.get(&state.language)
        && let Some(translated) = log_map.get(msg)
    {
        return translated.clone();
    }

    if let Some(log_map) = state.log_map.get(&state.language) {
        for (pattern, translation) in log_map.iter() {
            if (pattern.contains("{0}") || pattern.contains("{1}") || pattern.contains("{2}"))
                && let Some(translated) = match_parameterized_message(msg, pattern, translation)
            {
                return translated;
            }
        }
    }

    msg.to_string()
}

fn match_parameterized_message(msg: &str, pattern: &str, translation: &str) -> Option<String> {
    let mut regex_pattern = regex::escape(pattern);

    let placeholder_count = pattern.matches('{').count();
    for i in 0..placeholder_count {
        regex_pattern = regex_pattern.replace(&format!("\\{{{}\\}}", i), "(.+?)");
    }

    let re = regex::Regex::new(&format!("^{}$", regex_pattern)).ok()?;
    let caps = re.captures(msg)?;

    let mut result = translation.to_string();
    for i in 0..placeholder_count {
        if let Some(matched) = caps.get(i + 1) {
            result = result.replace(&format!("{{{}}}", i), matched.as_str());
        }
    }

    Some(result)
}
