# CLAUDE.md

rline — native Linux GUI text editor with AI-assisted coding features. Rust + GTK4 (gtk4-rs), multi-crate workspace.

## Commands

```bash
# Build & Run
cargo build                      # Debug build
cargo build --release            # Release build
cargo run                        # Run the editor

# Testing
cargo test --workspace           # All workspace tests
cargo test -p rline-core         # Single crate tests
cargo test -- --nocapture        # With stdout output
cargo test <test_name>           # Specific test

# Quality (ALWAYS run before commits)
cargo fmt --check                # Check formatting
cargo clippy -- -D warnings      # Lint (warnings = errors)
cargo fmt && cargo clippy -- -D warnings && cargo test --workspace  # Pre-commit checklist

# Documentation
cargo doc --no-deps --document-private-items --open  # Generate & view docs
```

## System Dependencies

GTK4, GtkSourceView 5, and VTE4 development libraries must be installed:

```bash
# Ubuntu/Debian
sudo apt-get install -y libgtk-4-dev libgtksourceview-5-dev libvte-2.91-gtk4-dev libgraphene-1.0-dev

# Fedora
sudo dnf install -y gtk4-devel gtksourceview5-devel vte291-gtk4-devel graphene-devel

# Arch
sudo pacman -S gtk4 gtksourceview5 vte4 graphene
```

Requires Rust 1.85 or later.

## Tech Stack

| Crate | Purpose |
|-------|---------|
| `gtk4` (0.10, feature `v4_10`) | GUI framework (gtk4-rs bindings) |
| `sourceview5` (0.10) | Editor widget with syntax highlighting & themes |
| `vte4` (0.9) | Embedded terminal emulator widget |
| `glib` / `gio` (0.21) | GLib/GIO bindings (matching gtk4 0.10) |
| `tokio` | Async runtime for AI calls and I/O |
| `tokio-util` | Cancellation tokens |
| `serde` / `serde_json` | Serialisation (settings persistence) |
| `thiserror` | Library error types (per-crate) |
| `anyhow` | Application-level errors (binary crate only) |
| `tracing` / `tracing-subscriber` | Structured logging (never `println!` in production) |
| `directories` | XDG-compliant config/data paths |
| `git2` (0.20) | Git repository operations (status, blame, diff, staging) |
| `tree-sitter` / `tree-sitter-highlight` (0.25) | Incremental syntax parsing (14 language grammars) |
| `pango` (0.21) | Text layout and rendering |
| `reqwest` (0.12) | HTTP client for AI API calls (streaming SSE) |
| `regex` (1) | Regex search in tools and code definition extraction |
| `bytes` (1) | Byte buffer handling for streaming responses |
| `futures-core` (0.3) | Stream trait for reqwest byte streams |

**Planned but not yet used**: `ropey` (rope data structure).

## Workspace Architecture

