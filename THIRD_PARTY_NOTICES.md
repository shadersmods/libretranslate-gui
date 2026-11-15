# Dioxus Components

The styled Button and Input wrappers and the Combobox composition in `src/components/mod.rs` are adapted from Dioxus Labs' component registry. Tabs, dialogs, switches, combobox filtering, selection behavior, keyboard navigation, and focus management use the official `dioxus-primitives` library directly.

Source: https://github.com/DioxusLabs/dioxus-components

Pinned revision: `9a758255ea26e2b20c8cecf4c1c946feb9e71da7`

Upstream authors: Dioxus Labs and DogeDark. Licensed under MIT OR Apache-2.0. The upstream license texts are included in `licenses/`.

## Dioxus desktop

The app uses unmodified Dioxus desktop 0.7.10 from crates.io. [Upstream source](https://github.com/DioxusLabs/dioxus/tree/v0.7.10/packages/desktop). Upstream authors: Dioxus Labs and contributors. Licensed under MIT OR Apache-2.0; the license texts are included in `licenses/dioxus-desktop-MIT.txt` and `licenses/dioxus-desktop-APACHE.txt`.

# Flag icons

Country and regional SVG files in `assets/flags/` come from [lipis/flag-icons](https://github.com/lipis/flag-icons), pinned to revision `ac7e0eb3259eb62aa6100c435607e2fe03896a97`. Copyright Lipis. MIT license included in `licenses/flag-icons-MIT.txt`. The Esperanto flag (`eo.svg`) is an app-authored addition. Flags are visual aids alongside explicit language names and codes.

# Interface icons

The app-authored interface SVGs are stored in `assets/icons/`. Rust contains only asset references; SVG markup is kept in those files. Dioxus copies these and the country flags into the distribution's asset directory.

# Argos package index

`assets/model-index.json` and the language list derive from the [official Argos package index](https://github.com/argosopentech/argospm-index). Copyright (c) 2020 Argos Open Technologies, LLC. The index is offered under MIT or CC0; this distribution includes the MIT license in `licenses/argospm-index-MIT.txt`. Model packages are downloaded separately and retain their own upstream metadata and licenses.

# Localized language names

The `language_*` names in the added interface catalogs use Unicode CLDR 47 data, obtained through ICU 77.1 via `Intl.DisplayNames`. Unicode, Inc. copyright and permission notice: `licenses/Unicode-3.0.txt`. Locale-specific UI messages are authored for this project.
