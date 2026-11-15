# LibreTranslate GUI usage guide

A Rust desktop application for managing local LibreTranslate servers on Windows, macOS, and Linux. The interface uses Dioxus and the official Dioxus Components primitives, with a branded sidebar, an instance library, custom selectors, tabs, and dialogs. End users do not need Docker, Node.js, Rust, or manually installed Python. Dioxus uses the operating system's WebView.

LibreTranslate remains the upstream Python application. The manager downloads a private, pinned uv binary, verifies its release archive checksum, and provisions Python 3.11 and the selected LibreTranslate release in its application data directory. The default release is 1.9.6.

## Using the app

1. Open `libretranslate-gui` (or `libretranslate-gui.exe` on Windows).
2. Select **Add instance**, choose a name, port, translation languages, worker count, and CPU or CUDA, then **Create instance**. The language picker adds/removes rows, prevents duplicates, and keeps at least two languages.
3. Select **Install runtime**. Setup installs the runtime and selected language models without starting the server. Follow download and setup output in the live log.
4. Select **Start server**. Status becomes **Running** when the `/languages` API returns a valid language list.
5. Open an instance to use **Translator**, the first tab. Choose source and target languages and enter text. Translation starts after a short typing pause. **Files** lets you drop a supported file into the selection area or choose it with the file picker, then translate it and save the result. Both modes use the selected instance's API key when needed.

The instance library provides search, process status, and lifecycle controls. The **Options** page configures storage and appearance. The 20 interface languages are English, Romanian, Spanish, French, German, Brazilian Portuguese, European Portuguese, Russian, Simplified Chinese, Japanese, Korean, Arabic, Hindi, Hungarian, Italian, Polish, Dutch, Czech, Ukrainian, and Greek. The searchable picker shows native language names and flags. Arabic uses right-to-left content while URLs, paths, and native window controls keep their usual direction. Language and **Light / Dark / System** preferences are saved beside the executable. Translation languages are separate from the interface language.

**Add all** adds every supported translation language once. Existing valid configuration changes use the same automatic saving as individual language edits. Missing models download when the server next starts.

The title-bar **Tasks** menu shows active setup work and model downloads, including transferred bytes and totals when available. Finished, failed, and stopped transfers remain in the session history. Dismiss or clear history to remove those records; downloaded files stay on disk. The Models tab keeps the model inventory and update controls.

Enabling **Prometheus metrics** in Advanced adds a **Statistics** tab. It reads request totals, failed requests, weighted average response time, in-flight requests, and per-endpoint totals from the instance's metrics endpoint. It uses the configured metrics token and URL prefix. The tab refreshes every five seconds while open and reports unavailable or invalid data instead of displaying zero. Client addresses and API-key labels are not shown.

The in-app translator uses the running server's actual language list, supports automatic language detection, and retains a separate text draft for each instance during the session. Text translation waits 600 ms after a change, runs one request at a time, waits for input-method composition to finish, and discards stale responses. Reopening an unchanged completed draft does not send another request. The controls are disabled while the server is offline or uninstalled. File mode reads the server's supported formats and streams uploads and downloads. Translated results use temporary files in the configured cache folder's `translations/` directory and stay available when switching tabs or instances during the session. The app has no file size limit; server request limits, timeouts, and format restrictions still apply. A native save dialog exports the result. The original file is left untouched. File translation must be enabled in Advanced. Translation requests and file downloads stay on the selected local instance; the browser is not required.