```
rline/
  Cargo.toml              # Workspace root with [workspace.dependencies]
  src/
    main.rs                # Application entry point — thin, just wires crates together
  crates/
    rline-core/            # Position types, document metadata, search result types
      src/
        lib.rs             # Re-exports
        error.rs           # CoreError enum
        position.rs        # LineIndex, CharOffset, ByteOffset newtypes
        document.rs        # DocumentId, DocumentMeta
        search.rs          # SearchResult struct
    rline-config/          # Settings persistence via JSON at ~/.config/rline/
      src/
        lib.rs             # Re-exports
        error.rs           # ConfigError enum
        paths.rs           # XDG path resolution
        settings.rs        # EditorSettings (theme, font sizes, behavior toggles)
        syntax_theme.rs    # Rich TextMate scope resolution for imported themes
        vscode_import.rs   # VS Code theme discovery and conversion to GtkSourceView XML
    rline-ai/              # AI completion client + agentic tool-use engine
      src/
        lib.rs             # Re-exports, module declarations
        client.rs          # CompletionClient — FIM inline completion (v1/completions)
        error.rs           # AiError enum (HTTP, API, cancelled, tool, I/O, regex)
        runtime.rs         # Dedicated single-thread tokio runtime (ai_runtime())
        types.rs           # CompletionRequest/Response for FIM
        chat/
          mod.rs
          client.rs        # ChatClient — streaming SSE to /v1/chat/completions
          stream.rs        # SSE parser, ToolCallAccumulator for incremental parsing
          types.rs         # ChatMessage, ToolCall, ToolDefinition, ChatRequest, streaming deltas
        agent/
          mod.rs
          context.rs       # ConversationContext — message history, token tracking, truncation
          event.rs         # AgentEvent enum (TextDelta, ToolCall, Approval, FileChanged, etc.)
          loop.rs          # AgentLoop — core multi-turn orchestrator with Plan/Act modes
        tools/
          mod.rs           # Tool trait, ToolRegistry, ToolCategory, ToolResult
          definitions.rs   # JSON Schema builder macro
          read_file.rs     # Read file contents with optional line range
          write_to_file.rs # Create/overwrite files
          replace_in_file.rs # SEARCH/REPLACE block editing
          list_files.rs    # Recursive directory listing
          search_files.rs  # Regex search across project files
          execute_command.rs # Shell command execution with timeout
          list_code_definition_names.rs # Regex-based code structure extraction
          ask_followup_question.rs # Interactive: ask user for clarification
          attempt_completion.rs    # Interactive: signal task complete (Act mode only)
          plan_mode_respond.rs     # Interactive: present plan (Plan mode only)
    rline-syntax/          # Tree-sitter integration — incremental syntax highlighting
      src/
        lib.rs             # Crate-level docs, re-exports
        error.rs           # SyntaxError enum
        engine.rs          # HighlightEngine (parser + incremental highlighting)
        languages.rs       # SupportedLanguage enum, extension→grammar mapping (14 languages)
        scope_map.rs       # Tree-sitter capture names → GtkSourceView style IDs
        span.rs            # HighlightSpan, IncrementalResult types
    rline-ui/              # GTK4 application — all UI lives here
      src/
        lib.rs             # RlineApplication re-export, run() entry point
        app.rs             # RlineApplication (gtk4::Application subclass)
        window.rs          # RlineWindow — three-pane layout, action wiring, startup restore
        error.rs           # UiError enum
        menu.rs            # Hamburger menu (Settings entry)
        shortcuts.rs       # Keyboard accelerator registration
        theming.rs         # App-wide theme derived from sourceview scheme + VS Code UI colors
        status_bar.rs      # Bottom bar: repo name, branch, git blame for current line
        editor/
          mod.rs
          editor_pane.rs   # Tabbed notebook of EditorTab instances
          tab.rs           # Single sourceview5::View with buffer, language detection, modified indicator
          find_bar.rs      # Compact overlay find/replace bar (Ctrl+F / Ctrl+H)
          split_container.rs # Manages 1–2 side-by-side EditorPanes with cross-pane dedup
          syntax_highlighter.rs # Bridges tree-sitter spans to GtkSourceView TextTags
          settings_dialog.rs # Settings window (theme, fonts, behavior)
        file_browser/
          mod.rs
          browser_panel.rs # Browse button, TreeListModel tree, right-click context menu
          file_node.rs     # FileNode GObject subclass for tree items
          file_tree.rs     # Directory model builder with lazy loading
        search/
          mod.rs
          project_search.rs # Grouped-by-file search results with expand/collapse
          search_worker.rs  # Background file search + file path collection
          quick_open.rs     # Ctrl+P fuzzy file finder dialog
        git/
          mod.rs
          git_panel.rs     # Staged/unstaged file lists with stage/unstage/discard actions
          git_status_row.rs # Individual file status row widget
          git_worker.rs    # Background git ops: status, diff, blame, staging, commit
          diff_tab.rs      # Side-by-side diff view with hunk highlighting
        agent/
          mod.rs
          agent_panel.rs   # Right-pane chat UI: mode selector, message area, input, context counter
          message_widget.rs # Widget factories for user/AI/tool/completion/error/plan messages
          markdown.rs      # Markdown-to-Pango markup converter for AI responses
          permission.rs    # Auto-approve policy: safe command detection, workspace path checks
        terminal/
          mod.rs
          terminal_pane.rs # Tabbed notebook of terminal instances
          terminal_tab.rs  # Single VTE terminal with font size support
```

