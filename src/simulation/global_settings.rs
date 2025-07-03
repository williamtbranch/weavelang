use once_cell::sync::OnceCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForceLevel {
    Advanced,
    // We can add other levels here later if needed
    // Simple,
    // English,
}

pub static FORCE_LEVEL_OVERRIDE: OnceCell<ForceLevel> = OnceCell::new();