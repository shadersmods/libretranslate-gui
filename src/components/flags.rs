use dioxus::prelude::*;

// lipis/flag-icons; see THIRD_PARTY_NOTICES.md.
pub fn asset(code: &str) -> Option<Asset> {
    Some(match crate::i18n::model_code(code) {
        "ar" => asset!("/assets/countries/sa.svg"),
        "az" => asset!("/assets/countries/az.svg"),
        "bg" => asset!("/assets/countries/bg.svg"),
        "bn" => asset!("/assets/countries/bd.svg"),
        "ca" => asset!("/assets/countries/es-ct.svg"),
        "cs" => asset!("/assets/countries/cz.svg"),
        "da" => asset!("/assets/countries/dk.svg"),
        "de" => asset!("/assets/countries/de.svg"),
        "el" => asset!("/assets/countries/gr.svg"),
        "en" => asset!("/assets/countries/gb.svg"),
        "eo" => asset!("/assets/countries/eo.svg"),
        "es" => asset!("/assets/countries/es.svg"),
        "et" => asset!("/assets/countries/ee.svg"),
        "eu" => asset!("/assets/countries/es-pv.svg"),
        "fa" => asset!("/assets/countries/ir.svg"),
        "fi" => asset!("/assets/countries/fi.svg"),
        "fr" => asset!("/assets/countries/fr.svg"),
        "ga" => asset!("/assets/countries/ie.svg"),
        "gl" => asset!("/assets/countries/es-ga.svg"),
        "he" => asset!("/assets/countries/il.svg"),
        "hi" => asset!("/assets/countries/in.svg"),
        "hu" => asset!("/assets/countries/hu.svg"),
        "id" => asset!("/assets/countries/id.svg"),
        "it" => asset!("/assets/countries/it.svg"),
        "ja" => asset!("/assets/countries/jp.svg"),
        "ko" => asset!("/assets/countries/kr.svg"),
        "ky" => asset!("/assets/countries/kg.svg"),
        "lt" => asset!("/assets/countries/lt.svg"),
        "lv" => asset!("/assets/countries/lv.svg"),
        "ms" => asset!("/assets/countries/my.svg"),
        "nb" => asset!("/assets/countries/no.svg"),
        "nl" => asset!("/assets/countries/nl.svg"),
        "pb" => asset!("/assets/countries/br.svg"),
        "pl" => asset!("/assets/countries/pl.svg"),
        "pt" => asset!("/assets/countries/pt.svg"),
        "ro" => asset!("/assets/countries/ro.svg"),
        "ru" => asset!("/assets/countries/ru.svg"),
        "sk" => asset!("/assets/countries/sk.svg"),
        "sl" => asset!("/assets/countries/si.svg"),
        "sq" => asset!("/assets/countries/al.svg"),
        "sv" => asset!("/assets/countries/se.svg"),
        "sw" => asset!("/assets/countries/tz.svg"),
        "th" => asset!("/assets/countries/th.svg"),
        "tl" => asset!("/assets/countries/ph.svg"),
        "tr" => asset!("/assets/countries/tr.svg"),
        "uk" => asset!("/assets/countries/ua.svg"),
        "ur" => asset!("/assets/countries/pk.svg"),
        "vi" => asset!("/assets/countries/vn.svg"),
        "zh" => asset!("/assets/countries/cn.svg"),
        "zt" => asset!("/assets/countries/tw.svg"),
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