Dependency direction flows inward: `rline-ui` → `rline-core`, `rline-config`, `rline-ai`, `rline-syntax`. `rline-core` has no workspace dependencies. No circular dependencies between crates.

## Current Feature Set

### Layout
Three resizable columns using nested `gtk4::Paned` widgets (1px separators):
- **Left**: `gtk4::Stack` with three tabs (Files, Git, Search)
- **Middle**: Vertical split — editor tabs (top) + terminal tabs (bottom)
- **Right**: AI agent chat panel (Plan/Act mode selector, message history, input area)
- **Bottom**: Status bar (repo name, branch, git blame)

### File Browser (left pane, "Files" tab)
- "Browse" button opens `gtk4::FileDialog` to select project directory
- Recursive directory tree via `TreeListModel` + `ListView` with lazy loading
- Shows hidden files (dot files)
- Single-click opens files in editor
- Right-click context menu: Open, Rename, Delete

### Editor (middle pane, top)
- Tabbed `sourceview5::View` with syntax highlighting and language auto-detection
- Tree-sitter incremental highlighting bridged to GtkSourceView TextTags (14 languages)
- Modified indicator ("●" prefix) on tab labels
- Save/Discard/Cancel dialog on close of modified buffers
- Line numbers, current-line highlight, configurable tab width
- Find/replace overlay bar (Ctrl+F / Ctrl+H) using `sourceview5::SearchContext`
- Vertical split (Ctrl+\) — two side-by-side editor panes with cross-pane file deduplication
- Tab context menu: Close, Close All, Close Others, Close All Left/Right
- MRU tab cycling with Ctrl+Tab

### Git Integration (left pane, "Git" tab)
- Staged and unstaged file lists with status badges (M/A/D/R/C)
- Stage, unstage, and discard actions per file (single-click buttons)
- Stage All / Unstage All bulk actions
- Commit with inline message input
- Side-by-side diff view with hunk highlighting (opens in editor tab)
- Auto-refresh when Git tab becomes visible
- All git operations run on background threads via `git2`

### Status Bar (bottom of window)
- Repository name (from git workdir)
- Current branch name
- Git blame for current cursor line (author, relative time, commit summary)
- Blame updates debounced (300ms) on background thread
- Tracks active buffer via cursor-position signal

### Terminal (middle pane, bottom)
- Tabbed `vte4::Terminal` widgets with "+" button for new terminals
- Default working directory = project root or `$HOME`
- Configurable font size

### Project Search (left pane, "Search" tab, Ctrl+Shift+F)
- Full-text search across project files (background thread, cancellable)
- Results grouped by file with expand/collapse (▶/▼ arrows)
- Files with ≤ N matches auto-expanded (N configurable, default 5)
- Single-click on line result opens file at that line
- Skips `.git`, `target`, `node_modules`, and binary files

### Quick Open (Ctrl+P)
- Modal dialog with fuzzy subsequence matching on filenames
- File index collected from project root (capped at 10,000 files)

