# Interface translations

The interface supports 20 languages: English, Romanian, Spanish, French, German, Brazilian Portuguese, European Portuguese, Russian, Simplified Chinese, Japanese, Korean, Arabic, Hindi, Hungarian, Italian, Polish, Dutch, Czech, Ukrainian, and Greek. Each JSON catalog has the same keys. Locale names are written in their own language. Language names used in the translation pickers come from Unicode CLDR 47 through ICU 77.1 (`Intl.DisplayNames`). See the Unicode notice in `licenses/`.

Use `en.json` as the meaning reference. Keep identifiers, `{max}`, `{name}`, `{version}`, `{language}`, `{value}`, paths, ports, limits, and product names intact. Translate complete messages rather than joining sentence fragments. Do not copy English word order into another language.

| Language | Project register and terminology |
| --- | --- |
| English | Direct action labels and short explanations. Technical terms stay when they describe a setting. |
| Romanian | Singular imperatives such as „Alege” and „Oprește”. Keep diacritics and distinguish the server log from task history. |
| Spanish | Neutral vocabulary, infinitive action labels, and direct instructions. Avoid regional slang and literal English constructions. |
| French | Infinitives for actions and vous forms in instructions. Use normal French accents and punctuation. |
| German | Infinitive action labels and Sie in instructions. Use familiar compounds and consistent runtime terminology. |
| Brazilian Portuguese | Brazilian vocabulary, infinitive action labels, and everyday instructions. Use salvar, arquivo, and pasta consistently. |
| Russian | Infinitive buttons and polite plural instructions. Keep technical names stable rather than rotating synonyms. |
| Simplified Chinese | Short verb labels and direct instructions. Use Chinese punctuation; retain Latin product names and protocol identifiers. |
| Japanese | Short action labels and polite explanatory sentences. Preserve necessary uncertainty, such as operations that may take several minutes. |
| Korean | Compact noun/action labels and polite instructions. Keep normal Korean spacing and familiar technical terms. |
| Arabic | Modern Standard Arabic, direct instructions, and right-to-left content. Keep URLs, code, numeric inputs, and paths left-to-right. |
| Hindi | Familiar software vocabulary and polite instructions. Avoid unnecessarily formal replacements for established terms such as model, cache, and thread. |
| Hungarian | Direct singular instructions, native compounds, and stable terminology for instances and runtime environments. |
| European Portuguese | Use guardar, ficheiro, registo, and predefinição. Keep European constructions rather than copying Brazilian progress labels. |
| Italian | Infinitive actions and direct singular instructions. Use registro and ambiente di esecuzione consistently. |
| Polish | Imperative action labels and clear instructions. Check agreement and avoid count-dependent wording around numeric placeholders. |
| Dutch | Infinitive actions and informal je instructions. Keep instance, runtime, and logbook terminology consistent. |
| Czech | Infinitive actions and polite plural instructions. Use the correct accents and check noun cases around settings and errors. |
| Ukrainian | Infinitive actions and polite plural instructions. Preserve Ukrainian spelling, use тека consistently, and keep protocol identifiers unchanged. |
| Greek | Compact noun/action labels and polite plural instructions. Keep accents and consistent terminology for instances and logs. |

These are project choices, informed by the [Microsoft localization guides](https://learn.microsoft.com/en-us/globalization/reference/microsoft-style-guides). They are not a claim of compliance with Microsoft's house style. The Avoid AI Writing patterns and detector are calibrated for English. For other languages, review clarity, register, terminology, repetition, grammar, and meaning in that language; do not translate the English word blacklist or treat its score as multilingual evidence.

The 0.2.6 audit checked all catalog entries. The English technical-context detector returned no findings before or after the changes. The existing English and Romanian wording was largely retained; the generic select-search labels and page-timeout wording were clarified. A corrective pass fixed an ambiguous model-removal sentence in Spanish, number-dependent limit hints, and a few terminology issues. Quote normalization ran on extracted authored or changed text, never on JSON syntax or the generated CLDR names. Mechanical preservation checks passed for the existing English and Romanian values.

Every catalog is checked for matching keys, nonempty values, and matching placeholders. Locale tests cover regional codes, serialization, legacy English/Romanian preferences, and Arabic direction. `pt` uses the available Brazilian Portuguese catalog. Traditional Chinese locales fall back to English because only Simplified Chinese is included.

Translation and naturalness review was model-assisted, without an independent native-speaker review. Developer diagnostics and upstream server logs retain their original wording; the surrounding interface and error summaries are localized. Identifiers such as CPU, CUDA, API, Prometheus, and version numbers remain unchanged where needed.

The 0.2.7 pass added eight catalogs and translated the new file-translation and app-update labels in every catalog. There are 331 keys per language, including the file-storage error added when the GUI's size cap was removed. A second targeted pass clarified a Hungarian version message and Dutch model-removal wording; unrelated existing translations were retained. Quote normalization made no changes to the first set of added text. Short labels receive unscored detector results; a combined technical-context check of the added English copy returned no issues. Regional tests distinguish `pt-PT` from `pt-BR`; legacy bare `pt` preferences keep their Brazilian Portuguese meaning. Review remains model-assisted rather than independent native-speaker validation.
