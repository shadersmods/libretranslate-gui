# LibreTranslate GUI

<!-- impeccable:product-schema 1 -->

## Platform

web

Desktop application on Windows, macOS, and Linux, using Dioxus and the platform WebView to render its interface.

## Stack

Rust with Dioxus for the desktop UI. LibreTranslate is an upstream Python application; a private Python runtime is provisioned automatically without requiring a separate user installation.

## Users

People who want to manage local LibreTranslate servers without installing Docker or other development tools.

## Product Purpose

Create, configure, install, start, stop, remove, and inspect local LibreTranslate instances through a desktop interface.

Also connect to LibreTranslate.com or a self-hosted server on another machine. All three instance types share the translator and can be removed. Cloud and remote instances discover languages and frontend capabilities from their server and expose no local process or model-management controls. LibreTranslate.com requires an API key before text or file translation; without one, show setup guidance and disable translation. Self-hosted remote connections accept an optional API key according to server requirements. LibreTranslate.com instances are added manually and require an API key when created or saved. First launch and legacy migration never add hosted instances automatically; existing saved entries are preserved. Remote translation clearly identifies where text and files are sent.

## Capabilities and Constraints

Cross-platform desktop application. Instance status and logs must reflect actual processes. Internet access is needed for initial installation and model downloads. Local-only binding is the initial implementation assumption. Instances stop when the manager closes. Models and packages remain available for subsequent launches.

Cloud and remote availability is checked automatically while the app is open. Each connection has a configurable check interval, request timeout, and consecutive-failure threshold before it is marked Offline. A successful check restores Online, and users can check a connection immediately. Polling stops when the app closes.

Twenty persisted interface languages: English, Romanian, Spanish, French, German, Brazilian Portuguese, European Portuguese, Russian, Simplified Chinese, Japanese, Korean, Arabic, Hindi, Hungarian, Italian, Polish, Dutch, Czech, Ukrainian, and Greek. Arabic content uses right-to-left layout; technical values retain left-to-right direction. Options include instance data, download-cache, and managed-Python folders. Default storage is a data folder beside the executable. Path changes apply after restart and do not move existing files automatically. CUDA is an optional per-instance device on supported NVIDIA GPUs, detected before offering the option; CPU remains the default. An application icon is generated and packaged for Windows, macOS, and Linux.

Download activity belongs in a title-bar Tasks menu with byte progress and dismissible session history. Models retains inventory and update controls. A Statistics tab appears when Prometheus metrics are enabled and reads live instance totals without displaying client or API-key labels. Translator is the first and default instance tab; each tab has a related local SVG icon. Translator offers automatic text translation after a typing pause, source/target selection, detection, and session drafts per instance. File mode uses server-supported formats and native file/save dialogs. Translation controls are disabled while offline or uninstalled. Translation requests and responses are not exposed as developer panels. Translation-language configuration includes Add all without duplicate entries.

On startup, check the project's published stable GitHub releases. Show an optional update dialog with changelog and version information. Accepted updates download and verify a platform-specific portable package, stop servers as the app closes, replace program files through a helper, and reopen the app. Instance data and preferences stay in place; failed replacements roll back. The application folder must be writable. About provides a manual check. Release publishing remains an explicit repository-owner action through the tagged-release workflow.

## Product Principles

- No manual runtime setup for end users.
- Show actual readiness, installation progress, and failures.
- Keep configuration and language data separate per instance.
- Make destructive actions explicit and recoverable before confirmation.
