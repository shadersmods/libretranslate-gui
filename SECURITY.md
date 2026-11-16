# Security Policy

## Report a vulnerability

Use [GitHub's private vulnerability report](https://github.com/shadersmods/libretranslate-gui/security/advisories/new). If private reporting is unavailable, email [neo@shadersmods.com](mailto:neo@shadersmods.com) with the subject `LibreTranslate GUI security report`.

Please keep vulnerability details out of public issues and pull requests until a fix and disclosure have been discussed. Include:

- The affected app version or commit, operating system, and package type.
- Steps to reproduce using harmless sample data, or a small proof of concept.
- The expected behavior, observed behavior, and likely impact.
- Any required access, configuration, or user interaction.
- Relevant logs with credentials, private text, and identifying paths removed.

Test only on systems and data you own or have permission to test. Avoid sending real credentials or private documents. The maintainer will use the private report to discuss reproduction, fixes, and disclosure; this project does not promise a response deadline or a paid bounty.

## Versions

Reports are welcome for published releases and the current development branch. Include the exact version even if you cannot reproduce the issue on a newer build. Fixes may require upgrading to a new release; continued fixes for older releases are not guaranteed.

## System and scope

LibreTranslate GUI is a desktop manager for local LibreTranslate servers on Windows, macOS, and Linux. This policy covers the Rust application, embedded UI, runtime and model management, self-updater, installers, and release workflows in this repository.

The app launches Python processes, downloads executable runtime components and models, reads local files selected for translation, and writes settings and instance data. Servers bind to `127.0.0.1` by default; users can enable LAN access and configure API keys or protected metrics. A local server still receives untrusted request data. Network access must be assessed against the actual configuration.

Relevant code includes `src/runtime.rs`, `src/models.rs`, `src/self_update.rs`, `src/update_storage.rs`, `src/translator_files.rs`, `src/network.rs`, `src/installation.rs`, and `.github/`.

## Trust boundaries and security requirements

These are requirements for assessing changes, rather than a claim that every possible attack has been ruled out:

- Downloaded archives, release metadata, model catalogs, server replies, and translated files cross a trust boundary. Validate them before use and verify executable downloads against the expected release checksums.
- Archive extraction, updates, cleanup, and instance removal must stay inside their intended directories. Traversal paths, symlinks, and path aliases must not redirect writes or deletion to unrelated files.
- Updates must preserve user settings and instance data. An interrupted or stale update must not silently undo a later successful update.
- API keys, metrics tokens, and private translation content must not leak through URLs, process arguments, logs, diagnostics, or release packages.
- Server requests must honor the selected instance, authentication settings, and configured endpoint. Enabling LAN access must not silently remove configured access controls.
- Runtime processes must remain under the app's lifecycle controls. Closing or removing an instance must not leave an owned server running unexpectedly.

When reporting a finding, explain how an attacker reaches the affected operation and what access they gain. Examples include executing code through a crafted download or server response, accessing files outside an instance, bypassing configured authentication, disclosing secrets, or corrupting data through an update or cleanup operation. Identify any prerequisite local access instead of assuming that all local attacks have the same impact.

## Dependencies and limitations

LibreTranslate and its Python dependencies provide the translation server. Dioxus and the operating system's WebView provide the desktop UI. Report vulnerabilities in those components to their upstream maintainers as appropriate, and report any unsafe use by this app here. A dependency alert alone is not enough to establish impact; include the affected version and reachable behavior when known.

The GUI does not sandbox Python runtime code or replace the server's authentication controls. Release packages are currently unsigned. Published checksums detect a mismatch with release metadata; they do not establish publisher identity if the release account or distribution channel is compromised. The unsigned MSIX is a testing package, as described in the [user guide](docs/usage.md#unsigned-msix).

These limitations are context for assessing a report, not blanket exclusions for findings involving dependencies, downloads, or local access.
