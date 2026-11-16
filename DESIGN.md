---
name: LibreTranslate GUI
description: Compact desktop controls for local LibreTranslate servers.
colors:
  accent: "#087d44"
  accent-hover: "#066536"
  accent-ink: "#fff"
  accent-soft: "#d3eadc"
  bg: "#f1f3f5"
  panel: "#ffffff"
  rail: "#dde3e7"
  surface: "#e9edf0"
  surface-hover: "#dce4e8"
  input: "#fff"
  text: "#17232b"
  muted: "#52636e"
  border: "#cdd6dc"
  danger: "#af343f"
  warning-bg: "#fff0d5"
  warning: "#795017"
  error-bg: "#f6dfe1"
  dark-accent: "#1bd66f"
  dark-accent-hover: "#47e58e"
  dark-accent-ink: "#092317"
  dark-accent-soft: "#204b37"
  dark-bg: "#16181c"
  dark-panel: "#22252b"
  dark-rail: "#25292f"
  dark-surface: "#2c3037"
  dark-surface-hover: "#373e46"
  dark-input: "#2d3239"
  dark-text: "#edf1f5"
  dark-muted: "#a8b3bf"
  dark-border: "#383e47"
  dark-danger: "#f38991"
  dark-warning-bg: "#3e3321"
  dark-warning: "#efce8a"
  dark-error-bg: "#43272e"
  danger-action: "#b83d4c"
  danger-action-hover: "#a3303e"
  log-error: "#ac2637"
  log-warning: "#835000"
  log-success: "#087d44"
  log-progress: "#155da4"
  log-build: "#6b39a3"
  log-detail: "#596a75"
  dark-log-error: "#ff939e"
  dark-log-warning: "#edc476"
  dark-log-success: "#72dba3"
  dark-log-progress: "#8abfff"
  dark-log-build: "#c4abff"
  dark-log-detail: "#a3aebb"
  tooltip-bg: "#ffffff"
  tooltip-border: "#bbc6cf"
  dark-tooltip-bg: "#373e46"
  dark-tooltip-border: "#535e69"
typography:
  page-title:
    fontSize: "30px"
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "-.035em"
  instance-title:
    fontSize: "23px"
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "-.025em"
  section-title:
    fontSize: "18px"
    fontWeight: 650
    lineHeight: 1.4
    letterSpacing: "-.02em"
  card-title:
    fontSize: "16px"
    fontWeight: 650
    lineHeight: 1.4
    letterSpacing: "-.02em"
  body:
    fontFamily: '"Segoe UI", -apple-system, BlinkMacSystemFont, "Noto Sans", sans-serif'
    fontSize: "13px"
    lineHeight: 1.65
  statistic:
    fontSize: "25px"
    fontWeight: 650
    lineHeight: 1.2
  translation:
    fontSize: "14px"
    lineHeight: 1.7
  label:
    fontSize: "12px"
    fontWeight: 600
  mono:
    fontFamily: 'Consolas, "SFMono-Regular", "Liberation Mono", monospace'
    fontSize: "12px"
rounded:
  tooltip: "7px"
  progress: "3px"
  chip: "5px"
  option: "6px"
  field: "8px"
  control: "9px"
  popup: "10px"
  toast: "11px"
  rail-control: "12px"
  panel: "14px"
  card: "15px"
  workspace: "22px"
