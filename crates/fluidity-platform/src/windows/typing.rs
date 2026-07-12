use fluidity_core::{InsertError, InsertTarget, TextInserter};

/// Windows text inserter using SendInput + UI Automation + Clipboard fallback.
/// Stub for Phase 1 — will be implemented in Phase 2.
pub struct WindowsTextInserter;

impl TextInserter for WindowsTextInserter {
    fn insert(&self, _text: &str, _target: InsertTarget) -> Result<(), InsertError> {
        Err(InsertError::TypeFailed("Not implemented yet".into()))
    }

    fn insert_with_fallback(&self, _text: &str) -> Result<(), InsertError> {
        Err(InsertError::TypeFailed("Not implemented yet".into()))
    }
}
