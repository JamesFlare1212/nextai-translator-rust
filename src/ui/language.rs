use gpui_kit::{SharedString, component::select::SelectItem};
use std::{collections::HashMap, sync::LazyLock};
use unicode_script::{Script, UnicodeScript};

#[derive(Clone, Copy)]
pub(super) struct Language(
    pub(super) &'static str,
    pub(super) &'static str,
    pub(super) Option<&'static str>,
);

impl SelectItem for Language {
    type Value = &'static str;
    fn title(&self) -> SharedString {
        self.1.into()
    }
    fn value(&self) -> &Self::Value {
        &self.0
    }
    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.0.to_lowercase().contains(&query) || self.1.to_lowercase().contains(&query)
    }
}

pub(super) const AUTO: Language = Language("Auto", "自动检测", None);
pub(super) const CHINESE: &str = "Simplified Chinese";
pub(super) const ENGLISH: &str = "English";
pub(super) const LANGUAGES: &[Language] = &[
    Language("English", "English", None),
    Language("English (US)", "American English", Some("English")),
    Language("English (UK)", "British English", Some("English")),
    Language("English (Canada)", "Canadian English", Some("English")),
    Language("English (Australia)", "Australian English", Some("English")),
    Language("Simplified Chinese", "简体中文", None),
    Language("Traditional Chinese", "繁體中文", None),
    Language("Cantonese", "粤语", Some("Simplified Chinese")),
    Language("Classical Chinese", "古文", None),
    Language(
        "Modern Standard Chinese",
        "近代白话文",
        Some("Simplified Chinese"),
    ),
    Language(
        "Contemporary Chinese",
        "现代白话文",
        Some("Simplified Chinese"),
    ),
    Language("Japanese", "日本語", None),
    Language("Korean", "한국어", None),
    Language("Informal Korean", "한국어 반말", Some("Korean")),
    Language("French", "Français", None),
    Language("German", "Deutsch", None),
    Language("Spanish", "Español", None),
    Language("Italian", "Italiano", None),
    Language("Russian", "Русский", None),
    Language("Portuguese", "Português", None),
    Language("Dutch", "Nederlands", None),
    Language("Polish", "Polski", None),
    Language("Arabic", "العربية", None),
    Language("Afrikaans", "Afrikaans", None),
    Language("Amharic", "አማርኛ", None),
    Language("Azerbaijani", "Azərbaycan", None),
    Language("Belarusian", "Беларуская", None),
    Language("Bulgarian", "Български", None),
    Language("Bengali", "বাংলা", None),
    Language("Bosnian", "Bosanski", None),
    Language("Catalan", "Català", None),
    Language("Cebuano", "Cebuano", None),
    Language("Corsican", "Corsu", None),
    Language("Czech", "Čeština", None),
    Language("Welsh", "Cymraeg", None),
    Language("Danish", "Dansk", None),
    Language("Greek", "Ελληνικά", None),
    Language("Esperanto", "Esperanto", None),
    Language("Estonian", "Eesti", None),
    Language("Basque", "Euskara", None),
    Language("Persian", "فارسی", None),
    Language("Finnish", "Suomi", None),
    Language("Fijian", "Fijian", None),
    Language("Frisian", "Frysk", None),
    Language("Irish", "Gaeilge", None),
    Language("Scottish Gaelic", "Gàidhlig", None),
    Language("Galician", "Galego", None),
    Language("Gujarati", "ગુજરાતી", None),
    Language("Hausa", "Hausa", None),
    Language("Hawaiian", "Hawaiʻi", None),
    Language("Hebrew", "עברית", None),
    Language("Hindi", "हिन्दी", None),
    Language("Hmong", "Hmong", None),
    Language("Croatian", "Hrvatski", None),
    Language("Haitian Creole", "Kreyòl Ayisyen", None),
    Language("Hungarian", "Magyar", None),
    Language("Armenian", "Հայերեն", None),
    Language("Indonesian", "Bahasa Indonesia", None),
    Language("Igbo", "Igbo", None),
    Language("Icelandic", "Íslenska", None),
    Language("Javanese", "Jawa", None),
    Language("Georgian", "ქართული", None),
    Language("Kazakh", "Қазақ", None),
    Language("Mongolian", "Монгол хэл", None),
    Language("Turkish", "Türkçe", None),
    Language("Uyghur", "ئۇيغۇر تىلى", None),
    Language("Ukrainian", "Українська", None),
    Language("Urdu", "اردو", None),
    Language("Vietnamese", "Tiếng Việt", None),
    Language("Swedish", "Svenska", None),
    Language("Thai", "ไทย", None),
];

pub(super) fn language(name: &str) -> Option<Language> {
    LANGUAGES.iter().copied().find(|l| l.0 == name)
}

pub(super) fn detect(text: &str) -> Option<&'static str> {
    let bounded: String = text.chars().take(4096).collect();
    let sample = bounded
        .lines()
        .filter(|line| !line.trim_start().starts_with("{{<"))
        .collect::<Vec<_>>()
        .join(" ");
    let scripts = || sample.chars().map(|c| c.script());
    if scripts().any(|s| matches!(s, Script::Hiragana | Script::Katakana)) {
        return Some("Japanese");
    }
    if scripts().any(|s| s == Script::Hangul) {
        return Some("Korean");
    }
    let han = scripts().filter(|&s| s == Script::Han).count();
    let info = whatlang::detect(&sample);
    if han > 0
        && (han * 2 >= sample.chars().filter(|c| c.is_alphabetic()).count()
            || info.as_ref().is_some_and(|i| i.lang().code() == "cmn"))
    {
        return Some(if han_score(&sample) > 0 {
            "Traditional Chinese"
        } else {
            CHINESE
        });
    }
    let info = info?;
    if !info.is_reliable() {
        let common_english = sample.split(|c: char| !c.is_ascii_alphabetic()).any(|w| {
            matches!(
                w.to_ascii_lowercase().as_str(),
                "the" | "hello" | "thanks" | "this" | "that" | "please" | "you" | "with"
            )
        });
        return (sample.is_ascii() && (info.lang() == whatlang::Lang::Eng || common_english))
            .then_some(ENGLISH);
    }
    language(info.lang().eng_name()).map(|l| l.0)
}

fn han_score(s: &str) -> i32 {
    static VARIANTS: LazyLock<HashMap<char, i32>> = LazyLock::new(|| {
        let mut lines = include_str!("../../assets/han-variants.txt").lines();
        let simple = lines.next().unwrap();
        let traditional = lines.next().unwrap();
        traditional
            .chars()
            .map(|c| (c, 1))
            .chain(simple.chars().map(|c| (c, -1)))
            .collect()
    });
    s.chars().filter_map(|c| VARIANTS.get(&c)).sum()
}

pub(super) fn automatic_target(
    source: Option<&str>,
    preferred: &'static str,
    reverse: &'static str,
) -> &'static str {
    let chinese = |s: &str| {
        matches!(
            s,
            "Simplified Chinese" | "Traditional Chinese" | "Classical Chinese"
        )
    };
    if source.is_some_and(|s| s == preferred || (chinese(s) && chinese(preferred))) {
        reverse
    } else {
        preferred
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_extended_and_halfwidth_scripts_without_treating_punctuation_as_kana() {
        assert_eq!(detect("𠮷"), Some(CHINESE));
        assert_eq!(detect("ｺﾝﾆﾁﾊ"), Some("Japanese"));
        assert_eq!(detect("ㄱㅏ"), Some("Korean"));
        assert_eq!(detect("简体中文・翻译"), Some(CHINESE));
    }
}
