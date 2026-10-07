mod console;
mod files;
pub use console::Console;
use files::{Name, RECORD};
use i8080_core::cpu::CPU;
use std::fs::{self, File};
use std::ops::IndexMut;
use std::path::PathBuf;

type Mem = dyn IndexMut<u16, Output = u8>;

/// BDOS entry point; the word at 0x0006 also tells programs where the TPA ends.
pub const BDOS_BASE: u16 = 0xFD00;
// Fake disk parameter block and allocation vector, returned by functions 31 and 27.
const DPB_ADDR: u16 = BDOS_BASE + 0x10;
const ALV_ADDR: u16 = BDOS_BASE + 0x80;
// 2 MB disk: 2K blocks (BSH 4, BLM 15, EXM 0), DSM 1023, DRM 511, 8 directory blocks.
const DPB: [u8; 15] = [64, 0, 4, 15, 0, 0xFF, 0x03, 0xFF, 0x01, 0xFF, 0x00, 0, 0, 0, 0];
const BLOCK: u64 = 2048;
const BLOCKS: usize = 1024;
const DIR_BLOCKS: usize = 8;

// FCB offsets
const FCB_EX: u16 = 12;
const FCB_S2: u16 = 14;
const FCB_RC: u16 = 15;
const FCB_CR: u16 = 32;
const FCB_R0: u16 = 33;

/// High level emulation of the CP/M 2.2 BDOS. Drive A: is the host directory `root`.
pub struct CPM {
    pub console: Console,
    root: PathBuf,
    dma: u16,
    drive: u8,
    user: u8,
    search: Vec<(Name, PathBuf)>,
}

impl CPM {
    pub fn new(root: PathBuf) -> CPM {
        CPM {
            console: Console::new(),
            root,
            dma: 0x0080,
            drive: 0,
            user: 0,
            search: Vec::new(),
        }
    }
    /// Runs the BDOS function in C and returns to the caller. Returns false
    /// when the program asked for a warm boot, or waits for console input
    /// after stdin has closed.
    pub fn syscall(&mut self, cpu: &mut CPU, mem: &mut Mem) -> bool {
        let regs = cpu.get_regs();
        let de = (regs.d as u16) << 8 | regs.e as u16;
        let e = regs.e;
        let ret: u16 = match regs.c {
            // P_TERMCPM
            0x00 => return false,
            // C_READ
            0x01 => {
                let c = match self.console.read() {
                    Some(c) => c,
                    None => return false,
                };
                self.echo(c);
                c as u16
            }
            // C_WRITE
            0x02 => {
                self.console.write(e);
                0
            }
            // A_READ: reader always at end of file
            0x03 => 0x1A,
            // A_WRITE, L_WRITE
            0x04 | 0x05 => 0,
            // C_RAWIO
            0x06 => match e {
                0xFF if !self.console.status() => 0,
                0xFF | 0xFD => match self.console.read() {
                    Some(c) => c as u16,
                    None => return false,
                },
                0xFE => self.console_status(),
                _ => {
                    self.console.write(e);
                    0
                }
            },
            // A_GETIOBYTE, A_SETIOBYTE
            0x07 => mem[0x0003] as u16,
            0x08 => {
                mem[0x0003] = e;
                0
            }
            // C_WRITESTR
            0x09 => {
                self.write_string(mem, de);
                0
            }
            // C_READSTR
            0x0A => {
                if !self.read_line(mem, de) {
                    return false;
                }
                0
            }
            // C_STAT
            0x0B => self.console_status(),
            // S_BDOSVER: CP/M 2.2
            0x0C => 0x0022,
            // DRV_ALLRESET
            0x0D => {
                self.dma = 0x0080;
                self.set_drive(mem, 0);
                0
            }
            // DRV_SET
            0x0E => {
                self.set_drive(mem, e & 0x0F);
                0
            }
            0x0F => self.f_open(mem, de),
            0x10 => self.f_close(mem, de),
            0x11 => self.f_sfirst(mem, de),
            0x12 => self.f_snext(mem),
            0x13 => self.f_delete(mem, de),
            0x14 => self.f_read(mem, de),
            0x15 => self.f_write(mem, de),
            0x16 => self.f_make(mem, de),
            0x17 => self.f_rename(mem, de),
            // DRV_LOGINVEC: only A: is logged in
            0x18 => 0x0001,
            // DRV_GET
            0x19 => self.drive as u16,
            // F_DMAOFF
            0x1A => {
                self.dma = de;
                0
            }
            // DRV_ALLOCVEC
            0x1B => {
                self.write_alv(mem);
                ALV_ADDR
            }
            // DRV_SETRO, DRV_ROVEC: no drive is read-only
            0x1C | 0x1D => 0,
            // F_ATTRIB: attributes are ignored
            0x1E => self.f_attrib(mem, de),
            // DRV_DPB
            0x1F => {
                for (i, b) in DPB.iter().enumerate() {
                    mem[DPB_ADDR + i as u16] = *b;
                }
                DPB_ADDR
            }
            // F_USERNUM
            0x20 if e == 0xFF => self.user as u16,
            0x20 => {
                self.user = e & 0x0F;
                self.set_drive(mem, self.drive);
                0
            }
            0x21 => self.f_readrand(mem, de),
            // F_WRITERAND, F_WRITEZF
            0x22 | 0x28 => self.f_writerand(mem, de),
            0x23 => self.f_size(mem, de),
            // F_RANDREC
            0x24 => {
                let rec = seq_record(mem, de);
                set_random(mem, de, rec);
                0
            }
            // DRV_RESET
            0x25 => 0,
            f => panic!("Unimplemented CPM syscall: {:02x}", f),
        };
        // Results come back in both A/B and L/H.
        cpu.regs.a = ret as u8;
        cpu.regs.l = ret as u8;
        cpu.regs.b = (ret >> 8) as u8;
        cpu.regs.h = (ret >> 8) as u8;
        cpu.ret(mem);
        true
    }

