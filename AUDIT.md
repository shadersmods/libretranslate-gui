# App audit — 15 November 2026

Reviewed the 0.2.7 app and applied the fixes below to the working tree. This is an unpublished audit build; the existing distribution and its instances were not replaced.

## Result

The first pass found **13 issues: 6 P1 and 7 P2**. All 13 are now fixed, including the two previously open deletion and updater-storage findings. Follow-up testing found and fixed the additional failures below. The latest verification also closed shrinking-upload and Windows storage-alias gaps. Disk-backed results remove the GUI's 50 MiB file limit, and Dioxus's listener has explicit shutdown. No P0 issue was found in the exercised paths.

The review covered instance configuration and lifecycle, autosave, local translation requests, native file dialogs, model synchronization, downloads, metrics, logs, localization, window controls, and the update helper. Windows checks used isolated data, local HTTP fixtures, and a real LibreTranslate 1.9.6 runtime with copied English/Romanian models. Source inspection and regression tests covered failure paths that are difficult to produce through the interface.

The implementation preserves the existing single rail, compact title bar, local SVG assets, official Dioxus controls, and light/dark themes. The Impeccable finish review returned **ship at the UI fix-list scope**; it did not certify the whole application. DESIGN.md and its sidecar were preserved.

## Fixed issues

| Priority | Issue and trigger | Change and verification |
|---|---|---|
| P1 | Switching tabs during a text request discarded its completion and submitted the unchanged text again. | Per-instance request state now outlives the panel. The reproduced navigation case went from **2 requests to 1**. Revision and process checks discard stale replies, and failed drafts can retry after a server restart. See [translator.rs](D:/minecraft/libretranslate-gui/src/translator.rs). |
| P1 | File selection and a completed translation disappeared when leaving Files or switching instances. A pending operation could also lose its result. | Root-owned file state and completion tasks preserve the selection/result across modes, workspace tabs, and instances. Verified through the app with a native Windows picker and a local server. See [translator_files.rs](D:/minecraft/libretranslate-gui/src/translator_files.rs). |
| P1 | Rendering a downloaded file cloned its entire buffer, up to 50 MiB per render. | Results now share an anonymous temporary file, with no payload copy when views clone state. Tests verify shared ownership, repeated/concurrent saves, and release after the last owner drops. |
| P1 | The bounded manager event queue silently dropped control notices when the renderer did not drain it. Losing a save acknowledgement could leave autosave waiting indefinitely. | Control events now use a reliable channel; unchanged periodic snapshots are coalesced. A test queues 40 saves without reading events and receives all 40 acknowledgements. See [manager.rs](D:/minecraft/libretranslate-gui/src/manager.rs). |
| P1 | Saving a translated file directly truncated an existing destination before the write succeeded. A disk/write failure could destroy its previous content. | Write and sync a temporary file beside the destination, then rename it into place. An injected partial-write failure preserves the original and removes the temporary file. |
| P1 | The updater moved the current executable into backup before installing its replacement, leaving a period with no executable at its normal path. | Copy the backup first and replace through rename; rollback also avoids deleting the destination before restoring it. Existing replacement/rollback tests pass. This narrows the interruption risk; a power-loss recovery journal remains future work. See [self_update.rs](D:/minecraft/libretranslate-gui/src/self_update.rs). |
| P2 | The UI wrote its global state every 150 ms while idle and copied the downloads map on every tick. | Collect updates before taking a reactive write; copy downloads only when changed. Idle ticks no longer dirty the UI. Tests also cover autosave wakeups and expiry of Saved feedback. Window maximize state has its own change check. See [state.rs](D:/minecraft/libretranslate-gui/src/state.rs). |
| P2 | Follow new log entries changed only in memory and reset after restarting. | Persist that preference immediately while preserving unfinished folder edits. Verified on disk and in a regression test. |
| P2 | Text/Files tabs had no corresponding tab panels. Workspace and mode panels also lacked explicit names tied to their tabs. | Use official TabContent components with named associations. The built app exposes matching control/panel IDs; settled ArrowRight focus reaches Files. |
| P2 | Choose a file remained enabled while its native dialog was open, allowing multiple pickers. Save also needed a guard before awaiting its dialog. | Per-instance choosing/saving guards cover the full dialog lifetime, including navigation. |
| P2 | Supported files with uppercase extensions, such as REPORT.TXT, were rejected by the client-side format check. | Match extensions without case sensitivity. Tests cover TXT, HTML, and an unsupported ZIP. |
| P2 | Deleting an instance removed its files before saving configuration. A failed configuration write left a listed instance without its runtime/models. | Stage the directory with a durable journal, restore it if configuration cannot be saved, then clean committed removals in the background. Startup recovers interrupted removals and skips active leases. Injected save failure preserves the files; cleanup/reopen stress covers 80 stages, and the Manager reopen regression passed 12/12 repetitions. See [removals.rs](D:/minecraft/libretranslate-gui/src/removals.rs). |
| P2 | Update attempts retained archives, extracted bundles, and helpers indefinitely. | Owned preparation guards reclaim failures; startup maintenance skips active work, reclaims completed stages, and keeps the latest rollback backup. Unknown folders and failed stages with backups are preserved. Locked payload/directory tests verify that cleanup keeps durable retry metadata. See [update_storage.rs](D:/minecraft/libretranslate-gui/src/update_storage.rs). |