spacing:
  compact: "4px"
  small: "8px"
  medium: "12px"
  group: "18px"
  card: "20px"
  panel: "22px"
  workspace: "24px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.accent-ink}"
    rounded: "{rounded.control}"
    padding: "10px 15px"
  button-primary-hover:
    backgroundColor: "{colors.accent-hover}"
  button-danger:
    backgroundColor: "{colors.danger-action}"
    textColor: "#fff"
    rounded: "{rounded.control}"
    padding: "10px 15px"
  button-danger-hover:
    backgroundColor: "{colors.danger-action-hover}"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "10px 15px"
  button-text:
    textColor: "{colors.accent}"
    rounded: "{rounded.control}"
    padding: "6px 8px"
  input:
    backgroundColor: "{colors.input}"
    textColor: "{colors.text}"
    rounded: "{rounded.field}"
    padding: "10px 12px"
  rail-active:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.accent}"
    rounded: "{rounded.rail-control}"
    width: "44px"
    height: "44px"
  language-chip:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.muted}"
    rounded: "{rounded.chip}"
    padding: "4px 6px"
  instance-card:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text}"
    rounded: "{rounded.card}"
    padding: "20px"
  tasks-popover:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text}"
    rounded: "{rounded.rail-control}"
    width: "390px"
  translator-pane:
    backgroundColor: "{colors.input}"
    textColor: "{colors.text}"
    rounded: "{rounded.popup}"
    typography: "{typography.translation}"
  translator-result:
    backgroundColor: "{colors.bg}"
  model-download-progress:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.progress}"
    height: "5px"
  rail-instance:
    backgroundColor: "transparent"
    rounded: "{rounded.control}"
    padding: "4px"
    width: "46px"
    height: "46px"
  tooltip:
    backgroundColor: "{colors.tooltip-bg}"
    textColor: "{colors.text}"
    rounded: "{rounded.tooltip}"
    padding: "7px 10px"
  instance-preview:
    backgroundColor: "{colors.tooltip-bg}"
    textColor: "{colors.text}"
    rounded: "{rounded.rail-control}"
    padding: "16px"
    width: "280px"
  model-table:
    textColor: "{colors.text}"
    width: "100%"
  log-output:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.text}"
    typography: "{typography.mono}"
    rounded: "{rounded.control}"
    padding: "18px"
  tab-active:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.accent}"
    padding: "11px 16px"
    typography: "{typography.label}"
---

# Design System: LibreTranslate GUI

## Overview

**Creative North Star: "Operate"**

Operate is a compact desktop utility for managing local LibreTranslate servers. Its user-approved Modrinth-inspired shell pairs a single expandable rail with a rounded workspace, neutral surfaces, and green actions. The interface gives actual readiness, configuration, and logs priority over decoration.

Dark charcoal and light neutral themes share geometry and interaction states; persisted System mode follows the operating system. The shared components support twenty interface locales: English (en), Romanian (ro), Spanish (es), French (fr), German (de), Brazilian Portuguese (pt-BR), European Portuguese (pt-PT), Russian (ru), Simplified Chinese (zh-Hans), Japanese (ja), Korean (ko), Arabic (ar), Hindi (hi), Hungarian (hu), Italian (it), Polish (pl), Dutch (nl), Czech (cs), Ukrainian (uk), and Greek (el). Labels must wrap cleanly and Arabic content mirrors right to left. The existing two-panel speech icon on a green tile with a lime connection remains the packaged app identity, with transparency only outside the tile.

**Key Characteristics:**
- Compact desktop shell with one expandable rail.
- Neutral tonal layering with green start/install actions, red stop controls, and green selection states.
- Real process status and session tasks, direct lifecycle controls, connected work tabs, and actual local translation/statistics.
- Shared Light, Dark, and System geometry across twenty locales, including mirrored Arabic content.

## Colors

Green is the sole action accent; neutral surfaces carry the workspace. The frontmatter records Light values under unprefixed names and Dark values under `dark-` names. CSS maps both themes to the same semantic custom properties.

### Primary

- **Action Green** (`accent` / `dark-accent`) identifies lifecycle actions, selected tabs, and running state. Hover uses the matching accent-hover token; button text uses accent-ink rather than a fixed white.
- **Selection Green** (`accent-soft` / `dark-accent-soft`) marks rail selection and active options without competing with primary actions.

### Neutral

- **Workspace**, **Panel**, and **Rail** establish the three main surface levels.
- **Surface** and **Surface Hover** provide secondary controls and interaction states; **Input** distinguishes editable fields.
- **Text**, **Muted**, and **Border** separate primary content, supporting details, and restrained divisions in both themes.
- **Tooltip Background** and **Tooltip Border** form a raised surface within the active theme. Both normal hints and rich previews use these semantic tokens with the theme Text color; System-dark follows the Dark values.

### Semantic States

Danger text and error backgrounds identify failures and destructive interactions. Warning foreground/background pairs explain unavailable hardware and progress. Stop server in both detail headers and cards uses the same red danger-action fill and white text as destructive confirmation; Start and Install retain Action Green. Danger-action-hover darkens the red on hover. Status always includes text alongside its dot. Log-specific error, warning, success, progress, build, and detail roles are paired for Light and Dark; they annotate actual output while plain lines retain the main text color.

