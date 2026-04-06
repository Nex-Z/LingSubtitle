use std::collections::BTreeMap;

use serde::Serialize;

pub const DEFAULT_BASE_URL: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";
pub const DEFAULT_MODEL: &str = "gummy-realtime-v1";
pub const DEFAULT_SAMPLE_RATE: u32 = 16_000;
pub const DEFAULT_VAD_SILENCE_MS: u32 = 800;
pub const DEFAULT_SOURCE_LANGUAGE: &str = "auto";
pub const DEFAULT_TARGET_LANGUAGE: &str = "en";

const SOURCE_LANGUAGES: [(&str, &str); 21] = [
    ("auto", "自动识别"),
    ("zh", "中文"),
    ("en", "英语"),
    ("ja", "日语"),
    ("ko", "韩语"),
    ("yue", "粤语"),
    ("de", "德语"),
    ("fr", "法语"),
    ("ru", "俄语"),
    ("es", "西班牙语"),
    ("it", "意大利语"),
    ("pt", "葡萄牙语"),
    ("id", "印尼语"),
    ("ar", "阿拉伯语"),
    ("th", "泰语"),
    ("hi", "印地语"),
    ("da", "丹麦语"),
    ("ur", "乌尔都语"),
    ("tr", "土耳其语"),
    ("nl", "荷兰语"),
    ("ms", "马来语"),
];

const EXTRA_TARGET_ONLY_LANGUAGES: [(&str, &str); 1] = [("vi", "越南语")];