## Follow-up fixes

Three agents tested file/API failures, removal recovery, and updater storage independently. Their changes were checked together with the app's state and runtime fixes.

| Failure | Change and evidence |
|---|---|
| An unrelated background error discarded an unsaved instance draft. Save acknowledgements also lacked an instance ID. | Separate instance-specific save failures from general errors and identify acknowledgements by instance. Failed drafts stay available across navigation; automatic retry uses a five-second backoff. An injected configuration-write failure in the built app preserved the draft and saved it after the fault was removed. |
| A file that grew after selection could upload bytes beyond its validated size. | Open once, validate the handle, and limit multipart reads to that size. The regression appends bytes after form construction and verifies that they are excluded. Empty and non-file inputs are rejected before a request; large files stream without a GUI size cap. |
| Empty or partial download responses could be accepted as completed translations. | Require a nonempty HTTP 200 response and a complete body. Fixtures exercise 204/206, truncation, 65 MiB downloads with and without Content-Length, malformed upload replies, and binary output. Input files remain intact. |
| File download authentication failures and body-read timeouts lost their useful error category. | Preserve authorization and timeout classifications through headers and body reads. Delayed JSON and download-body fixtures verify both paths. |
| Inherited Python settings could redirect the private runtime or disable its installation assertions. | Clear foreign PYTHONHOME/PYTHONPATH and related overrides, including PYTHONOPTIMIZE, before launching. A command-environment regression checks isolation without changing the test process's environment. |
| Windows path case/separator aliases could reject valid updater storage or bypass reserved shared-path checks. | Compare validated path components without lossy string conversion. Reserve both instances and instance-removals; resolve existing ancestors when validating configurable shared paths. Tests cover casing, separators, parent traversal, and device prefixes. |
| The GUI imposed a 50 MiB file cap to bound in-memory results. | Upload from the selected handle; download through a 64 KiB buffer to an anonymous file under the configured cache's translations directory. Share the file across navigation and atomically copy it to the chosen destination on save. The OS reclaims temporary storage when the last handle closes. Disk failures have a distinct error translated in all 20 locales. |
| Uppercase extensions passed the GUI's format check but failed LibreTranslate's case-sensitive check. | Normalize only the multipart filename extension. The original file and name stay unchanged. A multipart regression and the real LibreTranslate test use uppercase input names. |
| A file truncated after upload preparation still produced a completed multipart body shorter than its declared length. | The bounded upload reader now reports UnexpectedEof if the validated bytes disappear. The regression failed before this change and now passes; files that grow remain capped at their validated size. |
| Trailing-dot aliases and Unicode case variants bypassed reserved shared-path checks before storage folders existed. | Use [Windows' native lexical normalization](https://doc.rust-lang.org/std/path/fn.absolute.html) and [ordinal UTF-16 case comparison](https://learn.microsoft.com/en-us/windows/win32/api/stringapiset/nf-stringapiset-comparestringordinal). Both reproduced bypasses now fail validation for shared cache and Python paths. Regressions preserve literal intermediate spaces and verbatim suffixes. Removal and update storage use the same component comparison without conflating device prefixes or parent traversal. |
| Canonical extended prefixes on existing long paths could hide a reserved ancestor from storage validation. | Convert drive/UNC prefixes only in comparison identities. A regression creates an existing path longer than 260 characters under instances and verifies rejection; literal suffixes on verbatim paths remain distinct. |

Testing the new cleanup code also exposed Windows recovery races. Removal recovery and final journal deletion now use a short shared lock; bulk file deletion stays outside it. Update cleanup records its discard decision before removing payloads and deletes ownership metadata last, so a locked file can be retried. The helper holds its stage lease until process exit to protect its still-mapped executable from the restarted app.

Follow-up completion checks passed **102 app tests**, including **19 file/API tests, 7 removal tests, 22 update/storage tests, and 6 storage-path tests**. The default run still explicitly ignores the same six opt-in/platform tests; the prepared real-runtime test passed separately on the final test binary. All 11 model-synchronization tests, formatting, and clippy with warnings denied also passed. [App tests](D:/minecraft/libretranslate-gui/.runtime-test/follow-up-final-tests.log), [clippy](D:/minecraft/libretranslate-gui/.runtime-test/follow-up-final-clippy.log), [real runtime](D:/minecraft/libretranslate-gui/.runtime-test/follow-up-final-real-runtime.log), [model synchronization](D:/minecraft/libretranslate-gui/.runtime-test/follow-up-model-sync.log). The final packaged Windows release built successfully with all 85 assets in a separate output folder because the usual output executable was already running. [Build log](D:/minecraft/libretranslate-gui/.runtime-test/follow-up-final-build.log), [build folder](D:/minecraft/libretranslate-gui/.runtime-test/follow-up-target/dx/libretranslate-gui/release/windows/app). Historical UI and helper evidence below was not rerun for these backend changes.

## Performance evidence

- **Unchanged draft, navigation during an active request:** 2 HTTP translation requests before; 1 after. The local server delays the request so this exercises in-flight work.
- **File results:** the GUI no longer keeps the complete payload in RAM. Downloads use a 64 KiB transfer buffer and a temporary file; view clones share its handle. A generated 129 MiB stream verifies bounded read buffers. Real 65 MiB HTTP responses are saved and checked without building a full payload buffer in the test.
- **Idle state handling:** previously an unconditional reactive write every 150 ms, about 6.7 per second. The new collector returns no update when nothing changed, verified by a regression test. Model and log events still update the interface.
- The local HTTP client is reused across text, language, format, and file requests, allowing connection pooling.
- **Real CPU runtime:** six concurrent English-to-Romanian replies matched the same six inputs translated serially by a fresh server process. The isolated run also translated Romanian-to-English and a text file and reported nine translation requests through Prometheus. This checks correctness under concurrency; it is not a sustained-load benchmark.

Short process CPU/memory samples were collected, but the before/after runs differed in window visibility and visited screens. They are not comparable enough to claim an overall CPU or memory percentage improvement.

## Windows framework shutdown fix

Normal closure of the previous build logged a background-thread panic in dioxus-desktop 0.7.10's edit-socket listener: Winsock error 10093 while binding a replacement socket. It occurred during shutdown, including after helper-launched sessions. Both replacement and rollback had completed, the tracked hashes were preserved, and the targeted processes exited. Before-fix evidence: [normal close](D:/minecraft/libretranslate-gui/.runtime-test/v029-final-live.err), [post-update close](D:/minecraft/libretranslate-gui/.runtime-test/v029-final-helper-success.err).

The dependency's detached accept loop retried accept failures by binding again. A local patch now owns, stops, and joins that listener on LoopDestroyed, before process teardown. A loopback wakeup interrupts accept without idle polling. Initial HTTP/authentication has a one-second timeout because Windows does not cancel an already pending read through a duplicate socket; the timeout is cleared before normal edit traffic. Reconnect failures use cancellation-aware backoff. Genuine listener panics still propagate.

The vendored 0.7.10 source retains its licenses and provenance. Nine listener/protocol tests pass, including authenticated edit/ack behavior beyond the handshake deadline. [Patch scope and removal criteria](D:/minecraft/libretranslate-gui/vendor/dioxus-desktop/PATCHES.md).

The packaged Windows build passed **four consecutive open/close cycles**, each with exit code 0 and empty stderr. Local update replacement and forced rollback also restarted and closed without the previous panic. [Close-cycle results](D:/minecraft/libretranslate-gui/.runtime-test/ui-v030/shutdown-results.json), [helper restart results](D:/minecraft/libretranslate-gui/.runtime-test/v030-helper-results.json).

## Validation

| Area | Evidence |
|---|---|
| Rust checks | cargo fmt and clippy with warnings denied passed. **97 app tests passed; 6 were explicitly ignored in the default run.** These are four existing opt-in runtime tests, the prepared real-runtime test run separately, and a Windows symlink test requiring unavailable privileges. The final Dioxus desktop release build passed. [Tests](D:/minecraft/libretranslate-gui/.runtime-test/v030-final-tests.log), [Clippy](D:/minecraft/libretranslate-gui/.runtime-test/v030-final-clippy.log), [build](D:/minecraft/libretranslate-gui/.runtime-test/v030-final-build.log). |
| Model synchronization | **11 tests passed**, covering selection, pruning, downloads, archive validation, and recovery. Tests use local fixtures, not new model downloads. |
| Text lifecycle | Navigation during an active request, revision handling, authentication/error parsing, restart retry, and Unicode. |
| IME | Actual composition events produced **0 requests during composition and 1 after composition ended**. Playwright fill cancels composition and was excluded from this result. |
| Files | 18 file/API tests pass. They cover streamed 65 MiB uploads, 65 MiB HTTP downloads with Content-Length and chunked encoding, bounded reads for a generated 129 MiB stream, disk errors, cleanup, concurrent/repeated saves, and uppercase extensions. Earlier native Windows pick/save and navigation checks remain applicable to the unchanged dialogs. [Tests](D:/minecraft/libretranslate-gui/.runtime-test/v030-files-final.log). |
| Real runtime | The opt-in test was rerun with disk-backed results and an uppercase input filename against LibreTranslate 1.9.6. It uses the production launch script, copied en/ro models, a URL prefix, API authentication, Waitress, file translation, and Prometheus. Six concurrent replies matched a fresh process's serial replies. No model downloads were allowed. [Run](D:/minecraft/libretranslate-gui/.runtime-test/v030-real-runtime.log), [result](D:/minecraft/libretranslate-gui/.runtime-test/real-audit-v029/result.json). |
| Autosave failure | In the isolated app, a blocked instances.json.tmp write left the edited name and Unsaved changes state intact across instance navigation. Removing the fault allowed automatic retry to persist the draft. |
| Removal recovery | Configuration-write failure, interrupted journal recovery, active leases, conflicts, locked files, and 80-stage cleanup/reopen stress. Seven removal tests passed; the exact Manager reopen regression passed 12/12 repetitions. |
| Update helper | The final patched build applied a local update, restarted, then rolled back a deliberately incomplete bundle and restarted again. Tracked settings, instance configuration, beside-executable data, and an unknown asset retained their hashes. Unknown stage data was preserved. All test apps were closed afterward, with no shutdown/cleanup panic. [Replacement](D:/minecraft/libretranslate-gui/.runtime-test/live-update-v029/v030-success-result.json), [rollback](D:/minecraft/libretranslate-gui/.runtime-test/live-update-v029/v030-failure-result.json), [restart results](D:/minecraft/libretranslate-gui/.runtime-test/v030-helper-results.json). No public release was downloaded. |
| Update storage | 22 updater/storage tests cover retention, live parents, path aliases, locked payload/directory retries, replacement, and rollback. A failed cleanup cannot promote a partially deleted backup to the latest retained rollback copy. |
| Framework listener | Nine vendor tests pass, including idle handshakes, blocking accept, reconnect, simultaneous shutdown, last-owner drop, authenticated edits/acks, and propagation of genuine panics. Four packaged app close cycles and helper-launched sessions exit without the former Winsock panic. [Tests](D:/minecraft/libretranslate-gui/.runtime-test/v029-dioxus-listener/vendor-tests.log). |
| Locale copy | All 20 catalogs have 331 nonempty keys with placeholder parity. The obsolete size-cap text was removed and the storage error translated. Review was model-assisted, without a native-speaker certification claim. [Validation](D:/minecraft/libretranslate-gui/.runtime-test/v029-locale-file-storage/report.json). |
| UI | All six enabled instance tabs; 1160×820 and the supported minimum 820×600; both themes; collapsed/expanded rail; German and Arabic Options. No page errors or horizontal page/main overflow in those checks. Associated form labels were included when checking accessible names. |
| Existing data | Tests used separate settings, ports, data, and copied models. The real-runtime test used the installed Python packages as read-only code with bytecode writes disabled. The user's three configured instances were not started, edited, removed, or migrated by this audit. |

### UI health assessment

| Dimension | Score | Basis and limit |
|---|---|---|
| Accessibility | 3/4 | Tab associations, labels, keyboard focus, and composition checked; no full Narrator or WCAG certification pass. |
| Performance | 3/4 | Duplicate work and large render copies removed; real model inference under sustained load was not profiled. |
| Responsive layout | 3/4 | Supported desktop sizes checked; native DPI/text-scaling combinations remain untested. |
| Theming | 3/4 | Shared light/dark tokens and localized screens checked; no exhaustive per-control contrast matrix. |
| Implementation integrity | 4/4 | Product-specific shell and established components preserved; scoped finish review found no material regression. |
| **Total** | **16/20 — Good** | Desktop WebView assessment, not a claim of complete platform certification. |

## Limits and next checks

Four existing opt-in runtime tests were not rerun. The new real-runtime test was invoked separately against an explicitly prepared isolated fixture. Native macOS/Linux behavior, a fresh-install platform matrix, CUDA inference, DPI/text-scaling combinations, and public release downloads remain untested in this pass. The Windows symlink test is explicitly ignored because the host lacks symlink privileges; the Unix version remains enabled but was not run here.

Update storage deliberately retains unknown legacy folders and failed stages containing rollback backups. Their ownership/recovery needs cannot safely be inferred. Replacement and rollback were exercised with a local helper and real process restart, but power-loss recovery between every filesystem operation is not guaranteed.

### Large files

The GUI's former **50 MiB upload and download limit is removed**. Results stream to disk in the configured cache folder and remain available across navigation during the session. Saving copies the temporary result atomically to the chosen destination; it does not overwrite the input file unless the user chooses that destination.

Upstream restrictions still apply. LibreTranslate 1.9.6 checks supported formats; its default Waitress server limits the whole request body to **1 GiB**, including multipart overhead. File conversion and the upstream download endpoint can use substantial memory. The GUI's streaming change does not guarantee arbitrary files fit the server's resources or request timeout. [LibreTranslate source](https://github.com/LibreTranslate/LibreTranslate/blob/v1.9.6/libretranslate/app.py), [Waitress settings](https://docs.pylonsproject.org/projects/waitress/en/stable/arguments.html#max-request-body-size).

The next performance work should measure sustained request latency and throughput with selected model pairs and CPU/CUDA devices, including CTranslate2's inference thread settings alongside Waitress workers. Large retained file results now consume cache disk space rather than a full in-process payload buffer; users still need enough free storage for the result and the temporary copy used when saving.

The two findings left open by the first pass are closed. The checks above establish behavior for the exercised paths, not complete platform or security certification. The 0.2.7 working build remains unpublished.
