# Contributing

Bug reports, code changes, translations, and documentation fixes are welcome. Follow the [Code of Conduct](CODE_OF_CONDUCT.md) in project discussions. Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## Before making a change

Search [existing issues](https://github.com/shadersmods/libretranslate-gui/issues) and pull requests for related work. For a large feature or a change to storage, installation, or update behavior, open an issue first so the approach can be discussed before you spend time implementing it. Small fixes can go straight to a pull request.

Use the bug report form for broken behavior. Include the app version, operating system, package type, steps to reproduce, and relevant logs. Remove API keys, tokens, private text, and identifying file paths from anything you attach. A small sample file that reproduces a translation problem is more useful than a private document.

## Set up a development environment

You need Rust 1.88 or newer and the native dependencies listed in the [build guide](docs/usage.md#building). Python 3.11 is used for the repository's Python checks and packaging scripts. The application manages its own Python runtime for translation.

Fork the repository, clone your fork, and create a branch from `main`. Install the pinned Dioxus CLI and start the desktop app:

```sh
cargo install dioxus-cli --version 0.7.10 --locked
dx serve --platform desktop
```

Use `dx` for desktop development: it prepares the assets and component scripts the UI needs. A plain `cargo run` does not produce the complete app bundle. A release build uses:

```sh
dx build --platform desktop --release --locked
```

Keep development runs separate from instances you use for real work. `LIBRETRANSLATE_GUI_SETTINGS_FILE` and `LIBRETRANSLATE_GUI_DATA_DIR` can point to a disposable settings file and data folder; see the [storage guide](docs/usage.md#storage). Do not commit runtime environments, downloaded models, preferences, logs, or build output.

## Make and check your change

Keep each pull request focused on one problem. Match the surrounding code, explain changes to storage or compatibility, and add a regression test when it helps demonstrate the fix. Keep dependency changes intentional and include `Cargo.lock` when dependencies change. Preserve third-party notices when adding or replacing bundled code or assets.

For UI changes, check light and dark themes, a narrow window, keyboard navigation, and Arabic's right-to-left layout when relevant. Add screenshots when they help a reviewer assess the result. Keep user-facing messages in the locale catalogs. Follow [the translation notes](locales/README.md), preserve placeholders, and keep catalog keys in sync.

The CI checks are:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python tests/model_sync.py
python tests/release_packaging.py
python tests/windows_installer.py
python tests/windows_msix.py
```

The default tests do not require a GPU or download translation models. Optional runtime tests are documented in the [verification guide](docs/usage.md#verification). On Windows, installer changes also need the MSI smoke test in a Visual Studio developer shell:

```powershell
python tests/windows_installer.py --smoke
```

Packaging changes should also be checked with the relevant [package build commands](docs/usage.md#building). Windows checks do not establish macOS or Linux compatibility. State which platforms you tested and which checks you could not run.

## Open a pull request

Target `main` and use a title that describes the change. Explain the problem, the resulting behavior, any related issue, and how you checked the result. Include compatibility concerns or follow-up work when relevant. Update the user guide if the change affects setup, controls, storage, or supported behavior.

Do not include a version bump, release tag, or generated packages unless the change is specifically about preparing a release. Maintainers handle publishing. Reviews may ask for changes or more evidence; small, focused follow-up commits make that easier.
