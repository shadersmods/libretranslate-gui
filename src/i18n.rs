use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::LazyLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    #[default]
    En,
    Ro,
    Es,
    Fr,
    De,
    #[serde(rename = "pt-BR", alias = "pt", alias = "pt-br", alias = "pb")]
    PtBr,
    Ru,
    #[serde(rename = "zh-Hans", alias = "zh", alias = "zh-hans", alias = "zh-CN")]
    ZhHans,
    Ja,
    Ko,
    Ar,
    Hi,
    Hu,
    #[serde(rename = "pt-PT", alias = "pt-pt")]
    PtPt,
    It,
    Pl,
    Nl,
    Cs,
    Uk,
    El,
}
type Catalog = BTreeMap<String, String>;
const SOURCES: &[(Locale, &str)] = &[
    (Locale::En, include_str!("../locales/en.json")),
    (Locale::Ro, include_str!("../locales/ro.json")),
    (Locale::Es, include_str!("../locales/es.json")),
    (Locale::Fr, include_str!("../locales/fr.json")),
    (Locale::De, include_str!("../locales/de.json")),
    (Locale::PtBr, include_str!("../locales/pt-BR.json")),
    (Locale::Ru, include_str!("../locales/ru.json")),
    (Locale::ZhHans, include_str!("../locales/zh-Hans.json")),
    (Locale::Ja, include_str!("../locales/ja.json")),
    (Locale::Ko, include_str!("../locales/ko.json")),
    (Locale::Ar, include_str!("../locales/ar.json")),
    (Locale::Hi, include_str!("../locales/hi.json")),
    (Locale::Hu, include_str!("../locales/hu.json")),
    (Locale::PtPt, include_str!("../locales/pt-PT.json")),
    (Locale::It, include_str!("../locales/it.json")),
    (Locale::Pl, include_str!("../locales/pl.json")),
    (Locale::Nl, include_str!("../locales/nl.json")),
    (Locale::Cs, include_str!("../locales/cs.json")),
    (Locale::Uk, include_str!("../locales/uk.json")),
    (Locale::El, include_str!("../locales/el.json")),
];
static CATALOGS: LazyLock<BTreeMap<Locale, Catalog>> = LazyLock::new(|| {
    SOURCES
        .iter()
        .map(|(locale, source)| {
            (
                *locale,
                serde_json::from_str(source).expect("Invalid embedded translation catalog"),
            )
        })
        .collect()
});
impl Locale {
    pub const ALL: [Self; 20] = [
        Self::En,
        Self::Ro,
        Self::Es,
        Self::Fr,
        Self::De,
        Self::PtBr,
        Self::Ru,
        Self::ZhHans,
        Self::Ja,
        Self::Ko,
        Self::Ar,
        Self::Hi,
        Self::Hu,
        Self::PtPt,
        Self::It,
        Self::Pl,
        Self::Nl,
        Self::Cs,
        Self::Uk,
        Self::El,
    ];
    pub fn system() -> Self {
        Self::from_code(&sys_locale::get_locale().unwrap_or_default())
    }
    pub fn from_code(code: &str) -> Self {
        let normalized = code
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .replace('_', "-")
            .to_ascii_lowercase();
        let mut parts = normalized.split('-');
        match parts.next().unwrap_or_default() {
            "ro" => Self::Ro,
            "es" => Self::Es,
            "fr" => Self::Fr,
            "de" => Self::De,
            "pt" => {
                if parts.any(|part| part == "pt") {
                    Self::PtPt
                } else {
                    Self::PtBr
                }
            }
            "pb" => Self::PtBr,
            "ru" => Self::Ru,
            "zh" if !parts.any(|part| matches!(part, "hant" | "tw" | "hk" | "mo")) => Self::ZhHans,
            "ja" => Self::Ja,
            "ko" => Self::Ko,
            "ar" => Self::Ar,
            "hi" => Self::Hi,
            "hu" => Self::Hu,
            "it" => Self::It,
            "pl" => Self::Pl,
            "nl" => Self::Nl,
            "cs" => Self::Cs,
            "uk" => Self::Uk,
            "el" => Self::El,
            _ => Self::En,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ro => "ro",
            Self::Es => "es",
            Self::Fr => "fr",
            Self::De => "de",
            Self::PtBr => "pt-BR",
            Self::Ru => "ru",
            Self::ZhHans => "zh-Hans",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Ar => "ar",
            Self::Hi => "hi",
            Self::Hu => "hu",
            Self::PtPt => "pt-PT",
            Self::It => "it",
            Self::Pl => "pl",
            Self::Nl => "nl",
            Self::Cs => "cs",
            Self::Uk => "uk",
            Self::El => "el",
        }
    }
    pub fn translation_code(self) -> &'static str {
        match self {
            Self::PtBr => "pb",
            Self::PtPt => "pt",
            Self::ZhHans => "zh",
            _ => self.code(),
        }
    }
    pub fn direction(self) -> &'static str {
        if self == Self::Ar { "rtl" } else { "ltr" }
    }
    pub fn text(self, key: &str) -> &'static str {
        CATALOGS[&self]
            .get(key)
            .or_else(|| CATALOGS[&Self::En].get(key))
            .map(String::as_str)
            .unwrap_or("[missing translation]")
    }
    pub fn status(self, status: crate::manager::Status) -> &'static str {
        use crate::manager::Status::*;
        self.text(match status {
            NotInstalled => "not_installed",
            Installing => "installing",
            UpdatingModels => "updating_models",
            Stopped => "stopped",
            Starting => "starting",
            Running => "running",
            RemoteChecking => "remote_checking",
            RemoteOnline => "remote_online",
            RemoteOffline => "remote_offline",
            Unresponsive => "unresponsive",
            Failed => "failed",
        })
    }
}
/// Convert API language tags to Argos model identifiers used by catalogs and flags.
pub fn model_code(code: &str) -> &str {
    match code {
        "pt-BR" | "pt-br" => "pb",
        "pt-PT" | "pt-pt" => "pt",
        "zh-Hans" | "zh-hans" => "zh",
        "zh-Hant" | "zh-hant" => "zt",
        _ => code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn placeholders(text: &str) -> Vec<&str> {
        let mut tokens = text
            .split('{')
            .skip(1)
            .map(|part| part.split_once('}').expect("Unclosed placeholder").0)
            .collect::<Vec<_>>();
        tokens.sort_unstable();
        tokens
    }
    #[test]
    fn all_catalogs_have_matching_keys_values_and_placeholders() {
        let english = &CATALOGS[&Locale::En];
        assert_eq!(CATALOGS.len(), Locale::ALL.len());
        for locale in Locale::ALL {
            let catalog = &CATALOGS[&locale];
            assert_eq!(
                english.keys().collect::<Vec<_>>(),
                catalog.keys().collect::<Vec<_>>(),
                "Catalog keys: {locale:?}"
            );
            for (key, value) in catalog {
                assert!(
                    !value.trim().is_empty(),
                    "Empty {locale:?} translation: {key}"
                );
                assert_eq!(
                    placeholders(&english[key]),
                    placeholders(value),
                    "Placeholders: {locale:?}/{key}"
                );
            }
        }
    }
    #[test]
    fn regional_locales_and_saved_preferences_round_trip() {
        for (code, expected) in [
            ("ro_RO.UTF-8", Locale::Ro),
            ("ro-MD", Locale::Ro),
            ("ES_mx", Locale::Es),
            ("fr-CA", Locale::Fr),
            ("de-DE", Locale::De),
            ("pt_BR.UTF-8", Locale::PtBr),
            ("ru_RU", Locale::Ru),
            ("zh-CN", Locale::ZhHans),
            ("zh_Hans_SG", Locale::ZhHans),
            ("ja-JP", Locale::Ja),
            ("ko_KR", Locale::Ko),
            ("ar-EG", Locale::Ar),
            ("hi-IN", Locale::Hi),
            ("zh-Hant-TW", Locale::En),
            ("unknown", Locale::En),
        ] {
            assert_eq!(Locale::from_code(code), expected, "{code}");
        }
        for locale in Locale::ALL {
            assert_eq!(Locale::from_code(locale.code()), locale);
            let encoded = serde_json::to_string(&locale).unwrap();
            assert_eq!(serde_json::from_str::<Locale>(&encoded).unwrap(), locale);
            assert_eq!(encoded, format!("\"{}\"", locale.code()));
        }
        assert_eq!(
            serde_json::from_str::<Locale>("\"ro\"").unwrap(),
            Locale::Ro
        );
        assert_eq!(
            serde_json::from_str::<Locale>("\"pt\"").unwrap(),
            Locale::PtBr
        );
        assert_eq!(
            serde_json::from_str::<Locale>("\"zh\"").unwrap(),
            Locale::ZhHans
        );
    }
    #[test]
    fn directions_and_model_language_codes_match_the_locale() {
        assert_eq!(Locale::Ar.direction(), "rtl");
        for locale in Locale::ALL
            .into_iter()
            .filter(|locale| *locale != Locale::Ar)
        {
            assert_eq!(locale.direction(), "ltr");
        }
        assert_eq!(Locale::PtBr.translation_code(), "pb");
        assert_eq!(Locale::ZhHans.translation_code(), "zh");
    }
    #[test]
    fn ui_literal_keys_resolve_in_every_catalog() {
        for source in [
            include_str!("app.rs"),
            include_str!("state.rs"),
            include_str!("advanced.rs"),
            include_str!("model_ui.rs"),
            include_str!("components/mod.rs"),
            include_str!("tasks.rs"),
            include_str!("statistics.rs"),
            include_str!("translator.rs"),
            include_str!("translator_files.rs"),
            include_str!("app_updates.rs"),
            include_str!("self_update.rs"),
        ] {
            for suffix in source
                .match_indices(".text(\"")
                .filter(|(index, _)| source[..*index].trim_end().ends_with("locale"))
                .map(|(index, _)| &source[index + 7..])
                .chain(
                    source
                        .split("label_key:")
                        .skip(1)
                        .filter_map(|value| value.trim().strip_prefix('"')),
                )
            {
                let key = suffix.split('"').next().unwrap();
                for locale in Locale::ALL {
                    assert!(
                        CATALOGS[&locale].contains_key(key),
                        "Missing {locale:?} UI translation: {key}"
                    );
                }
            }
        }
    }
}