**The State Before Decoration Rule.** Use accent and semantic colors to explain available actions, selected controls, and real process state; do not add decorative metrics or status filler.

## Typography

The native interface stack in the body token is used throughout; this utility has no decorative display face. Consolas with SFMono-Regular and Liberation Mono fallbacks distinguishes endpoints, paths, and logs. Page titles are compact and tightly tracked; labels remain sentence case.

### Hierarchy

- **Page title:** library, creation, and Options headings; below the 1050px breakpoint its size becomes (27px).
- **Instance title:** the detail header uses the smaller instance-title token so state and actions remain nearby; at widths up to (1000px) it becomes (20px).
- **Section and card titles:** distinguish setting groups from server cards without introducing another font.
- **Body and label:** supporting prose uses the body token, with a maximum width of (74ch); form labels and controls use the compact label role.
- **Supporting detail:** hints and card metadata use (11px); endpoint text is monospace at (11px), while logs use the mono token with (1.8) line height.

## Layout

The shell fills the viewport with a custom draggable title bar (36px) above one rail and the workspace. The rail is (68px) wide when collapsed and (254px) when expanded; its labels reveal instance names, statuses, and ports. At widths up to (720px), the collapsed rail is (60px). Maximize removes the outer window border and corner rounding; minimize, maximize/restore, close, and edge resizing remain native window operations. On Windows, the implementation disables the undecorated native shadow and reapplies the border-none setting after window/WebView creation and focus/resize events; the packaged v0.2.3 window was checked on the current Windows desktop in dark mode, where its rounded edge showed no bright white native rim.

The workspace has a rounded upper-left corner and scrolls internally. Main content uses the workspace spacing token, reduced to (20px) at widths up to (1000px). Detail and creation surfaces center within a maximum width of (1100px); Options uses (850px). The library has at most three equal flexible columns with group-sized gaps. Its own inline-size container, rather than the window alone, switches to two columns at (920px) and one at (600px). The fallback for engines without container queries uses two columns at viewport widths up to (1100px). Card content may shrink within each track.

The first detail viewport groups the instance name and status, endpoint with copy, lifecycle actions, and repair/delete tools. At widths up to (1000px), the header remains on one row and lifecycle actions become icon-only squares (36px), retaining localized accessible labels and tooltips. Translator opens by default, followed by Configuration, Models, Advanced, and Logs in one connected panel that fills remaining height; Statistics appears between Models and Advanced only when instance metrics are enabled. The tab row stays on one line and scrolls horizontally rather than wrapping. Configuration uses a two-column grid with (20px) gaps, repeatable language rows, and GPU selection. Language/device groups span the grid below (1000px), and the whole form becomes one column below (720px). Existing instances save valid Configuration and Advanced edits automatically after a (600ms) debounce. An inline pending/saving/saved status replaces the former Save row; invalid drafts remain unsaved with visible validation. New-instance creation retains explicit Create and Cancel actions.

**The Single Shell Rule.** Keep branding in the title bar and navigation in one expandable rail; do not add a second workspace header or footer.

## Elevation & Depth

Resting surfaces use tone and borders. Floating selects, dialogs, and toasts use soft shadows; these define the persistent elevation vocabulary. Tooltips and rich instance previews use their same-theme raised surface and thin border without a shadow. Popups layer above the workspace and toasts above popups. Shadows, focus treatment, motion, and responsive breakpoints are recorded in the sidecar.

**The Flat Workspace Rule.** Separate resting workspace surfaces with tone and restrained borders. Reserve shadows for select popups, dialogs, and toasts.

## Shapes

Controls and fields have compact rounded corners, with softer panel and card silhouettes. The frontmatter records the reused radius vocabulary. The workspace rounds its upper-left corner in left-to-right layouts and mirrors that corner to the upper-right for Arabic; the outer window uses (12px) corners when restored. Fields, cards, and confirmation surfaces use thin borders or tonal separation rather than hard edges. Status dots, the activity spinner, and switch thumbs are circular. Language flags retain a small (3px) corner treatment.

## Components