    fn console_status(&mut self) -> u16 {
        if self.console.status() {
            0xFF
        } else {
            0
        }
    }
    /// Echoes a typed character, showing control characters as ^X.
    fn echo(&mut self, c: u8) {
        if c < 0x20 && !matches!(c, b'\r' | b'\n' | b'\t' | 0x08) {
            self.console.write(b'^');
            self.console.write(c + 0x40);
        } else {
            self.console.write(c);
        }
    }
    fn write_string(&mut self, mem: &mut Mem, addr: u16) {
        let mut addr = addr;
        loop {
            let c = mem[addr];
            if c == b'$' {
                break;
            }
            self.console.write(c);
            addr = addr.wrapping_add(1);
        }
    }
    /// Buffered line input with backspace, ^U/^X to erase the line, and ^C at
    /// the start of the line to warm boot. Returns false on ^C or end of input.
    fn read_line(&mut self, mem: &mut Mem, buf: u16) -> bool {
        let max = mem[buf] as usize;
        let mut line: Vec<u8> = Vec::new();
        loop {
            let c = match self.console.read() {
                Some(c) => c,
                None => return false,
            };
            match c {
                b'\r' | b'\n' => break,
                0x03 if line.is_empty() => {
                    self.echo(c);
                    self.console.write(b'\r');
                    self.console.write(b'\n');
                    return false;
                }
                0x08 | 0x7F => {
                    if let Some(prev) = line.pop() {
                        self.erase(prev);
                    }
                }
                0x15 | 0x18 => {
                    while let Some(prev) = line.pop() {
                        self.erase(prev);
                    }
                }
                _ if line.len() < max => {
                    line.push(c);
                    self.echo(c);
                }
                _ => {}
            }
        }
        for (i, c) in line.iter().enumerate() {
            mem[buf.wrapping_add(2 + i as u16)] = *c;
        }
        mem[buf.wrapping_add(1)] = line.len() as u8;
        self.console.write(b'\r');
        true
    }
    fn erase(&mut self, c: u8) {
        let width = if c < 0x20 { 2 } else { 1 };
        for _ in 0..width {
            for b in [0x08, b' ', 0x08] {
                self.console.write(b);
            }
        }
    }
    fn set_drive(&mut self, mem: &mut Mem, drive: u8) {
        self.drive = drive;
        mem[0x0004] = (self.user << 4) | drive;
    }

