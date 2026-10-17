use std::fmt;
use std::ops::Deref;

use super::{Account, Buffer, Meter};
use crate::LimitExceeded;

/// Fallible UTF-8 output whose capacity remains charged to its operation.
pub struct Text {
    bytes: Buffer<u8>,
    account: Account,
    error: Option<LimitExceeded>,
}

impl Text {
    pub fn new(account: &Account) -> Self {
        Self {
            bytes: Buffer::new(),
            account: account.clone(),
            error: None,
        }
    }

    pub fn push(&mut self, meter: &mut Meter<'_>, text: &str) -> Result<(), LimitExceeded> {
        self.bytes.extend_from_slice(meter, text.as_bytes())
    }

    /// The original allocation/fuel error when a formatting sink failed.
    pub fn error(&self) -> Option<LimitExceeded> {
        self.error
    }
}

impl Deref for Text {
    type Target = str;

    fn deref(&self) -> &str {
        // SAFETY: every append copies a complete UTF-8 string.
        unsafe { std::str::from_utf8_unchecked(self.bytes.as_slice()) }
    }
}

impl fmt::Write for Text {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.error.is_some() {
            return Err(fmt::Error);
        }

        match self
            .bytes
            .extend_from_slice(&mut self.account.meter(), text.as_bytes())
        {
            Ok(()) => Ok(()),
            Err(error) => {
                self.error = Some(error);
                Err(fmt::Error)
            }
        }
    }
}

impl fmt::Display for Text {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self)
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        <str as fmt::Debug>::fmt(self, out)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Text {
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(self)
    }
}