### Buttons

Start and Install buttons use action green and theme-specific accent ink. Stop server uses the red danger-action fill, white text, and danger-action-hover treatment in both detail headers and library cards. Secondary controls use the surface fill with a thin border; text buttons use accent text and an accent-soft hover. Standard actions have a minimum height of (40px). Detail header actions are denser at (36px) with (8px 12px) padding, while card actions are (35px) with (8px 10px) padding. Header tool/delete controls and card delete controls use matching (36px) squares. Narrow-window lifecycle buttons use the same square size and hide only their visible labels. Disabled buttons use (.45) opacity and do not suggest interactivity. Destructive header icons reveal danger color on hover and open confirmation before removal.

Contextual button hints use official Dioxus tooltips: a compact raised surface within the active theme, a thin tooltip-border stroke, tooltip-bg fill, theme Text color, (7px) corners, and (7px 10px) padding. The hint palette follows Light, Dark, or System rather than inverting it. Normal hints sit above their anchor with an (8px) gap, fall below when there is insufficient space above, and clamp to (8px) viewport margins. A shared controlled state permits only one hint or instance preview at a time. Positioning follows layout, scrolling, and resizing continuously through requestAnimationFrame. Click, Escape, blur, document hiding, loss of hover/keyboard focus, or an anchor leaving the viewport dismisses the current hint. Direct button focus opens keyboard hints; wrappers do not add a second tab stop. Minimize, maximize/restore, and close retain accessible labels but deliberately have no tooltips.

Focus is a visible accent outline (3px) offset by (3px). Color changes use (120ms ease-out); rail expansion uses (180ms ease-out). Reduced-motion preferences disable animation and transitions.

Shared CSS motion tokens keep pointer feedback consistent. Action buttons press to (0.97) scale over (100ms) and release over (160ms); switch thumbs move over (120ms). Tasks opens from the top-right trigger over (180ms) and closes over (125ms). Centered dialogs fade their backdrop and scale the panel from (0.96) together over (250ms), with a (150ms) close. These transitions use cubic-bezier(0.23, 1, 0.32, 1). Toast entrances use the same (180ms) fade and (0.97) scale; dismissal stays immediate under the upstream toast lifecycle. Closed dialogs and Tasks are inert while their exit finishes. Keyboard input settles transitions immediately. Selects, tooltips, tabs, list rows, and translation results have no entrance motion or stagger.

### Inputs and Selects

Inputs and select triggers use an input surface, thin border, and field corners, with minimum height (42px). Use official Dioxus Components primitives for select filtering and keyboard navigation. The custom floating list is fixed, bounded to the viewport with (12px) margins, capped at (320px), and flips above the trigger when space is greater there. Searches appear only for lists longer than eight choices; shorter lists retain keyboard input without displaying an unnecessary search field. Popup search fields fit inside their container including margins. Support pointer toggling, outside dismissal, Escape, visible focus, and a localized no-match result.

### Validation and Automatic Saving

Existing-instance edits share automatic saving across Configuration and Advanced. Show pending, saving, and briefly saved feedback in muted (11px) status text; show invalid drafts in danger-colored (12px) text. Invalid drafts are preserved for correction and block lifecycle/model actions that require saved settings. New-instance creation and Options retain their explicit actions.

Place live port availability directly under the port field with an icon and text, using accent for available and danger for unavailable. Distinguish another configured instance, an externally occupied port, a checking result, and the current instance's own listener. Keep Find free port beside this status. Port values use the supported (1024–65535) range; the free-port action checks candidates rather than claiming a port has been reserved. The worker field shows its device-derived CPU maximum and accepts (1) through the available parallelism limit.

### Navigation and Tabs

The rail combines expandable labels with local SVG controls and compact instance avatars. Active rail controls use selection green; instance avatars display initials and a status dot. Collapsed instance targets are fixed (46px) squares with (4px) insets around (38px) avatars. Expanded instance targets use automatic height with a (52px) minimum, retaining space for names and status. The full-width scrolling instance list spans the rail padding, hides horizontal overflow, and reserves stable scrollbar gutters on both edges so targets remain centered when scrolling is needed. Connected tabs begin with the default Translator, followed by Configuration, Models, conditional Statistics, Advanced, and Logs. Every tab pairs its label with a related local SVG icon (15px), separated by a (7px) gap; selection uses a soft fill, accent text, and a thin accent underline. Interface language belongs in Options. Do not repeat the title-bar logo inside the rail.