### AI Agent (right pane, Ctrl+Shift+A)
- Agentic tool-use chat panel modeled after the Cline VS Code extension
- **Two modes**: Plan (read-only analysis) and Act (full execution), selectable via dropdown
- **Plan mode**: read-only tools only (`read_file`, `list_files`, `search_files`, `list_code_definition_names`, `ask_followup_question`, `plan_mode_respond`). Model calls `plan_mode_respond` to present its plan; user then switches to Act mode.
- **Act mode**: all tools including file writes, command execution, and `attempt_completion`
- **10 built-in tools**: `read_file`, `write_to_file`, `replace_in_file`, `list_files`, `search_files`, `execute_command`, `list_code_definition_names`, `ask_followup_question`, `attempt_completion`, `plan_mode_respond`
- **Streaming responses**: text displayed as it arrives via SSE parsing, markdown rendered on completion
- **Permission system**: auto-approve by category (read files, edit files, safe commands) with workspace boundary enforcement. Unsafe commands always require approval.
- **Safe command detection**: whitelist of read-only/build/test commands matching Cline's classification; shell operators (`|`, `;`, `>`, `&`) force manual approval
- **Tool call UI**: collapsible cards showing tool name + key argument (e.g. file path, command), approve/deny buttons for non-auto-approved calls
- **Follow-up questions**: multi-line text area with Enter to submit, Shift+Enter for newline
- **Context management**: token-aware truncation removing oldest complete exchanges; live counter ("12k / 128k") in header
- **Conversation persistence**: context preserved across sends and mode switches within the same task; "New Task" button resets
- **Conversation history**: saved to `.agent-history/` directory as timestamped Markdown files, updated after each agent run
- **Diff view**: file edits automatically open side-by-side diff in the editor (via git unstaged diff)
- **Cline compatibility**: loads `.clinerules` (file or directory) and `memory-bank/*.md` into the system prompt
- **OpenAI-compatible API**: works with any local model server (llama.cpp, vLLM, Ollama, etc.) that supports `/v1/chat/completions` with tool/function calling
- All AI operations run on dedicated tokio runtime (`ai_runtime()`) — never blocks GTK main thread

### Settings (hamburger menu → Settings)
Three tabs: **Editor**, **Completion**, **Agent**.

**Editor tab:**
- Theme (dropdown of all sourceview5 schemes + imported VS Code / Zed themes)
- Editor font family and size, letter spacing, line height, font hinting
- Tab width, insert spaces
- Terminal font family and size
- Open last project on startup (toggle)
- Search auto-expand threshold, Ctrl+Tab cycle depth
- Tree-sitter highlighting toggle

**Completion tab (inline FIM):**
- Enable/disable, endpoint URL, API key, model
- Trigger mode (automatic/manual/both), debounce, max tokens, context lines, temperature

**Agent tab:**
- Endpoint URL (auto-appends `/chat/completions` if needed), API key, model
- Max tokens, temperature, context length (tokens)
- Command timeout (seconds)
- Auto-approve permissions (Act mode only): read files, edit files, execute safe commands

- Persisted as JSON at `~/.config/rline/settings.json`
- Uses `#[serde(default)]` for forward-compatible deserialization

### Theming
- Application chrome color derived from sourceview scheme background
- VS Code theme import: auto-discovers installed VS Code extensions, converts theme JSON to GtkSourceView XML schemes
- Rich TextMate scope resolution via `SyntaxTheme` for UI element colors (sidebar, status bar, tabs, etc.)
- Perceived brightness detection → automatic light/dark text
- GTK dark theme preference set via `gtk4::Settings::set_gtk_application_prefer_dark_theme`
- 1px pane separators colored to match theme

### Syntax Highlighting (rline-syntax)
- Tree-sitter incremental parsing via `HighlightEngine`
- 14 language grammars (feature-gated): Rust, Python, JavaScript, C, C++, JSON, Bash, HTML, CSS, Markdown, Ruby, YAML, XML, HAML
- Scope mapping: tree-sitter capture names → GtkSourceView style IDs
- Incremental re-highlighting on buffer edits

### Keyboard Shortcuts
| Shortcut | Action |
|----------|--------|
| Ctrl+O | Open file |
| Ctrl+S | Save file |
| Ctrl+W | Close current editor tab |
| Ctrl+P | Quick open (fuzzy file finder) |
| Ctrl+F | Find in current file |
| Ctrl+H | Find and replace in current file |
| Ctrl+\ | Split editor vertically |
| Ctrl+Tab | Cycle MRU tabs |
| Ctrl+Q | Quit application |
| Ctrl+Shift+F | Focus project search |
| Ctrl+Shift+G | Show git panel |
| Ctrl+Shift+E | Show files panel |
| Ctrl+Shift+W | Focus terminal |
| Ctrl+Shift+A | Focus agent panel |

### Startup Behavior
- Last opened project restored automatically (if enabled in settings)
- Theme applied globally on startup
- Status bar populated with repo info on project restore

## Rust Code Style

### Ownership & Borrowing

