use dioxus::prelude::*;

// lipis/flag-icons; see THIRD_PARTY_NOTICES.md.
pub fn asset(code: &str) -> Option<Asset> {
    Some(match crate::i18n::model_code(code) {
        "ar" => asset!("/assets/flags/sa.svg"),
        "az" => asset!("/assets/flags/az.svg"),
        "bg" => asset!("/assets/flags/bg.svg"),
        "bn" => asset!("/assets/flags/bd.svg"),
        "ca" => asset!("/assets/flags/es-ct.svg"),
        "cs" => asset!("/assets/flags/cz.svg"),
        "da" => asset!("/assets/flags/dk.svg"),
        "de" => asset!("/assets/flags/de.svg"),
        "el" => asset!("/assets/flags/gr.svg"),
        "en" => asset!("/assets/flags/gb.svg"),
        "eo" => asset!("/assets/flags/eo.svg"),
        "es" => asset!("/assets/flags/es.svg"),
        "et" => asset!("/assets/flags/ee.svg"),
        "eu" => asset!("/assets/flags/es-pv.svg"),
        "fa" => asset!("/assets/flags/ir.svg"),
        "fi" => asset!("/assets/flags/fi.svg"),
        "fr" => asset!("/assets/flags/fr.svg"),
        "ga" => asset!("/assets/flags/ie.svg"),
        "gl" => asset!("/assets/flags/es-ga.svg"),
        "he" => asset!("/assets/flags/il.svg"),
        "hi" => asset!("/assets/flags/in.svg"),
        "hu" => asset!("/assets/flags/hu.svg"),
        "id" => asset!("/assets/flags/id.svg"),
        "it" => asset!("/assets/flags/it.svg"),
        "ja" => asset!("/assets/flags/jp.svg"),
        "ko" => asset!("/assets/flags/kr.svg"),
        "ky" => asset!("/assets/flags/kg.svg"),
        "lt" => asset!("/assets/flags/lt.svg"),
        "lv" => asset!("/assets/flags/lv.svg"),
        "ms" => asset!("/assets/flags/my.svg"),
        "nb" => asset!("/assets/flags/no.svg"),
        "nl" => asset!("/assets/flags/nl.svg"),
        "pb" => asset!("/assets/flags/br.svg"),
        "pl" => asset!("/assets/flags/pl.svg"),
        "pt" => asset!("/assets/flags/pt.svg"),
        "ro" => asset!("/assets/flags/ro.svg"),
        "ru" => asset!("/assets/flags/ru.svg"),
        "sk" => asset!("/assets/flags/sk.svg"),
        "sl" => asset!("/assets/flags/si.svg"),
        "sq" => asset!("/assets/flags/al.svg"),
        "sv" => asset!("/assets/flags/se.svg"),
        "sw" => asset!("/assets/flags/tz.svg"),
        "th" => asset!("/assets/flags/th.svg"),
        "tl" => asset!("/assets/flags/ph.svg"),
        "tr" => asset!("/assets/flags/tr.svg"),
        "uk" => asset!("/assets/flags/ua.svg"),
        "ur" => asset!("/assets/flags/pk.svg"),
        "vi" => asset!("/assets/flags/vn.svg"),
        "zh" => asset!("/assets/flags/cn.svg"),
        "zt" => asset!("/assets/flags/tw.svg"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn canonical_api_tags_share_the_model_flag() {
        for (api, model) in [("pt-BR", "pb"), ("zh-Hans", "zh"), ("zh-Hant", "zt")] {
            assert_eq!(super::asset(api), super::asset(model));
        }
    }
}
