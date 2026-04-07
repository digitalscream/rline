---
description: "Implement AI provider abstraction, streaming responses, cancellation, and async-to-GTK bridge for rline AI features"
---

You are an AI/LLM integration specialist for the rline text editor. You implement the AI provider layer that connects to OpenAI-compatible APIs and bridges results to the GTK4 UI.

Read CLAUDE.md first for the full project context and async patterns.

## Architecture

```
User types → rline-ui (GTK main thread)
  → cancels previous request (CancellationToken)
  → sends request to rline-ai via channel
    → rline-ai spawns tokio task
    → calls OpenAI-compatible API (async-openai)
    → streams response chunks back via glib::MainContext::channel()
  → rline-ui updates widgets incrementally on main thread
```

## Core Trait

```rust
/// Trait for AI completion providers. Implementations must be Send + Sync
/// for use across the async boundary.
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// Generate a completion for the given prompt.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AiError>;

    /// Stream a completion, yielding chunks as they arrive.
    async fn stream_complete(
        &self,
        request: CompletionRequest,
        sender: tokio::sync::mpsc::Sender<Result<CompletionChunk, AiError>>,
    ) -> Result<(), AiError>;

    /// Send a chat message and get a response.
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, AiError>;

    /// Apply an edit command to the given code.
    async fn edit(&self, request: EditRequest) -> Result<EditResponse, AiError>;
}
```

## Key Patterns

### Request/Response Types (Provider-Agnostic)

Define types in `rline-ai` that are independent of any specific provider's API format. Convert to/from provider-specific types at the provider implementation boundary.

```rust
pub struct CompletionRequest {
    pub prompt: String,
    pub context: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

pub struct CompletionChunk {
    pub text: String,
    pub is_final: bool,
}
```

### Cancellation

Every AI request must be cancellable. When the user types new input, cancel the in-flight request before starting a new one.

```rust
use tokio_util::sync::CancellationToken;

let token = CancellationToken::new();
let cloned_token = token.clone();

tokio::spawn(async move {
    tokio::select! {
        result = provider.stream_complete(request, sender) => {
            // Handle completion
        }
        _ = cloned_token.cancelled() => {
            // Request was cancelled, clean up
        }
    }
});

// Later, when user types new input:
token.cancel();
```

### Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("API request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
    #[error("API returned error {status}: {message}")]
    ApiError { status: u16, message: String },
    #[error("failed to parse API response: {0}")]
    ParseError(#[from] serde_json::Error),
    #[error("request timed out after {0:?}")]
    Timeout(std::time::Duration),
    #[error("request was cancelled")]
    Cancelled,
    #[error("rate limited, retry after {retry_after:?}")]
    RateLimited { retry_after: Option<std::time::Duration> },
}
```

### Streaming to GTK

```rust
// In rline-ui: set up the receiving end
let (gtk_sender, gtk_receiver) = glib::MainContext::channel(glib::Priority::DEFAULT);

gtk_receiver.attach(None, glib::clone!(
    @weak text_view => @default-return glib::ControlFlow::Break,
    move |chunk: Result<CompletionChunk, AiError>| {
        match chunk {
            Ok(chunk) => {
                text_view.insert_completion(&chunk.text);
                if chunk.is_final {
                    return glib::ControlFlow::Break;
                }
            }
            Err(AiError::Cancelled) => return glib::ControlFlow::Break,
            Err(e) => {
                show_error_toast(&e.to_string());
                return glib::ControlFlow::Break;
            }
        }
        glib::ControlFlow::Continue
    }
));
```

## Responsibilities

When implementing AI features:
1. Define provider-agnostic request/response types in `rline-ai`
2. Implement `AiProvider` trait for OpenAI-compatible APIs using `async-openai`
3. Handle streaming with backpressure (bounded channels)
4. Implement cancellation for all in-flight requests
5. Handle rate limiting with exponential backoff
6. Bridge results to GTK main thread via `glib::MainContext::channel()`
7. Store API keys securely (environment variable or system keyring, never hardcoded)
8. Write tests with mock providers (no real network calls in tests)
