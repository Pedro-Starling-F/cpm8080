use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;

/// Console shared by the BIOS and BDOS. Stdin is read on a background thread
/// so the status calls can report whether a key is waiting without blocking.
pub struct Console {
    input: Receiver<u8>,
    pending: Option<u8>,
    eof: bool,
}

impl Console {
    pub fn new() -> Console {
        let (tx, rx) = channel();
        thread::spawn(move || {
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 1];
            while let Ok(1) = stdin.read(&mut buf) {
                if tx.send(buf[0]).is_err() {
                    break;
                }
            }
        });
        Console {
            input: rx,
            pending: None,
            eof: false,
        }
    }
    /// True when a character is waiting. At end of input this also reports
    /// true, so a program polling for a key goes on to read and ends the session.
    pub fn status(&mut self) -> bool {
        if self.pending.is_some() || self.eof {
            return true;
        }
        match self.input.try_recv() {
            Ok(c) => {
                self.pending = Some(c);
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.eof = true;
                true
            }
        }
    }
    /// Blocking read. CP/M expects CR as the line terminator. Returns None once
    /// input has run out, since nothing will ever answer the program.
    pub fn read(&mut self) -> Option<u8> {
        let _ = std::io::stdout().flush();
        let c = match self.pending.take() {
            Some(c) => Some(c),
            None if self.eof => None,
            None => self.input.recv().ok(),
        };
        match c {
            Some(b'\n') => Some(b'\r'),
            Some(c) => Some(c),
            None => {
                self.eof = true;
                None
            }
        }
    }
    pub fn write(&mut self, c: u8) {
        let mut out = std::io::stdout();
        let _ = out.write_all(&[c]);
        let _ = out.flush();
    }
}

impl Default for Console {
    fn default() -> Console {
        Console::new()
    }
}
