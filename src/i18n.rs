use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "it")]
    Italian,
}

impl Language {
    pub const ALL: [Self; 6] = [
        Self::English,
        Self::Russian,
        Self::French,
        Self::German,
        Self::Spanish,
        Self::Italian,
    ];
    pub fn name(self) -> &'static str {
        [
            "English",
            "Русский",
            "Français",
            "Deutsch",
            "Español",
            "Italiano",
        ][self as usize]
    }
    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }
    pub fn code(self) -> &'static str {
        ["en", "ru", "fr", "de", "es", "it"][self as usize]
    }
    pub fn text(self, english: &str) -> &str {
        if self == Self::English {
            return english;
        }
        catalog().get(english).map_or(english, |translations| {
            translations[self as usize - 1].as_str()
        })
    }
    pub fn format(self, english: &str, values: &[(&str, String)]) -> String {
        let template = self.text(english);
        let mut text = String::with_capacity(template.len());
        let mut remainder = template;
        while let Some((before, after)) = remainder.split_once('{') {
            text.push_str(before);
            let Some((key, rest)) = after.split_once('}') else {
                text.push('{');
                text.push_str(after);
                return text;
            };
            if let Some((_, value)) = values.iter().find(|(name, _)| *name == key) {
                text.push_str(value);
            } else {
                text.push('{');
                text.push_str(key);
                text.push('}');
            }
            remainder = rest;
        }
        text.push_str(remainder);
        text
    }
}

fn catalog() -> &'static BTreeMap<String, [String; 5]> {
    static CATALOG: OnceLock<BTreeMap<String, [String; 5]>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("locales.json")).expect("validated embedded translations")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn placeholders(text: &str) -> Vec<&str> {
        let mut result = vec![];
        let mut remainder = text;
        while let Some((_, after)) = remainder.split_once('{') {
            let (placeholder, rest) = after.split_once('}').expect("unclosed placeholder");
            result.push(placeholder);
            remainder = rest;
        }
        result.sort_unstable();
        result
    }
    #[test]
    fn every_translation_is_present_and_preserves_parameters() {
        assert!(catalog().len() > 90);
        for (english, translations) in catalog() {
            for translated in translations {
                assert!(!translated.trim().is_empty(), "{english}");
                assert_eq!(
                    placeholders(english),
                    placeholders(translated),
                    "{english}: {translated}"
                );
            }
        }
    }
    #[test]
    fn values_are_never_translated_or_reinterpreted() {
        let value = "C:\\Русский\\Settings.exe {count}";
        assert_eq!(
            Language::French.format("Saved to {path}", &[("path", value.into())]),
            format!("Enregistré dans {value}")
        );
        assert_eq!(Language::German.text(value), value);
        assert_eq!(
            Language::English.format(
                "{first} / {second}",
                &[("first", "{second}".into()), ("second", "safe".into())]
            ),
            "{second} / safe"
        );
    }
}
