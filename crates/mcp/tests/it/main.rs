// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md).
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod automation_roots;