- Prefer borrowing (`&T`, `&mut T`) over cloning. Clone only when ownership transfer is genuinely needed.
- Use `&str` in function parameters, not `String`, unless the function needs to own the string.
- Use `Cow<'_, str>` when a function sometimes needs to allocate and sometimes does not.
- Never use `Rc`/`Arc` to "make the borrow checker happy" — redesign the data flow instead. GTK widget references are the one exception (GTK is inherently reference-counted).

### Error Handling (NON-NEGOTIABLE)

- **NEVER** use `.unwrap()` or `.expect()` in library code. Only acceptable in tests and `main.rs` initial setup.
- Each library crate defines its own error enum with `#[derive(Debug, thiserror::Error)]`.
- Use `anyhow::Result` only in `main.rs` and application-level code, never in library crates.
- Use the `?` operator for propagation. Never `match` on `Result` just to re-wrap it.

```rust
// Per-crate error type pattern
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("position {0} out of bounds for document of length {1}")]
    PositionOutOfBounds(usize, usize),
    #[error("invalid UTF-8 in document")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

### Naming Conventions

- Types: `PascalCase` — `DocumentBuffer`, `SyntaxHighlighter`
- Functions/methods: `snake_case` — `insert_text`, `get_line_at`
- Constants: `SCREAMING_SNAKE_CASE`
- Modules: `snake_case`, one concept per module
- Crates: `rline-{name}` (kebab-case)

### Struct Design

- Private fields with public methods. No public fields unless the type is a plain data struct.
- Builder pattern for types with more than 3 construction parameters.
- `#[derive(Debug)]` on everything. Derive `Clone`, `PartialEq` only when semantically meaningful.
- Use type-safe wrappers for positions: `ByteOffset(usize)`, `CharOffset(usize)`, `LineIndex(usize)` — never bare `usize` for coordinates.

### Traits

- Define traits for abstractions with multiple implementations (e.g., `AiProvider` for different backends).
- Keep trait methods minimal. Provide default implementations where sensible.
- Prefer `impl Trait` in argument position over `dyn Trait` unless runtime polymorphism is required.

### Async & Concurrency

- GTK4 runs on the main thread. **NEVER** block the main thread with `.await` or synchronous I/O.
- Use `tokio::spawn` for async work. Communicate results back to GTK via `glib::MainContext::channel()`.
- Use `glib::spawn_future_local` for futures that need to touch GTK widgets.
- For background file I/O (search), use `std::thread::spawn` + `std::sync::mpsc` + `glib::idle_add_local` to poll results.
- AI API calls must be fully async with cancellation support (`tokio_util::sync::CancellationToken`).

### Imports

Group imports in this order, separated by blank lines:

1. `std` library
2. External crates
3. Workspace crates (`rline_core`, `rline_ai`, etc.)
4. Local modules (`crate::`, `self::`)

### Clippy

All code must pass `cargo clippy -- -D warnings`. No `#[allow(clippy::...)]` without a comment explaining why.

### Comments

- `///` doc comments on all public items. Include `# Examples` section for non-obvious APIs.
- Inline comments explain WHY, never WHAT. No ticket numbers in comments.
- Use `// TODO:` with a description. No bare `// FIXME`.

## GTK4 Patterns

- GTK widgets are reference-counted — `Clone` is cheap and shares the underlying object.
- Use `glib::clone!(#[weak] widget, move |_| { ... })` in signal handlers to avoid preventing widget destruction.
- Prefer `#[weak]` references in closures unless you genuinely need the widget to stay alive.
- Build UI in code (not XML/Glade) for better type safety and refactorability.
- Use `EventControllerKey` for keyboard input, not deprecated key event signals.
- For single-click behavior on `ListView`, use `GestureClick` (button 1) — `connect_activate` fires on double-click.
- GObject subclasses are required for items in `gio::ListStore` — use `glib::Properties` derive macro.

### Async → GTK Bridge

