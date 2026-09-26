//! A built-in game table that a data pack may replace for a run. Each crate carrying one of the
//! tables a pack holds (`oracle_protocol::DATA_FILES`) keeps it in a `LazyLock` whose first read
//! [`PackTable::take`]s the pack's copy, if the app put one in place ([`PackTable::set`]), and
//! parses its built-in text otherwise. From that first read on the table is decided for the run:
//! the parsed table is in use, and swapping it would leave borrowed rows pointing at the other.

use std::fmt;
use std::sync::{Mutex, PoisonError};

/// Where a data pack's copy of a table waits for the table's first read.
pub struct PackTable<T>(Mutex<Slot<T>>);

enum Slot<T> {
    Empty,
    Waiting(T),
    /// The table has been read: the pack's copy, or the built-in one.
    Decided,
}

impl<T> PackTable<T> {
    pub const fn new() -> Self {
        PackTable(Mutex::new(Slot::Empty))
    }

    /// Puts a data pack's copy in place of the built-in table. Refused once the table has been
    /// read, or when a copy is already waiting.
    pub fn set(&self, table: T) -> Result<(), TableInUse> {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match *slot {
            Slot::Empty => {
                *slot = Slot::Waiting(table);
                Ok(())
            }
            Slot::Waiting(_) | Slot::Decided => Err(TableInUse),
        }
    }

    /// For the table's first read, which decides it: the pack's copy, if one is waiting.
    pub fn take(&self) -> Option<T> {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match std::mem::replace(&mut *slot, Slot::Decided) {
            Slot::Waiting(table) => Some(table),
            Slot::Empty | Slot::Decided => None,
        }
    }
}

impl<T> Default for PackTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// [`PackTable::set`] came too late: the table has been read, or another copy took its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableInUse;

impl fmt::Display for TableInUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the table was read or replaced already")
    }
}

impl std::error::Error for TableInUse {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_set_before_the_first_read_is_the_one_read() {
        let table = PackTable::new();
        table.set("pack").unwrap();
        assert_eq!(table.take(), Some("pack"));
        // Read and decided: nothing replaces it for the rest of the run.
        assert_eq!(table.set("another pack"), Err(TableInUse));
        assert_eq!(table.take(), None);
    }

    #[test]
    fn a_copy_set_after_the_first_read_is_refused() {
        let table = PackTable::new();
        // The first read found no copy: the built-in table is in use.
        assert_eq!(table.take(), None);
        assert_eq!(table.set("pack"), Err(TableInUse));
        assert_eq!(table.take(), None);
    }

    #[test]
    fn a_second_copy_does_not_replace_the_first() {
        let table = PackTable::new();
        table.set("first").unwrap();
        assert_eq!(table.set("second"), Err(TableInUse));
        assert_eq!(table.take(), Some("first"));
    }
}
