//! `net.minecraft.util.DelegateDataOutput` — a `DataOutput` that forwards every
//! call to a parent. Used by `NbtIo.StringFallbackDataOutput` to override just
//! the string-write behavior.
//!
//! Only the delegate shape the `writeUTF` override needs is ported. Java's
//! `parent` is `private` and reached only through the delegated methods, so no
//! accessor is ported either.

use crate::data_io::DataOutput;
use std::io;

/// `DelegateDataOutput` — wraps a parent `DataOutput`, delegating all methods.
pub struct DelegateDataOutput<T: DataOutput> {
    parent: T,
}

impl<T: DataOutput> DelegateDataOutput<T> {
    /// `new DelegateDataOutput(DataOutput parent)`.
    pub fn new(parent: T) -> Self {
        DelegateDataOutput { parent }
    }
}

impl<T: DataOutput> DataOutput for DelegateDataOutput<T> {
    fn write(&mut self, b: i32) -> io::Result<()> {
        self.parent.write(b)
    }

    fn write_all(&mut self, b: &[u8]) -> io::Result<()> {
        self.parent.write_all(b)
    }

    fn write_utf(&mut self, s: &str) -> io::Result<()> {
        self.parent.write_utf(s)
    }

    fn write_boolean(&mut self, v: bool) -> io::Result<()> {
        self.parent.write_boolean(v)
    }

    fn write_byte(&mut self, v: i32) -> io::Result<()> {
        self.parent.write_byte(v)
    }

    fn write_short(&mut self, v: i16) -> io::Result<()> {
        self.parent.write_short(v)
    }

    fn write_int(&mut self, v: i32) -> io::Result<()> {
        self.parent.write_int(v)
    }

    fn write_long(&mut self, v: i64) -> io::Result<()> {
        self.parent.write_long(v)
    }

    fn write_float(&mut self, v: f32) -> io::Result<()> {
        self.parent.write_float(v)
    }

    fn write_double(&mut self, v: f64) -> io::Result<()> {
        self.parent.write_double(v)
    }
}