```rust
// Pattern: background thread sends results to GTK main thread
let (sender, receiver) = std::sync::mpsc::channel();

std::thread::spawn(move || {
    // Do blocking work, send results via sender
});

glib::idle_add_local(move || {
    match receiver.try_recv() {
        Ok(result) => { /* update UI */ glib::ControlFlow::Continue }
        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(TryRecvError::Disconnected) => glib::ControlFlow::Break,
    }
});
```

## Testing Requirements

- **Unit tests**: `#[cfg(test)] mod tests { ... }` in each module file. Test all public functions.
- **Integration tests**: In `crates/<name>/tests/` for cross-module behaviour.
- **Doc tests**: All `# Examples` in doc comments must compile and pass.
- **Naming**: `test_<function_name>_<scenario>` — e.g., `test_insert_text_at_beginning`, `test_delete_empty_document`.
- **Pattern**: Arrange-Act-Assert with descriptive assertion messages.
- **No external dependencies**: Tests must not depend on network, filesystem outside temp dirs, or environment variables.
- **Async tests**: Use `#[tokio::test]` for async test functions.
- **Test command**: Always use `cargo test --workspace` to run all crate tests.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_text_at_beginning() {
        // Arrange
        let mut doc = Document::new("hello world");

        // Act
        doc.insert(0, "say ").unwrap();

        // Assert
        assert_eq!(doc.text(), "say hello world", "text should be prepended");
    }
}
```

## Documentation Standards

- `///` doc comments on all public structs, enums, traits, functions, and methods.
- `//!` module-level docs at the top of each file explaining the module's purpose.
- Crate-level docs in `lib.rs` explain the crate's purpose and relationship to other workspace crates.
- Include `# Examples` for non-obvious APIs. These also serve as doc tests.
- README.md for users, doc comments for developers.

## Domain Terminology

| Term | Meaning |
|------|---------|
| Document / Buffer | In-memory representation of a file (backed by sourceview5 Buffer in iteration 1) |
| View | A visible editor pane showing a document |
| Cursor / Caret | The insertion point in a document |
| Selection | A range of text (anchor + head positions) |
| Provider | An AI backend (OpenAI-compatible, Claude, local model) |
| Completion | AI-generated code suggestion (inline FIM) |
| Command | An editor action (not a shell command) |
| Chrome | Application UI surrounding the editor (sidebars, headers, tab bars) |
| Plan Mode | Agent read-only mode: analyze code, present plan via `plan_mode_respond` |
| Act Mode | Agent execution mode: full tool access, file writes, command execution |
| Tool Call | A function the AI model requests to execute (read_file, execute_command, etc.) |
| Agent Loop | The multi-turn cycle: send context → stream response → execute tools → repeat |
| Context | The full conversation history (system prompt + messages) sent to the AI model |

## Key Rules (NON-NEGOTIABLE)

1. **Never `.unwrap()` in library code** — use `?` and proper error types
2. **Never block the GTK main thread** — all I/O and AI calls are async or on background threads
3. **Run `cargo fmt && cargo clippy -- -D warnings && cargo test --workspace` before every commit**
4. **Document all public APIs** with `///` doc comments
5. **Test all functionality** — unit tests in modules, integration tests in `tests/`
6. **Use `tracing` for logging** — never `println!` in production code
7. **Prefer composition over inheritance** — use traits and composition, not deep type hierarchies
8. **Keep crate boundaries clean** — no circular dependencies between workspace crates
9. **Use type-safe wrappers** for positions — `LineIndex(usize)`, `CharOffset(usize)`, not bare `usize`
10. **Cancel previous AI requests** before starting new ones — avoid stale completions
11. **Use `#[weak]` in GTK signal closures** — prevent preventing widget destruction
12. **Single-click for all interactive lists** — use `GestureClick`, not `connect_activate`

## Slash Commands

| Command | Use when |
|---------|----------|
| `/architect` | Planning crate structure, module boundaries, or data flow for a new feature |
| `/code-reviewer` | Reviewing completed code for Rust idiomaticity, safety, and project standards |
| `/test-writer` | Writing comprehensive tests for new or changed functionality |
| `/gtk4-ui` | Building GTK4 widgets, layouts, or signal handling |
| `/ai-integration` | Implementing AI provider calls, streaming, or response parsing |
