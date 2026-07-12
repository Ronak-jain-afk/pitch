/// Text insertion trait — platform crate implements this.
pub trait TextInserter: Send {
    fn insert(&self, text: &str, target: InsertTarget) -> Result<(), InsertError>;
    fn insert_with_fallback(&self, text: &str) -> Result<(), InsertError>;
}

#[derive(Debug, Clone, Copy)]
pub enum InsertTarget {
    FocusedWindow,
    Pid(u32),
}

#[derive(Debug, thiserror::Error)]
pub enum InsertError {
    #[error("Failed to type text: {0}")]
    TypeFailed(String),
    #[error("Accessibility permissions not granted")]
    PermissionDenied,
    #[error("Clipboard operation failed")]
    ClipboardFailed,
    #[error("Target not found: PID {0}")]
    TargetNotFound(u32),
    #[error("Empty text")]
    EmptyText,
}