Hovering a rail instance reveals a rich preview inheriting the shared tooltip surface, border, and theme Text color, with the rail-control corner token and (16px) padding. It has no shadow. Its nominal width is (280px), constrained by the viewport. The preview repeats actual library-card information: app logo, name, address, flags/language codes, status, instance type, and CPU/CUDA device when applicable. Place the (14px) type icon in the preview metadata row before CPU/CUDA. It opens to the right of the rail with an (8px) gap, falls back to the left if necessary, and clamps to the same (8px) viewport margins as other hints. Library cards show up to eight language badges plus a remaining-count badge; sidebar previews show up to three; cloud and remote instances use detected languages. The richer preview has its own tonal card treatment and shares the single active tooltip state.

### Cards and Language Chips

Library cards represent actual servers. Each card is a flex column; automatic space above the status/device row pins that row and its following divider/actions to the bottom. Cards in the same grid row keep their lower controls aligned when language chips occupy one or two lines. They combine a compact app image and name, deletion control, endpoint copy, language flags/codes, process status, CPU/CUDA icon, and direct Manage plus Install/Start/Stop controls. Card titles stay on one line and truncate with an ellipsis; a full-name tooltip and accessible label retain the complete instance name. Chips use a muted surface and compact padding. Explicit language names accompany SVG country/region flags inside selection controls. Flags come from lipis/flag-icons under assets/countries; interface SVGs under assets/icons inherit their control color.

### Models and Advanced Settings

Models uses a real data table: language pair with flags and names, installed version, available version, actual on-disk size, and download size. Each language pair uses an inner three-track grid: equal flexible source and target tracks around a centered chevron (14px), with (10px) gaps. The cell retains its table layout and a minimum width of (240px); the inner grid aligns arrows across rows regardless of language-name length. Header labels are muted (11px); body cells are (12px), separated by thin borders. Available updates use accent text and a small update label. Show checking, unknown-size, cached-catalog, empty, and error states explicitly. Keep Refresh and Update models together in compact controls; updates require a stopped, installed instance with no unsaved draft. Model replacements are transactional with interrupted-update recovery; progress and failures belong in real status/log output. At startup, synchronize every selected language pair: download and validate missing pairs even when other pairs are already installed, and retain installed versions unless an update was requested. Only after all required model and private sentence-model downloads succeed, remove deselected installed pairs, their retained previous versions, and unused private sentence models. A failed download prevents this pruning. Manual Update models uses the same validated installation routines without pruning deselected models.

Download activity lives in the title-bar Tasks popover. Models retains the version/size table rather than an always-visible Downloads list. Task transfers report actual received bytes against a known total or an explicit unknown-total label; percentages appear only when a total exists. Use the official Dioxus Progress and ProgressIndicator primitives, with a (5px) rounded track and green fill. Queued and validation phases are indeterminate, as are downloads with unknown totals; never invent completion percentages. Text distinguishes queued, downloading, validating, ready, failed, and stopped phases. Ready uses accent text, failure uses danger, and stopped transfers use a muted fill. Preserve received byte counts on Stop and when switching tabs, including completed entries.

Tasks separates active work from session History. Downloading and validating transfers appear before queued transfers; terminal entries follow in History, with failed/stopped entries before ready entries. Setup operations without active byte transfers show the instance name, a real installing/starting/updating status, and a spinner. Terminal records retain byte counts and can be dismissed individually or cleared from History; these actions never remove active work. History belongs to the current application session. Determinate fills use a (150ms) linear transform transition within a clipped, left-to-right track, avoiding layout animation. Unknown-progress motion is restrained, and reduced-motion preferences disable both transitions and animation.

At widths up to (1000px), each model table row becomes a two-column grid with a full-width language pair and visible per-cell labels. Preserve all data rather than trimming columns. Advanced begins with Runtime version, then reuses the existing fields, selects, and switches in groups for network access, translator page, API access, and models/metrics. Groups are separated by tone-neutral dividers and (28px) top spacing; section headings are compact (15px). Password fields keep generate/copy controls adjacent to the input.