At launch, the app checks published stable releases from [its GitHub repository](https://github.com/shadersmods/libretranslate-gui/releases). A newer release opens a dialog with the version and changelog; **Not now** leaves the current version running. **Update and restart** downloads the matching portable package, verifies SHA-256 using GitHub's asset digest or the release's checksum file, and stages the program files. A helper waits for the app to close, replaces the executable and packaged assets, and reopens the app. Closing stops running servers. Instance data, preferences, and unrelated files are preserved. If replacement fails, changed program files are rolled back; backups remain under `.updates/`. The program folder must be writable. About includes a manual update check. Missing releases and failed checks are reported without inventing a newer version.

Stop an instance before editing settings, repairing its runtime, or removing it. Configuration changes save automatically after a short pause; invalid values stay in the form without replacing the saved configuration. The port field reports availability and can find a free port. The thread limit follows the current computer's logical CPU count. Removal asks for confirmation and deletes that instance's environment and models. On the next start, added languages download their missing models; deselected languages have their installed translation models, previous model versions, and private sentence models removed. Shared download caches remain available.

Servers bind to `127.0.0.1` by default. **Advanced** can enable local-network access, request limits, API keys, translator-page options, a URL prefix, translation caching, model updates on startup, and protected metrics. Character and batch limits are in **Configuration**. Generated API keys and metrics tokens are stored in the instance configuration; they are passed to the server through its environment rather than command-line arguments.

**Advanced → LibreTranslate version** selects a stable release from 1.9.1 onward for each instance. It shows installed, selected, and latest versions separately. The app checks the official PyPI catalog on launch and hourly, with a manual check and a cached release list for offline use. It flags outdated installed releases without changing your selection. Stop the instance and select **Install selected version** to apply a change; the installer verifies the installed package version before marking the runtime ready. Language models stay in their existing folder.

Hovering a sidebar instance shows its name, endpoint, language flags, status, and device in a compact preview. **Options → About** credits Neo and links to [the GitHub repository](https://github.com/shadersmods/libretranslate-gui).

Closing the manager waits for pending edits to be saved, then stops servers and cancels installation. Native close requests hide the window while saving; if validation or saving fails, it reopens with the draft and error so you can correct the problem and retry. The app's own close button keeps the window visible until saving succeeds. Shutdown during an installation download can wait for the current request to finish or time out (up to 60 seconds). Windows Job Objects and Unix process groups supervise runtime subprocesses. A second manager cannot open the same data directory concurrently.

## Translation models

The **Models** tab lists direct translation pairs for the selected languages, with installed and available model versions, installed disk usage, and download sizes reported by the model host. These are model versions, separate from the LibreTranslate application version. The catalog comes from the official Argos package index; a cached copy is available offline. Unknown download sizes are shown as unavailable.

During installation, startup, and model updates, **Tasks** shows each translation or sentence model with live downloaded bytes, total bytes when the host provides them, and progress. It distinguishes queued, downloading, validation, ready, failed, and stopped states. Switching tabs keeps the current progress; stopping preserves the transferred count. Downloads stream to partial files and are committed after the transfer finishes. Progress is kept for the current session and resets when a new operation begins.

Use **Refresh catalog** to check available versions. With the instance stopped and its runtime installed, **Update models** downloads missing or newer pairs and sentence-boundary models. The updater validates translation and ONNX sentence models before committing downloads. It keeps one previous model version per language pair under `model-transactions/`; interrupted replacements are recovered before the next update or server start. Allow space for the current model, its backup, and the download being staged. **Clean up storage** removes obsolete backups, old model archives, and inactive metrics runs while keeping the latest model backup. The Models tab also shows total instance storage. **Update models on startup** uses the same validated updater before starting the server.

Every startup synchronizes models with the selected languages, even when other models are already installed. Required downloads finish and pass validation before deselected models are removed. A failed download leaves the previous models intact and reports the failure in Logs. Ordinary starts keep existing model versions; enable startup updates or use **Update models** to upgrade them.

## Building

Install Rust 1.88 or newer from [rustup](https://rustup.rs), then:

```sh
cargo install dioxus-cli --version 0.7.10 --locked
dx serve --platform desktop
```

Build the portable desktop distribution (Windows builds statically link the C runtime):

```sh
dx build --platform desktop --release --locked
```

Keep the generated executable and its `assets/` directory together under `target/dx/libretranslate-gui/release/`. The Dioxus CLI processes both app assets and the official components' focus-management JavaScript; plain `cargo run` does not produce a complete UI bundle.

Developer requirements:

- **Windows:** Visual Studio Build Tools with **Desktop development with C++** and a Windows SDK.
- **macOS:** Apple command line tools (`xcode-select --install`).
- **Ubuntu/Debian:** `build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev libxdo-dev libssl-dev`. A graphical session is required.

Windows needs the Microsoft Edge WebView2 runtime, normally included with modern Windows. Linux needs the GTK/WebKitGTK runtime libraries corresponding to the development packages above. macOS uses system WebKit. The app uses official Dioxus 0.7.10 from crates.io, with dependency versions locked in `Cargo.lock`; no framework fork or local `vendor/` directory is required. Dioxus Components is pinned separately to its official upstream Git revision. Use `dx bundle --platform desktop --release` for native packages; `Dioxus.toml` supplies the bundle icon and metadata.

GitHub Actions checks formatting, lints, runs tests, builds releases on all three systems, and uploads executable artifacts. These are unsigned executables, not installer packages. macOS and Linux require validation on their own systems; a Windows build alone does not establish compatibility there. ARM64 runtime downloads are implemented but are outside the current CI matrix.

## Storage

By default, the application is portable: `data/` and `libretranslate-desktop.json` live **beside the executable**. Relative paths are resolved against the executable's folder, not the shell's working directory.

In **Options**, choose the instance-data folder, shared download cache, and shared managed-Python folder. Folder pickers and typed paths are supported. Empty cache/Python fields use `cache/` and `python/` inside the data folder. Saving validates that folders are writable. **Storage changes take effect after closing and reopening the application. Existing files are not moved or deleted automatically.** Python environments can contain absolute paths: reuse an existing environment in its original location, or reinstall it in a new location rather than copying it arbitrarily.

The active data path and preferences path can be copied from Options. `LIBRETRANSLATE_GUI_DATA_DIR` overrides the selected data location; `LIBRETRANSLATE_GUI_SETTINGS_FILE` overrides the preferences file. The executable folder must be writable for portable defaults; move the application to a writable folder or set those overrides if installed in a protected location.

Existing installations from version 0.1 used the platform's application-data directory. Select that existing folder in Options and restart to keep using its instances. The new app does not silently move it.

```text
beside the executable/
  libretranslate-desktop.json # Locale, theme, storage paths, log preference
  assets/                    # Packaged flags, icons, component scripts
  data/
    instances.json           # Settings, written atomically
    manager.lock             # Prevents concurrent managers
    tools/                   # Private uv binary
    python/                  # Shared managed Python
    cache/                   # Shared package cache
    instances/<uuid>/
      venv/                  # Legacy installation, still supported
      runtime-active.json    # Selects the validated private runtime
      <runtime-id>/          # Validated Python environment; previous runtime retained
      packages/              # Translation models
      model-transactions/    # Model-update staging and retained previous versions
      data/, cache/, config/ # Isolated runtime data
      installed-version      # Legacy marker; new markers live in the selected runtime
      cuda-installed         # Legacy marker; new markers live in the selected runtime
    webview/                 # Embedded browser profile
```

Setup needs internet access to GitHub, PyPI, package hosts, and model hosts. Allow several GB of disk space and enough RAM for your selected models. Each instance has a private environment and models. Translation works offline after required models are installed. Shared Python and package caches remain when an instance is removed. Logs retain up to 600 recent lines per instance for the current session.

## CUDA

CUDA is optional per instance. The app detects NVIDIA GPUs and drivers with a bounded `nvidia-smi` probe. The CUDA choice is enabled only for detected, supported NVIDIA hardware on Windows x64 or Linux. macOS and unsupported hardware use CPU. The default for new and older instance configurations is CPU.

Choose CUDA, wait for the configuration to save, then **Install runtime** (or **Repair runtime**) to provision private CUDA 12.8 libraries. The app pins CTranslate2 4.9.0, `nvidia-cublas-cu12==12.8.4.1`, and `nvidia-cuda-runtime-cu12==12.8.90`. It configures DLL/library search paths and sets `ARGOS_DEVICE_TYPE=cuda` for that process. CUDA requires a compatible NVIDIA display driver already installed; the app does not install or modify system drivers. Windows CUDA libraries add about 570 MB to the download. GPU detection alone is not a readiness guarantee: a CTranslate2 probe checks CUDA before server startup, and failures are shown in diagnostic output. Select CPU to recover without CUDA.

## Icons and localization

The repaired generated icon is supplied from `assets/icon.png` for the app window and embedded as a Windows executable resource. PNG, multi-resolution ICO, and ICNS variants are in `assets/`. The tile interior is opaque; transparency is limited to the outer silhouette. `build.rs` embeds the Windows resource, and `Dioxus.toml` supplies bundle icons.

UI text lives in the 20 JSON catalogs under `locales/`. Locale detection falls back to English. Catalog parity and UI-key tests check for missing text and mismatched placeholders. [The localization notes](../locales/README.md) document wording choices and the audit's limits. Technical upstream logs and operating-system errors remain in their original language under localized diagnostic headings. `assets/languages.json` is an embedded snapshot of the official Argos package index's languages; it is separate from UI locales. Country flags use SVG files from `lipis/flag-icons` in `assets/flags/`; other interface icons, including tab icons, live in `assets/icons/`. Dioxus asset references replace inline SVG and base64 data URLs. Dioxus Components, flag-icons, and Unicode licensing are recorded in `THIRD_PARTY_NOTICES.md` and `licenses/`.

## Verification

On Windows, official Dioxus 0.7.10 can log a background listener panic with Winsock error 10093 during normal shutdown. The app uses the unmodified upstream release; the earlier local listener patch is no longer applied.

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python tests/model_sync.py
```

The optional end-to-end test exercises downloading, installation, English/Spanish models, API readiness, translation, and process cleanup. It needs internet, time, disk space, and RAM:

```sh
cargo test --locked real_runtime_translation -- --ignored --nocapture
```

To exercise CUDA in that real download/translation/shutdown/restart test, set `LIBRETRANSLATE_GUI_TEST_CUDA=1` before running it. Shared test downloads stay in `.runtime-test/`, with an isolated temporary instance. Unit tests do not require a GPU or download models. Windows tests additionally verify that a launcher and its descendants are terminated together.

For Windows UI checks, launch with an isolated preferences file and `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`, then attach a browser checker to the local endpoint. This debugging endpoint is not enabled in normal launches.

## Source layout

- `src/app.rs`: UI, forms, controls, and logs.
- `src/advanced.rs`: server options and API-key controls.
- `src/model_ui.rs`, `src/models.rs`: model details, official catalog, and download-size lookup.
- `src/downloads.rs`: structured transfer progress shared with the interface.
- `src/tasks.rs`: active downloads and dismissible session history.
- `src/statistics.rs`: Prometheus parsing, polling, and the Statistics tab.
- `src/translator.rs`: local translation API client, session drafts, and the Translator tab.
- `src/translator_files.rs`: supported formats, multipart uploads, and translated-file downloads.
- `src/app_updates.rs`, `src/self_update.rs`: app-release checks, the update dialog, verified staging, replacement, and rollback.
- `src/releases.rs`: per-instance release selection support and the cached PyPI update check.
- `assets/update-models.py`: validated model replacement and transaction journals.
- `src/components/`: controls adapted from the official Dioxus registry and primitives.
- `src/state.rs`: application state and UI/backend coordination.
- `src/settings.rs`: portable preferences and validated storage locations.
- `src/i18n.rs`, `locales/`: localization.
- `src/gpu.rs`: bounded NVIDIA hardware detection.
- `src/network.rs`: port availability and free-port selection.
- `src/logs.rs`: semantic log colors.
- `src/manager.rs`: background manager, persistence, process monitoring, HTTP readiness.
- `src/runtime.rs`: verified bootstrap, environment isolation, installation, process cleanup.
- `src/window_chrome.rs`: Windows compositor border treatment.
- `src/model.rs`: validation and persistence.

Upstream: [LibreTranslate installation](https://docs.libretranslate.com/guides/installation/), [LibreTranslate releases](https://github.com/LibreTranslate/LibreTranslate/releases), [uv installation](https://docs.astral.sh/uv/getting-started/installation/), [managed Python](https://docs.astral.sh/uv/concepts/python-versions/).
