# Feature Specification: Visual Debugger UI

**Feature Branch**: `005-visual-debugger-ui`

**Created**: 2026-05-24

**Status**: Draft

**Input**: Phase 4 — a native egui application that serves as both a manual debugger interface and an AI-session observer, providing a visual window into what the debugger and AI agent are doing in real time.

---

## Context

This feature creates `apps/visual-debugger`, a standalone egui desktop application that displays the current debugging session in a rich visual format. The UI renders the source file being debugged, highlights the current execution line in yellow, shows breakpoints in the gutter, and provides a full toolbar of standard debugger controls with keyboard shortcuts.

The application has a dual purpose: a human operator can drive the debug session manually; simultaneously, the UI reflects every action the AI agent takes — button presses animate, breakpoints appear in real time, and the viewed file switches automatically as the AI steps into different source files.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Manual Debug Session (Priority: P1)

A developer opens the visual debugger, selects a binary to debug, and drives the session using the toolbar and keyboard shortcuts: setting breakpoints by clicking the gutter, stepping through code, and inspecting the execution state. The source file for the current frame is shown at all times with the active line highlighted.

**Why this priority**: This is the primary human interaction path. Without a working toolbar and source view, no other scenario is possible.

**Independent Test**: Launch the app, load a binary, click "Continue" (F5), then click in the gutter next to line 10 to set a breakpoint. Assert the red dot appears in the gutter. Assert the toolbar "Continue" button shows a pressed animation when F5 is pressed.

**Acceptance Scenarios**:

1. **Given** the app is open, **When** the user presses F5, **Then** the Continue button shows a pressed visual state for 200ms and the debug session resumes.
2. **Given** a source file is loaded, **When** the user clicks the gutter area next to a line, **Then** a breakpoint indicator (red dot) appears at that line; clicking again removes it.
3. **Given** execution is paused, **When** the user presses F10, **Then** the Step Over action fires, and the yellow highlight moves to the next line.
4. **Given** the toolbar is visible, **When** the user hovers over any button, **Then** a tooltip displays the button's action name and keyboard shortcut.

---

### User Story 2 — AI Agent Observation (Priority: P2)

While the AI agent drives the debug session (via MCP tools), the human observer can watch every action reflected live in the UI: buttons visually press, breakpoints appear and disappear in the gutter, the yellow highlight moves, and the active source file switches automatically when the AI steps into a different file.

**Why this priority**: The "assist" half of the spec — the UI as a real-time AI observer — depends on the manual UI foundation (P1) being in place. AI-driven animation is an overlay on the same controls.

**Independent Test**: Programmatically inject an AI action `step_over` into the UI state. Assert the Step Over button shows a pressed animation and the yellow highlight advances one line. Inject `set_breakpoint(file, line)` and assert the gutter dot appears.

**Acceptance Scenarios**:

1. **Given** the AI agent calls Step Into, **When** the UI receives the action event, **Then** the Step Into button shows the same pressed visual state a human click would produce, lasting 200ms.
2. **Given** the AI agent sets a breakpoint at `src/main.rs:42`, **When** the UI receives the event, **Then** a red dot appears in the gutter at line 42 without any user interaction.
3. **Given** the AI agent steps into a function in a different source file, **When** the debug session changes the active frame, **Then** the source viewer switches to the new file and highlights the new active line in yellow.
4. **Given** the AI removes a breakpoint, **When** the UI receives the event, **Then** the red dot at that line disappears.

---

### User Story 3 — File Tree Navigation (Priority: P3)

A developer uses the file tree panel to browse the project directory and open any source file in the viewer, independent of the current debug frame. This allows inspecting code that has not yet been reached during execution.

**Why this priority**: Navigation convenience. The debug flow works without it (the viewer auto-switches on frame changes), but exploring the codebase requires it.

**Independent Test**: Open the app. Expand the file tree to `src/`. Click on `lib.rs`. Assert the source viewer loads and displays the file content with line numbers. Assert no yellow highlight is shown (no current execution frame in that file).

**Acceptance Scenarios**:

1. **Given** the file tree is visible, **When** the user expands a directory node, **Then** child files and subdirectories are shown beneath it.
2. **Given** a file is selected in the tree, **When** the viewer loads it, **Then** all lines are shown with line numbers; `.rs` files display with basic syntax colouring (keywords, strings, comments).
3. **Given** the debug session moves to a different file, **When** the active frame changes, **Then** the file tree highlights the newly active file and the viewer scrolls to the active line.

---

### User Story 4 — Address Bar and Path Display (Priority: P4)

The address bar at the top of the source viewer always shows the absolute path of the currently displayed file. The user can manually type or paste a file path to navigate directly to that file.

**Why this priority**: Utility feature. Useful when paths are long or files are outside the project tree. Depends on the source viewer (P1) being functional.

**Independent Test**: Display a file at path `/workspace/rdc_app/src/main.rs`. Assert the address bar contains that full path. Clear the bar, type `/workspace/rdc_app/src/lib.rs`, press Enter. Assert the viewer loads `lib.rs`.

**Acceptance Scenarios**:

1. **Given** a file is shown in the viewer, **When** the address bar is read, **Then** it contains the full absolute path of the displayed file.
2. **Given** the user types a valid path in the address bar and presses Enter, **When** the file exists, **Then** the viewer loads and displays that file.
3. **Given** the user types an invalid or non-existent path, **When** Enter is pressed, **Then** an inline error message is shown in the address bar and the current file remains visible.