    /// The host file an FCB refers to, if its drive is A: and the file exists.
    fn fcb_path(&self, mem: &mut Mem, fcb: u16) -> Option<PathBuf> {
        if !self.fcb_on_a(mem, fcb) {
            return None;
        }
        files::find(&self.root, &fcb_name(mem, fcb, 1))
    }
    fn fcb_on_a(&self, mem: &mut Mem, fcb: u16) -> bool {
        let dr = mem[fcb] & 0x1F;
        let drive = if dr == 0 { self.drive } else { dr - 1 };
        drive == 0
    }
    /// Sets RC to the number of records in the extent holding the current
    /// sequential position.
    fn update_rc(&self, mem: &mut Mem, fcb: u16, path: &PathBuf) {
        let extent_start = seq_record(mem, fcb) & !0x7F;
        let rc = files::records(path).saturating_sub(extent_start).min(128);
        mem[fcb + FCB_RC] = rc as u8;
    }
    fn dma_to_buf(&self, mem: &mut Mem) -> [u8; RECORD] {
        let mut buf = [0u8; RECORD];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = mem[self.dma.wrapping_add(i as u16)];
        }
        buf
    }
    fn buf_to_dma(&self, mem: &mut Mem, buf: &[u8; RECORD]) {
        for (i, b) in buf.iter().enumerate() {
            mem[self.dma.wrapping_add(i as u16)] = *b;
        }
    }

    fn f_open(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        if !self.fcb_on_a(mem, fcb) {
            return 0xFF;
        }
        match files::search(&self.root, &fcb_name(mem, fcb, 1)).into_iter().next() {
            Some((name, path)) => {
                // With wildcards, the FCB gets the name that was found.
                for (i, b) in name.iter().enumerate() {
                    mem[fcb + 1 + i as u16] = *b;
                }
                self.update_rc(mem, fcb, &path);
                0
            }
            None => 0xFF,
        }
    }
    fn f_close(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        match self.fcb_path(mem, fcb) {
            Some(_) => 0,
            None => 0xFF,
        }
    }
    fn f_sfirst(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        // A '?' drive byte matches every directory entry.
        let pattern = if mem[fcb] == b'?' {
            [b'?'; 11]
        } else if self.fcb_on_a(mem, fcb) {
            fcb_name(mem, fcb, 1)
        } else {
            return 0xFF;
        };
        self.search = files::search(&self.root, &pattern);
        self.search.reverse();
        self.f_snext(mem)
    }
    /// Puts the next match in the first directory entry slot of the DMA buffer.
    fn f_snext(&mut self, mem: &mut Mem) -> u16 {
        let (name, path) = match self.search.pop() {
            Some(found) => found,
            None => return 0xFF,
        };
        let mut entry = [0u8; 32];
        entry[0] = self.user;
        entry[1..12].copy_from_slice(&name);
        entry[15] = files::records(&path).min(128) as u8;
        for (i, b) in entry.iter().enumerate() {
            mem[self.dma.wrapping_add(i as u16)] = *b;
        }
        0
    }
    fn f_delete(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        if !self.fcb_on_a(mem, fcb) {
            return 0xFF;
        }
        let found = files::search(&self.root, &fcb_name(mem, fcb, 1));
        let mut deleted = false;
        for (_, path) in found {
            deleted |= fs::remove_file(path).is_ok();
        }
        if deleted {
            0
        } else {
            0xFF
        }
    }
    fn f_read(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        let path = match self.fcb_path(mem, fcb) {
            Some(path) => path,
            None => return 1,
        };
        let rec = seq_record(mem, fcb);
        let mut buf = [0u8; RECORD];
        match files::read_record(&path, rec, &mut buf) {
            Ok(true) => {
                self.buf_to_dma(mem, &buf);
                set_seq_record(mem, fcb, rec + 1);
                self.update_rc(mem, fcb, &path);
                0
            }
            _ => 1,
        }
    }
    fn f_write(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        let path = match self.fcb_path(mem, fcb) {
            Some(path) => path,
            None => return 1,
        };
        let rec = seq_record(mem, fcb);
        let buf = self.dma_to_buf(mem);
        match files::write_record(&path, rec, &buf) {
            Ok(()) => {
                set_seq_record(mem, fcb, rec + 1);
                self.update_rc(mem, fcb, &path);
                0
            }
            Err(_) => 2,
        }
    }
    fn f_make(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        let name = fcb_name(mem, fcb, 1);
        if !self.fcb_on_a(mem, fcb) || name.contains(&b'?') {
            return 0xFF;
        }
        // Reuse an existing host file whose name only differs in case.
        let path = files::find(&self.root, &name)
            .unwrap_or_else(|| self.root.join(files::cpm_to_host(&name)));
        match File::create(&path) {
            Ok(_) => {
                mem[fcb + FCB_RC] = 0;
                0
            }
            Err(_) => 0xFF,
        }
    }
    fn f_rename(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        let new = fcb_name(mem, fcb, 17);
        let path = match self.fcb_path(mem, fcb) {
            Some(path) => path,
            None => return 0xFF,
        };
        match fs::rename(path, self.root.join(files::cpm_to_host(&new))) {
            Ok(()) => 0,
            Err(_) => 0xFF,
        }
    }
    fn f_attrib(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        match self.fcb_path(mem, fcb) {
            Some(_) => 0,
            None => 0xFF,
        }
    }
    fn f_readrand(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        let rec = match random_record(mem, fcb) {
            Some(rec) => rec,
            None => return 6,
        };
        let path = match self.fcb_path(mem, fcb) {
            Some(path) => path,
            None => return 1,
        };
        let mut buf = [0u8; RECORD];
        match files::read_record(&path, rec, &mut buf) {
            Ok(true) => {
                self.buf_to_dma(mem, &buf);
                // The next sequential read re-reads this record, as in CP/M.
                set_seq_record(mem, fcb, rec);
                self.update_rc(mem, fcb, &path);
                0
            }
            _ => 1,
        }
    }
    fn f_writerand(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        let rec = match random_record(mem, fcb) {
            Some(rec) => rec,
            None => return 6,
        };
        let path = match self.fcb_path(mem, fcb) {
            Some(path) => path,
            None => return 5,
        };
        let buf = self.dma_to_buf(mem);
        match files::write_record(&path, rec, &buf) {
            Ok(()) => {
                set_seq_record(mem, fcb, rec);
                self.update_rc(mem, fcb, &path);
                0
            }
            Err(_) => 2,
        }
    }
    fn f_size(&mut self, mem: &mut Mem, fcb: u16) -> u16 {
        match self.fcb_path(mem, fcb) {
            Some(path) => {
                set_random(mem, fcb, files::records(&path));
                0
            }
            None => {
                set_random(mem, fcb, 0);
                0xFF
            }
        }
    }
    /// Builds an allocation vector from the sizes of the files on A:.
    fn write_alv(&mut self, mem: &mut Mem) {
        let used: usize = files::search(&self.root, &[b'?'; 11])
            .iter()
            .map(|(_, path)| {
                let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                len.div_ceil(BLOCK) as usize
            })
            .sum();
        let used = (DIR_BLOCKS + used).min(BLOCKS);
        for byte in 0..BLOCKS / 8 {
            let mut bits = 0u8;
            for bit in 0..8 {
                if byte * 8 + bit < used {
                    bits |= 0x80 >> bit;
                }
            }
            mem[ALV_ADDR + byte as u16] = bits;
        }
    }
}