### Tasks Popover

The title bar contains a compact Tasks trigger (28px high) using the official Dioxus Popover primitives. It shows the active count and selection-green treatment while work is active. The panel opens below the title bar at the right, nominally (390px) wide with viewport limits, a (12px) radius, panel fill, and thin border. Its maximum height leaves (54px) outside the panel, and its body scrolls internally. A localized title and close action remain above Active and History groups; empty state text is explicit. Task transfer rows reuse the official Progress primitive with compact phase/byte labels (10px); ready history entries show only the downloaded size, without a progress bar, repeated total, or 100% label. Dismiss and Clear history apply only to terminal records.

### Statistics

Show Statistics only when the instance's metrics setting is enabled. While that tab is mounted and the server is running, read the actual local prefixed metrics endpoint every five seconds; Refresh performs a manual read. Use the configured bearer token when required. Display real request totals, error totals, request-weighted average duration, and in-flight requests, followed by an endpoint table. Aggregate client and API-key labels away rather than displaying identities. Missing/invalid metrics and authorization failures have localized states; do not invent zero-valued statistics.

Four summary values sit in a flat grid between thin dividers, using the statistic type token and tabular numerals. At widths up to (900px), the summary becomes two columns. The endpoint table scrolls if needed, with text aligned at the start and numeric columns at the end. These are actual enabled-instance observations, not decorative dashboard metrics.

### In-app Translator

Translator is the first and default instance tab; Open translator in the instance header returns to it. A compact Text/Files mode control pairs each label with a local SVG icon. It uses a Workspace track with control-sized corners, a (3px) inset/gap, and a Surface fill for the selected mode. Both modes share language selectors using the actual running server's language list, auto-detect for the source, and valid target choices. While the instance is stopped or uninstalled, show a localized start/install prompt and dim disabled translation controls; language loading and connection failures also keep controls unavailable.

Text mode places input and output in two equal panes with a (16px) gap; they stack below (720px). At viewport heights up to (700px), the panel drops its minimum height, the panes use a (180px) minimum, and text areas use (140px) minima to preserve room for feedback. A centered square swap control (42px) sits between the language selectors. The input pane uses Input and the result uses Workspace; both use thin borders and popup-sized corners. Pane focus is an accent outline (2px) offset by (2px), and text uses the translation type token with automatic text direction.

Keep character count/limit and Clear with the input; place detected-language feedback and Copy with the result. Automatic translation begins after a (600ms) typing pause, waits for input-method composition to finish, and allows one request at a time. Muted status text and a spinner convey actual pending state; localized errors explain connection, timeout, authorization, and character-limit failures. Each instance retains its input, language choices, and result in memory across tab switches; completed draft revisions reuse their result. Editing clears the old result and detected language, and responses for older draft revisions are ignored. Requests use the local URL prefix and configured API key. No external browser or developer request/response panel is part of this translator flow.

Files mode uses a flat Workspace selection area with a dashed Border stroke, popup-sized corners, a muted file icon, the chosen filename, and a secondary Choose file action. Show the server's actual supported extensions, with localized unavailable/disabled/error states. Files are streamed without a fixed GUI size cap; server limits still apply. Native dialogs choose the source and save translated bytes. Translate file is an explicit green action with byte progress and a Cancel action; Save translated file appears after a successful result. Actions wrap with a (10px) gap. At viewport heights up to (700px), reduce selection padding and icon size to preserve the file actions.

### Locales and Direction

Options uses a searchable twenty-choice picker with each locale's autonym and flag. Persist canonical regional codes, including pt-BR, pt-PT, and zh-Hans; legacy bare pt remains Brazilian Portuguese while European Portuguese has its own pt-PT value. Retain the established fallback behavior for regional system locales. Language names used in translation pickers follow the project's embedded CLDR data; [locales/README.md](locales/README.md) records register and terminology guidance, including the limits of the model-assisted prose review.

Arabic mirrors content and rail layout while keeping the title bar and native window controls left-to-right. URLs, code, logs, paths, numeric/password inputs, and technical identifiers preserve left-to-right order; translator text areas choose direction from their own content. Toast placement and workspace corner placement mirror with the content. The Tasks popover content follows the interface locale while its title-bar anchor remains in the left-to-right chrome.

