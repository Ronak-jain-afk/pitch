use fluidity_core::{InsertError, InsertTarget, TextInserter};

pub struct WindowsTextInserter;

impl TextInserter for WindowsTextInserter {
    fn insert(&self, text: &str, _target: InsertTarget) -> Result<(), InsertError> {
        if text.is_empty() {
            return Err(InsertError::EmptyText);
        }
        enigo_type(text)
    }

    fn insert_with_fallback(&self, text: &str) -> Result<(), InsertError> {
        if text.is_empty() {
            return Err(InsertError::EmptyText);
        }
        enigo_type(text)
    }
}

fn enigo_type(text: &str) -> Result<(), InsertError> {
    use enigo::{Enigo, Settings};
    let mut e = Enigo::new(&Settings::default())
        .map_err(|e| InsertError::TypeFailed(format!("enigo init: {e}")))?;
    e.text(text)
        .map_err(|e| InsertError::TypeFailed(format!("enigo: {e}")))
}
