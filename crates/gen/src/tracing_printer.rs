//! Route `tracing` output through `rustyline::ExternalPrinter` so async log
//! lines (Bevy, tokio, our own warnings) never clobber the interactive
//! `You:` prompt mid-typing.
//!
//! - When readline is **not** active, the printer writes directly to the tty.
//! - When readline **is** active (raw mode), rustyline atomically clears the
//!   prompt line, prints the message, then redraws the prompt with whatever
//!   the user had typed.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use rustyline::ExternalPrinter;
use tracing_subscriber::fmt::MakeWriter;

type BoxedPrinter = Box<dyn ExternalPrinter + Send>;

/// Shared handle used both by tracing and (indirectly) by the REPL editor.
#[derive(Clone)]
pub struct SharedPrinter(Arc<Mutex<BoxedPrinter>>);

impl SharedPrinter {
    pub fn new<P>(printer: P) -> Self
    where
        P: ExternalPrinter + Send + 'static,
    {
        Self(Arc::new(Mutex::new(Box::new(printer))))
    }
}

/// Per-event writer. Buffers bytes until drop, then sends the whole log line
/// in a single `print` call — otherwise tracing's fmt layer would split one
/// event across several prompt-redraws.
pub struct PrinterWriter {
    printer: Arc<Mutex<BoxedPrinter>>,
    buf: Vec<u8>,
}

impl Write for PrinterWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for PrinterWriter {
    fn drop(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let msg = String::from_utf8_lossy(&self.buf).into_owned();
        if let Ok(mut printer) = self.printer.lock() {
            let _ = printer.print(msg);
        }
    }
}

impl<'a> MakeWriter<'a> for SharedPrinter {
    type Writer = PrinterWriter;

    fn make_writer(&'a self) -> Self::Writer {
        PrinterWriter {
            printer: self.0.clone(),
            buf: Vec::with_capacity(128),
        }
    }
}