const TARGET_LANGUAGE_MATRIX: [(&str, &[&str]); 20] = [
    ("zh", &["en", "ja", "ko", "fr", "de", "es", "ru", "it"]),
    (
        "en",
        &[
            "zh", "ja", "ko", "pt", "fr", "de", "ru", "vi", "es", "nl", "da", "ar", "it",
            "hi", "yue", "tr", "ms", "ur", "id",
        ],
    ),
    ("ja", &["th", "en", "zh", "vi", "fr", "it", "de", "es"]),
    ("ko", &["th", "en", "zh", "vi", "fr", "es", "ru", "de"]),
    ("fr", &["th", "en", "ja", "zh", "vi", "de", "it", "es", "ru", "pt"]),
    ("de", &["th", "en", "ja", "zh", "fr", "vi", "ru", "es", "it", "pt"]),
    ("es", &["th", "en", "ja", "zh", "fr", "vi", "it", "de", "ru", "pt"]),
    ("ru", &["th", "en", "ja", "zh", "fr", "vi", "de", "es", "it", "yue", "pt"]),
    ("it", &["th", "en", "ja", "zh", "fr", "vi", "es", "ru", "de"]),
    ("pt", &["en"]),
    ("id", &["en"]),
    ("ar", &["en"]),
    ("th", &["ja", "vi", "fr"]),
    ("hi", &["en"]),
    ("da", &["en"]),
    ("ur", &["en"]),
    ("tr", &["en"]),
    ("nl", &["en"]),
    ("ms", &["en"]),
    ("vi", &["ja", "fr"]),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GummyLanguageOption {
    pub code: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GummyDefaults {
    pub base_url: String,
    pub model: String,
    pub sample_rate: u32,
    pub vad_silence_ms: u32,
    pub source_language: String,
    pub target_language: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GummyCapabilities {
    pub source_languages: Vec<GummyLanguageOption>,
    pub target_languages_by_source: BTreeMap<String, Vec<GummyLanguageOption>>,
    pub defaults: GummyDefaults,
}

pub fn capabilities() -> GummyCapabilities {
    GummyCapabilities {
        source_languages: source_languages(),
        target_languages_by_source: target_languages_by_source(),
        defaults: GummyDefaults {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            sample_rate: DEFAULT_SAMPLE_RATE,
            vad_silence_ms: DEFAULT_VAD_SILENCE_MS,
            source_language: DEFAULT_SOURCE_LANGUAGE.to_string(),
            target_language: DEFAULT_TARGET_LANGUAGE.to_string(),
        },
    }
}

pub fn source_languages() -> Vec<GummyLanguageOption> {
    SOURCE_LANGUAGES
        .iter()
        .map(|(code, label)| language_option(code, label))
        .collect()
}

pub fn target_languages_for(source: &str) -> Vec<GummyLanguageOption> {
    let known_labels = language_label_map();
    TARGET_LANGUAGE_MATRIX
        .iter()
        .find(|(code, _)| *code == source)
        .map(|(_, targets)| {
            targets
                .iter()
                .filter_map(|code| known_labels.get(code).map(|label| language_option(code, label)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn is_valid_source_language(code: &str, allow_auto: bool) -> bool {
    if allow_auto && code == "auto" {
        return true;
    }

    SOURCE_LANGUAGES
        .iter()
        .any(|(candidate, _)| *candidate == code && *candidate != "auto")
}

pub fn is_valid_target_language(code: &str) -> bool {
    TARGET_LANGUAGE_MATRIX
        .iter()
        .flat_map(|(_, targets)| targets.iter())
        .any(|candidate| *candidate == code)
}

pub fn validate_language_selection(
    source_language: &str,
    translation_enabled: bool,
    target_language: &str,
) -> Result<(), String> {
    if translation_enabled {
        if source_language == "auto" {
            return Err("开启翻译时请先将识别语言改为明确语种，Gummy 不支持自动识别配合严格语言对校验。".to_string());
        }
        if !is_valid_source_language(source_language, false) {
            return Err("当前识别语言不在 Gummy 支持范围内。".to_string());
        }
        if !is_valid_target_language(target_language) {
            return Err("当前翻译目标语言不在 Gummy 支持范围内。".to_string());
        }
        let supported = target_languages_for(source_language);
        if !supported.iter().any(|option| option.code == target_language) {
            return Err(format!(
                "Gummy 当前不支持 {} -> {} 这组实时翻译语言，请重新选择。",
                language_label(source_language).unwrap_or(source_language),
                language_label(target_language).unwrap_or(target_language)
            ));
        }
    } else if !is_valid_source_language(source_language, true) {
        return Err("当前识别语言不在 Gummy 支持范围内。".to_string());
    }

    Ok(())
}

pub fn normalize_source_language(input: &str) -> Option<String> {
    normalize_language_code(input, true)
}

pub fn normalize_target_language(input: &str) -> Option<String> {
    normalize_language_code(input, false)
}

pub fn language_label(code: &str) -> Option<&'static str> {
    language_label_map().get(code).copied()
}

fn language_option(code: &str, label: &str) -> GummyLanguageOption {
    GummyLanguageOption {
        code: code.to_string(),
        label: label.to_string(),
    }
}

fn target_languages_by_source() -> BTreeMap<String, Vec<GummyLanguageOption>> {
    TARGET_LANGUAGE_MATRIX
        .iter()
        .map(|(source, _)| (source.to_string(), target_languages_for(source)))
        .collect()
}

fn language_label_map() -> BTreeMap<&'static str, &'static str> {
    SOURCE_LANGUAGES
        .iter()
        .chain(EXTRA_TARGET_ONLY_LANGUAGES.iter())
        .copied()
        .collect()
}

fn normalize_language_code(input: &str, allow_auto: bool) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    let aliases = [
        ("auto", "auto"),
        ("自动", "auto"),
        ("自动识别", "auto"),
        ("zh", "zh"),
        ("中文", "zh"),
        ("简体中文", "zh"),
        ("汉语", "zh"),
        ("en", "en"),
        ("english", "en"),
        ("英语", "en"),
        ("ja", "ja"),
        ("日本語", "ja"),
        ("日语", "ja"),
        ("日文", "ja"),
        ("ko", "ko"),
        ("한국어", "ko"),
        ("韩语", "ko"),
        ("de", "de"),
        ("deutsch", "de"),
        ("德语", "de"),
        ("fr", "fr"),
        ("francais", "fr"),
        ("français", "fr"),
        ("法语", "fr"),
        ("ru", "ru"),
        ("русский", "ru"),
        ("俄语", "ru"),
        ("es", "es"),
        ("espanol", "es"),
        ("español", "es"),
        ("西语", "es"),
        ("西班牙语", "es"),
        ("it", "it"),
        ("italiano", "it"),
        ("意大利语", "it"),
        ("pt", "pt"),
        ("português", "pt"),
        ("portugues", "pt"),
        ("葡萄牙语", "pt"),
        ("id", "id"),
        ("bahasa indonesia", "id"),
        ("印尼语", "id"),
        ("ar", "ar"),
        ("العربية", "ar"),
        ("阿拉伯语", "ar"),
        ("th", "th"),
        ("ไทย", "th"),
        ("泰语", "th"),
        ("yue", "yue"),
        ("粤语", "yue"),
        ("hi", "hi"),
        ("हिन्दी", "hi"),
        ("印地语", "hi"),
        ("da", "da"),
        ("dansk", "da"),
        ("丹麦语", "da"),
        ("ur", "ur"),
        ("اردو", "ur"),
        ("乌尔都语", "ur"),
        ("tr", "tr"),
        ("türkçe", "tr"),
        ("turkce", "tr"),
        ("土耳其语", "tr"),
        ("nl", "nl"),
        ("nederlands", "nl"),
        ("荷兰语", "nl"),
        ("ms", "ms"),
        ("bahasa melayu", "ms"),
        ("马来语", "ms"),
        ("vi", "vi"),
        ("tiếng việt", "vi"),
        ("tieng viet", "vi"),
        ("越南语", "vi"),
    ];

    let matched = aliases
        .iter()
        .find_map(|(alias, code)| (*alias == normalized).then_some(*code))
        .or_else(|| {
            let exact = trimmed.to_ascii_lowercase();
            aliases
                .iter()
                .find_map(|(alias, code)| (*alias == exact).then_some(*code))
        })?;

    if matched == "auto" && !allow_auto {
        return None;
    }

    Some(matched.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_source_language, normalize_target_language, target_languages_for,
        validate_language_selection,
    };

    #[test]
    fn normalizes_legacy_language_labels() {
        assert_eq!(normalize_source_language("中文").as_deref(), Some("zh"));
        assert_eq!(normalize_source_language("English").as_deref(), Some("en"));
        assert_eq!(normalize_target_language("日本語").as_deref(), Some("ja"));
        assert_eq!(normalize_target_language("Francais").as_deref(), Some("fr"));
    }

    #[test]
    fn rejects_auto_source_for_translation() {
        let error = validate_language_selection("auto", true, "en").unwrap_err();
        assert!(error.contains("开启翻译时"));
    }

    #[test]
    fn validates_supported_translation_pairs() {
        validate_language_selection("zh", true, "en").unwrap();
        validate_language_selection("en", true, "ur").unwrap();
        assert!(validate_language_selection("zh", true, "pt").is_err());
    }

    #[test]
    fn exposes_target_languages_for_source() {
        let targets = target_languages_for("zh");
        assert!(targets.iter().any(|item| item.code == "en"));
        assert!(targets.iter().any(|item| item.code == "ja"));
    }
}