/// Reads the 11-byte name at `fcb + offset`, dropping attribute bits.
fn fcb_name(mem: &mut Mem, fcb: u16, offset: u16) -> Name {
    let mut name = [b' '; 11];
    for (i, b) in name.iter_mut().enumerate() {
        *b = mem[fcb.wrapping_add(offset + i as u16)] & 0x7F;
    }
    name
}
/// The sequential record number from S2, EX and CR.
fn seq_record(mem: &mut Mem, fcb: u16) -> u32 {
    ((mem[fcb + FCB_S2] & 0x3F) as u32) << 12
        | ((mem[fcb + FCB_EX] & 0x1F) as u32) << 7
        | (mem[fcb + FCB_CR] & 0x7F) as u32
}
fn set_seq_record(mem: &mut Mem, fcb: u16, rec: u32) {
    mem[fcb + FCB_CR] = (rec & 0x7F) as u8;
    mem[fcb + FCB_EX] = ((rec >> 7) & 0x1F) as u8;
    mem[fcb + FCB_S2] = ((rec >> 12) & 0x3F) as u8;
}
/// The random record number from R0 and R1, or None if R2 is set (past the
/// end of the disk).
fn random_record(mem: &mut Mem, fcb: u16) -> Option<u32> {
    if mem[fcb + FCB_R0 + 2] != 0 {
        return None;
    }
    Some(mem[fcb + FCB_R0] as u32 | (mem[fcb + FCB_R0 + 1] as u32) << 8)
}
fn set_random(mem: &mut Mem, fcb: u16, rec: u32) {
    mem[fcb + FCB_R0] = rec as u8;
    mem[fcb + FCB_R0 + 1] = (rec >> 8) as u8;
    mem[fcb + FCB_R0 + 2] = (rec >> 16) as u8;
}
