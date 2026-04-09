//! JSON Schema definitions for all built-in tools.
//!
//! These are the parameter schemas sent to the AI model as part of the
//! tool definitions in the chat completions request.

/// Helper to build a JSON Schema object for tool parameters.
///
/// Properties are passed as `serde_json::Value` items, not raw `{ ... }`.
macro_rules! schema {
    (
        required: [$($req:expr),* $(,)?],
        properties: { $($name:expr => $prop:expr),* $(,)? }
    ) => {{
        let mut props = serde_json::Map::new();
        $(
            props.insert($name.to_string(), $prop);
        )*
        serde_json::json!({
            "type": "object",
            "properties": props,
            "required": [$($req),*]
        })
    }};
}

pub(crate) use schema;
