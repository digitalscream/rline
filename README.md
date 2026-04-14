# rline

A native Linux GUI text editor with AI-assisted coding features, built with Rust and GTK4.

For the avoidance of doubt, this isn't meant to be a project that works for everyone out-of-the-box. It's an experiment in personalised software - the idea is that you fork it, and get your favourite vibe-coding slop generator to add the features that you want in a simple, distraction-free development environment.

## Features

### Editor
- Tabbed editor built on `sourceview5` with tree-sitter incremental syntax highlighting
- Language auto-detection from file extension, modified-buffer indicator, close-with-unsaved-changes prompt
- Vertical split (Ctrl+\) into two side-by-side editor panes with cross-pane deduplication
- Tab context menu (Close, Close All, Close Others, Close All Left/Right) and MRU cycling via Ctrl+Tab
- Find/replace overlay bar (Ctrl+F / Ctrl+H) backed by `sourceview5::SearchContext`
- Configurable line numbers, current-line highlight, tab width, font family, letter spacing, line height, and hinting

### File Browser
- Recursive project tree with lazy loading and hidden-file support
- Single-click to open, right-click context menu for Open / Rename / Delete
- "Browse" button to pick a new project root via `gtk4::FileDialog`

### Project Search (Ctrl+Shift+F)
- Background, cancellable full-text search across the project
- Results grouped by file with expand/collapse arrows; files with few matches auto-expand
- Skips `.git`, `target`, `node_modules`, and binary files

### Quick Open (Ctrl+P)
- Modal fuzzy file finder using subsequence matching (up to 10,000 files indexed per project)

### Git Integration
- Staged / unstaged lists with M/A/D/R/C status badges and per-file stage, unstage, discard actions
- Stage All / Unstage All bulk actions and inline commit message input
- Side-by-side diff view with hunk highlighting, opened as an editor tab
- Auto-refresh when the Git tab becomes visible; all operations run on background threads via `git2`

### Status Bar
- Repository name and current branch
- Debounced git blame for the current cursor line (author, relative time, commit summary)

### Embedded Terminal
- Tabbed `vte4` terminals with a "+" button for new sessions
- Defaults to the project root (or `$HOME`) as the working directory
- Configurable font size

### AI Inline Completion
- Fill-in-the-middle completion against any OpenAI-compatible `/v1/completions` endpoint
- Automatic, manual, or combined trigger modes with configurable debounce, context lines, max tokens, and temperature
- Cancels in-flight requests on new edits to avoid stale suggestions

### AI Agent (Ctrl+Shift+A)
- Agentic chat panel modeled on the Cline VS Code extension, with **Plan** and **Act** modes
- Streaming SSE responses rendered as they arrive, with markdown formatting on completion
- Eleven built-in tools: `read_file`, `write_to_file`, `replace_in_file`, `list_files`, `search_files`, `execute_command`, `list_code_definition_names`, `ask_followup_question`, `attempt_completion`, `plan_mode_respond`, `browser_action`
- Headless-Chromium `browser_action` tool for launching URLs, clicking, typing, scrolling, and capturing screenshots (attached inline for multimodal models, otherwise saved to `.agent-cache/screenshots/`)
- Permission system: auto-approve by category (read files, edit files, safe commands, browser), with workspace-boundary enforcement and a safe-command whitelist; unsafe commands always require approval
- Token-aware context truncation with a live counter in the panel header
- Conversation persistence across sends and mode switches; "New Task" resets the context
- History saved to `.agent-history/` as timestamped Markdown files
- Reads `.clinerules` (file or directory) and `memory-bank/*.md` into the system prompt
- Works with any OpenAI-compatible `/v1/chat/completions` server supporting tool/function calling (llama.cpp, vLLM, Ollama, etc.)

### Theming
- Built-in GtkSourceView schemes plus automatic import of installed VS Code themes (converted to GtkSourceView XML)
- Rich TextMate scope resolution for UI chrome (sidebar, status bar, tabs) so the whole window matches the scheme
- Automatic light/dark text based on perceived brightness of the scheme background

### Settings
Persisted as JSON at `~/.config/rline/settings.json`, with three tabs:
- **Editor** — theme, fonts, tab width, terminal font, startup behavior, search and Ctrl+Tab options, tree-sitter toggle
- **Completion** — endpoint, API key, model, trigger mode, debounce, max tokens, context lines, temperature
- **Agent** — endpoint, API key, model, max tokens, temperature, context length, command timeout, auto-approve toggles, browser multimodal / viewport

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
| Ctrl+Q | Quit |
| Ctrl+Shift+F | Focus project search |
| Ctrl+Shift+G | Show git panel |
| Ctrl+Shift+E | Show files panel |
| Ctrl+Shift+W | Focus terminal |
| Ctrl+Shift+A | Focus agent panel |

## Supported Languages

Tree-sitter grammars are shipped for the following languages (each is feature-gated so unused grammars can be omitted from the binary):

| Language | Extensions |
|----------|------------|
| Rust | `.rs` |
| Python | `.py`, `.pyi`, `.pyw` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| C | `.c`, `.h` |
| C++ | `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hxx`, `.hh` |
| JSON | `.json` |
| Bash | `.sh`, `.bash`, `.zsh` |
| HTML | `.html`, `.htm` |
| CSS | `.css` |
| Markdown | `.md`, `.markdown` |
| Ruby | `.rb`, `.rake`, `.gemspec` |
| YAML | `.yaml`, `.yml` |
| XML | `.xml`, `.xsl`, `.xslt`, `.xsd`, `.svg` |
| HAML | `.haml` |

Files without a matching grammar still open with GtkSourceView's built-in highlighting where available.

## System Dependencies

Install the required development libraries before building. A Chromium or Chrome binary must also be on `PATH` for the agent's `browser_action` tool (`chromium`, `chromium-browser`, or `google-chrome`).

### Ubuntu / Debian

```bash
sudo apt-get install -y \
    libgtk-4-dev \
    libgtksourceview-5-dev \
    libvte-2.91-gtk4-dev \
    libgraphene-1.0-dev \
    chromium-browser
```

### Fedora

```bash
sudo dnf install -y \
    gtk4-devel \
    gtksourceview5-devel \
    vte291-gtk4-devel \
    graphene-devel \
    chromium
```

### Arch Linux

```bash
sudo pacman -S gtk4 gtksourceview5 vte4 graphene chromium
```

## Building

Requires Rust 1.85 or later.

```bash
cargo build              # Debug build
cargo build --release    # Release build
```

## Binary Pre-Requisites

If installing a pre-built binary rather than building from source:

```bash
sudo apt install libvte-2.91-gtk4-0 libgtksourceview-5-0 libgraphene-1.0-0
```

## Running

```bash
cargo run
```

## Development

```bash
cargo fmt --check                                          # Check formatting
cargo clippy -- -D warnings                                # Lint
cargo test --workspace                                     # Run tests
cargo fmt && cargo clippy -- -D warnings && cargo test --workspace  # Pre-commit checklist
```