The translation-model language repeater retains Add language and adds Add all alongside it. Add all appends the missing entries from the fifty-language supported catalog without duplicates and uses the existing instance autosave. These model languages are independent of the twenty interface locales.

### Runtime Version Selection

Keep selected runtime, installed runtime, and latest available runtime distinct from the GUI application version. The first Advanced section presents installed/latest values in a wrapping summary and the selected version in the existing custom dropdown. Select latest changes the saved selection; Install selected version explicitly provisions it. Show when the installed runtime differs from the selection, and disable installation while edits remain unsaved, the instance is busy, or another installation/model update is active. A detected newer release adds an accent update note and a detail-header link to Advanced.

Version controls wrap with a (10px) gap; the selector grows from (180px) up to (280px), with (42px) action controls aligned to its lower edge. Check supported stable releases from PyPI at startup, hourly, and through Check for updates. Preserve cached or fallback choices offline and label them latest known rather than claiming a fresh check succeeded. Version choices come from the catalog; do not hardcode the GUI version as the runtime selection.

### Options and Paths

Keep interface language, theme, log following, data, cache, and Python paths together. Light, Dark, and System persist. Path copy/folder controls remain adjacent to the relevant path, with long paths allowed to wrap. Default paths resolve beside the executable; the restart requirement and lack of automatic migration remain explicit near the controls. CUDA availability is based on supported NVIDIA detection and remains distinct from runtime readiness. About displays the independent GUI version, Created by Neo, and the external [shadersmods.com](https://shadersmods.com/) link. Creator and website use a compact two-column definition list; the link uses accent text and an external-link icon.

### Application Updates

Check stable GUI releases at startup and expose Check for updates with inline status in About. A newer release opens an optional dialog; keep current/new version identifiers distinct from the managed LibreTranslate runtime version. The dialog uses Panel fill, card-sized corners, the existing dialog shadow, (26px) padding, a (560px) width cap, and a viewport-bounded height. Its green download icon and compact title lead into versions, then a keyboard-focusable, internally scrolling changelog on Workspace fill. The version chevron mirrors in Arabic; upstream changelog blocks and lines choose their own text direction so English notes retain left-to-right punctuation. Later and the green update action align at the lower end with a (10px) gap.

Accepted updates show actual download bytes and the shared thin Progress track, followed by indeterminate verifying/restarting phases. Disable dialog actions and dismissal during this work; unsupported packages disable the update action with an explanation. The verified package is staged and applied through the replacement helper, then the application reopens automatically. Instance data and preferences remain in place, and a failed replacement rolls back. Present localized failures through the established feedback surfaces.

### Logs, Feedback, and Confirmation

Logs use a scrollable monospace surface with filters and real process output. Error lines use red, warnings amber, successful lifecycle/package outcomes green, active downloads/installation blue, build/preparation purple, and low-priority details muted; unclassified lines retain normal text color. Preserve the message text so tone is never conveyed by color alone. Dioxus toasts appear at the bottom right without pushing page content, with a maximum of four visible. Success messages expire after five seconds; persistent errors retain expandable diagnostics and a dismiss control. Toast width is capped at (390px) with viewport margins. Deletion confirmation uses a centered panel capped at (470px) over a dimmed backdrop.

## Do's and Don'ts

### Do:

- Do preserve the compact Modrinth-inspired rail, rounded workspace, green actions, and neutral themes.
- Do show real readiness, installation progress, failures, and process logs.
- Do retain visible keyboard focus, explicit language names, room for translated labels, and Arabic direction handling.
- Do use the official Dioxus primitives and the existing local SVG asset sets.
- Do show automatic-save and validation status for existing instances, retain explicit creation/Options actions, and use toasts without moving page content.

### Don't:

- Don't add a second rail, duplicate title-bar branding, redundant workspace metadata bar, or footer.
- Don't use fake metrics, decorative dashboard filler, or repeated explanatory copy.
- Don't replace local interface SVG files with inline SVG or base64 icons in Rust.
- Don't offer CUDA as ready merely because supported hardware was detected; device availability and runtime readiness are separate states.
- Don't clip popup searches or allow floating option lists to leave the viewport.