---

### Edge Cases

- Source file has more than 10,000 lines — viewer must remain responsive (no full rerender of all lines on scroll).
- Active execution line is outside the current viewport — viewer automatically scrolls to bring the highlighted line into view.
- File changes on disk during a debug session (recompile) — viewer detects the change and reloads the file.
- Breakpoint set on a line that has no executable code — indicator is shown but flagged as unresolvable (e.g., dimmed dot).
- AI fires multiple actions in rapid succession (< 50ms apart) — each button press animation is shown; if animations would overlap, they queue without skipping.
- Binary path is invalid or binary crashes on launch — an error banner is shown in the UI without crashing the debugger app itself.
- Deep directory tree (> 5 levels) — file tree must be scrollable and not overflow the panel.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The application MUST display a toolbar with the following 11 debugger actions, each with its icon, label, and keyboard shortcut tooltip: Continue (F5), Break All (Ctrl+Alt+Break), Stop Debugging (Shift+F5), Restart (Ctrl+Shift+F5), Step Back Into (Ctrl+R, F11), Step Back Over (Ctrl+R, F10), Step Back Out (Ctrl+R, Shift+F11), Show Next Statement (Alt+Num *), Step Into (F11), Step Over (F10), Step Out (Shift+F11), and a Show Threads in Source toggle.
- **FR-002**: The toolbar MUST respond to all listed keyboard shortcuts in addition to mouse clicks.
- **FR-003**: Each toolbar button MUST display a "pressed" visual state (depressed appearance) for exactly 200ms when activated — whether by the human user or by the AI agent.
- **FR-004**: The source viewer MUST render `.rs` files with line numbers and basic syntax colouring (keywords, string literals, comments, types).
- **FR-005**: The source viewer MUST display a yellow highlight on the line corresponding to the current debug execution frame.
- **FR-006**: The left gutter of the source viewer MUST allow the user to click to add or remove a breakpoint; confirmed breakpoints MUST be shown as a red filled dot.
- **FR-007**: The source viewer MUST automatically switch to the correct source file and scroll to the active line when the debug frame changes, whether triggered by the human or the AI agent.
- **FR-008**: The address bar MUST always reflect the path of the currently displayed file and MUST accept typed/pasted paths for manual navigation.
- **FR-009**: The file tree panel MUST display the directory tree rooted at the project directory, support expand/collapse of directories, and allow single-click to open a file in the source viewer.
- **FR-010**: The application MUST accept real-time events from the AI agent (via the existing `IntrospectionStore` or a similar shared-state bus) that trigger button press animations, breakpoint changes, file switches, and line highlight updates.
- **FR-011**: The application MUST connect to the existing MCP/debug bridge (`win-debug-bridge`) to execute real debug operations when the human user activates toolbar controls.
- **FR-012**: The source viewer MUST keep the active line in the visible viewport at all times when execution pauses; it MUST scroll automatically when the highlighted line moves out of view.

### Key Entities

- **DebugSession**: The current state of the debug session — active file, active line, list of breakpoints, running/paused state, thread list.
- **BreakpointEntry**: A source location (file path + line number) with a resolved/unresolvable status flag.
- **SourceView**: The rendered source file — file path, total line count, visible line range, syntax-highlighted line content.
- **ToolbarAction**: One of the 11 debugger commands. Has a pressed state (bool) and a timestamp of last activation.
- **AiActionEvent**: A signal from the AI agent identifying which `ToolbarAction` was fired or which file/line/breakpoint changed, used to mirror AI operations in the UI.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 11 toolbar buttons and their keyboard shortcuts are functional and fire the correct debug operation in 100% of tests.
- **SC-002**: The pressed visual state appears within one rendered frame (< 16ms) of a user keypress or AI action event, and lasts 200ms ± 10ms.
- **SC-003**: The source viewer scrolls to the active execution line within 100ms of a frame change during a debug session.
- **SC-004**: The file tree renders a project with 500+ files without noticeable lag (< 100ms to expand any directory node).
- **SC-005**: Breakpoints set or removed by the AI agent appear or disappear in the gutter within one rendered frame (< 16ms) of the event.
- **SC-006**: The active source file switches and the new file is fully rendered within 200ms of a frame-change event that crosses file boundaries.
- **SC-007**: A human observer watching the screen can follow every AI step without any action being skipped or unrendered.

---

## Assumptions

- The debugger app runs on Windows 10/11 x86-64; cross-platform support is out of scope.
- The binary to debug is selected at startup (via CLI argument or a startup dialog); changing the binary mid-session is out of scope.
- Syntax colouring covers the most visually important Rust tokens (keywords, string literals, comments, type names); full language-server-grade accuracy is not required.
- The AI agent communicates with the UI via an in-process shared state (equivalent to the `IntrospectionStore` pattern from feature 003), not via a separate process or IPC.
- Step-back commands (Step Back Into/Over/Out) are wired to the toolbar UI but their backend execution depends on debugger support; they are visually present and fire the action even if the backend returns "unsupported".
- The "Show Threads in Source" feature shows a list of threads; the detailed thread inspector panel is out of scope for this feature.
- Source file encoding is UTF-8; other encodings are out of scope.
- The file tree root is the working directory when the app is launched.
