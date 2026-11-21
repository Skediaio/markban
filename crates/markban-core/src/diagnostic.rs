//! Diagnostics: the error and warning codes of spec section 6.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Code {
    E001,
    E002,
    E003,
    E004,
    E005,
    E006,
    E007,
    E008,
    E009,
    W001,
    W002,
}

impl Code {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::E001 => "E001",
            Self::E002 => "E002",
            Self::E003 => "E003",
            Self::E004 => "E004",
            Self::E005 => "E005",
            Self::E006 => "E006",
            Self::E007 => "E007",
            Self::E008 => "E008",
            Self::E009 => "E009",
            Self::W001 => "W001",
            Self::W002 => "W002",
        }
    }

    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::W001 | Self::W002 => Severity::Warning,
            _ => Severity::Error,
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Errors make a board invalid; warnings do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One finding about a board, with file (relative to the board directory) and line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub file: String,
    pub line: Option<u32>,
    pub message: String,
}

impl Diagnostic {
    #[must_use]
    pub fn new(
        code: Code,
        file: impl Into<String>,
        line: Option<u32>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: code.severity(),
            file: file.into(),
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    /// `file:line: E007 message`, or `file: E007 message` without a line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}: {} {}", self.file, self.code, self.message),
            None => write!(f, "{}: {} {}", self.file, self.code, self.message),
        }
    }
}
