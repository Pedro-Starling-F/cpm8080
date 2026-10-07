use core::ops::{Index, IndexMut};
use std::io::Write;
use cpm8080_core::cpm::BDOS_BASE;
use crate::{CPM, CPU};

// BIOS jump table; the word at 0x0001 points at its WBOOT entry.
const BIOS_BASE: u16 = 0xFE00;
const BIOS_ENTRIES: u16 = 17;
const BIOS_END: u16 = BIOS_BASE + BIOS_ENTRIES * 3;
/// Initial stack pointer. The word at the top of the stack is 0x0000, so a
/// program that exits with RET lands on the warm boot vector.
pub const INITIAL_SP: u16 = 0xFFFE;

pub struct Sys {
    mem: [u8; 0x10000],
}
impl Sys {
    pub fn new(com_file: &[u8]) -> Sys {
        let com_file_len = com_file.len();
        let mut mem_arr = [0xfd; 0x10000];
        // 0x0000: JMP WBOOT
        mem_arr[0x0000] = 0xC3;
        mem_arr[0x0001] = (BIOS_BASE + 3) as u8;
        mem_arr[0x0002] = ((BIOS_BASE + 3) >> 8) as u8;
        // 0x0003: IOBYTE, 0x0004: current drive/user
        mem_arr[0x0003] = 0x00;
        mem_arr[0x0004] = 0x00;
        // 0x0005: JMP BDOS
        mem_arr[0x0005] = 0xC3;
        mem_arr[0x0006] = BDOS_BASE as u8;
        mem_arr[0x0007] = (BDOS_BASE >> 8) as u8;
        mem_arr[BDOS_BASE as usize] = 0xC9;
        // 0x005C/0x006C: default FCBs (current drive, blank name), 0x0080: empty command tail
        mem_arr[0x005C..0x0100].fill(0x00);
        mem_arr[0x005D..0x0068].fill(b' ');
        mem_arr[0x006D..0x0078].fill(b' ');
        // Each BIOS entry jumps to itself, so programs that read the JMP target
        // to call the BIOS directly still land on a trapped address.
        for i in 0..BIOS_ENTRIES {
            let entry = BIOS_BASE + i * 3;
            mem_arr[entry as usize] = 0xC3;
            mem_arr[entry as usize + 1] = entry as u8;
            mem_arr[entry as usize + 2] = (entry >> 8) as u8;
        }
        mem_arr[INITIAL_SP as usize] = 0x00;
        mem_arr[INITIAL_SP as usize + 1] = 0x00;
        mem_arr[0x100..0x100 + com_file_len].copy_from_slice(&com_file[0..com_file_len]);
        Sys {
            mem: mem_arr,
        }
    }
    /// Sets up the command tail at 0x0080 and parses the first two arguments
    /// into the default FCBs at 0x005C and 0x006C, as the CCP would.
    pub fn set_command_line(&mut self, args: &[String]) {
        let tail: String = args.iter().map(|a| format!(" {}", a)).collect();
        let tail = tail.to_ascii_uppercase();
        let len = tail.len().min(126);
        self.mem[0x0080] = len as u8;
        self.mem[0x0081..0x0081 + len].copy_from_slice(&tail.as_bytes()[..len]);
        self.mem[0x0081 + len] = 0;
        for (arg, fcb) in args.iter().zip([0x005C, 0x006C]) {
            parse_fcb(&mut self.mem[fcb..fcb + 12], arg);
        }
    }
    /// Runs one instruction, servicing BDOS/BIOS calls. Returns false once the
    /// program has exited through a warm boot.
    pub fn run_instruction(&mut self, cpu:&mut CPU, os:&mut CPM) -> bool {
        cpu.next(self);
        let pc = cpu.get_regs().pc;
        if pc == 0x0005 || pc == BDOS_BASE {
            return os.syscall(cpu, self);
        } else if (BIOS_BASE..BIOS_END).contains(&pc) && (pc - BIOS_BASE) % 3 == 0 {
            return self.bios_call(cpu, os, ((pc - BIOS_BASE) / 3) as u8);
        }
        true
    }
    fn bios_call(&mut self, cpu: &mut CPU, os: &mut CPM, func: u8) -> bool {
        match func {
            // BOOT, WBOOT: the program is done
            0x00 | 0x01 => {
                let _ = std::io::stdout().flush();
                return false;
            }
            // CONST
            0x02 => cpu.regs.a = if os.console.status() { 0xFF } else { 0x00 },
            // CONIN: end the session once input has run out
            0x03 => match os.console.read() {
                Some(c) => cpu.regs.a = c,
                None => return false,
            },
            // CONOUT
            0x04 => os.console.write(cpu.regs.c),
            // LIST, PUNCH
            0x05 | 0x06 => {}
            // READER: always at end of file
            0x07 => cpu.regs.a = 0x1A,
            // HOME, SETTRK, SETSEC, SETDMA
            0x08 | 0x0A | 0x0B | 0x0C => {}
            // SELDSK: no disks, HL = 0
            0x09 => {
                cpu.regs.h = 0;
                cpu.regs.l = 0;
            }
            // READ, WRITE: report an error
            0x0D | 0x0E => cpu.regs.a = 0x01,
            // LISTST: printer always ready
            0x0F => cpu.regs.a = 0xFF,
            // SECTRAN: no translation, HL = BC
            0x10 => {
                cpu.regs.h = cpu.regs.b;
                cpu.regs.l = cpu.regs.c;
            }
            _ => unreachable!(),
        }
        cpu.ret(self);
        true
    }
}

/// Fills the drive, name and type of an FCB from a command line argument like
/// `B:NAME.TXT`, turning `*` into `?` wildcards.
fn parse_fcb(fcb: &mut [u8], arg: &str) {
    let arg = arg.to_ascii_uppercase();
    let arg = arg.as_bytes();
    let (drive, rest) = match arg {
        [d, b':', rest @ ..] if d.is_ascii_uppercase() && *d <= b'P' => (d - b'A' + 1, rest),
        _ => (0, arg),
    };
    let (name, ext) = match rest.iter().position(|&c| c == b'.') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &[][..]),
    };
    fcb[0] = drive;
    fcb[1..12].fill(b' ');
    fill_field(&mut fcb[1..9], name);
    fill_field(&mut fcb[9..12], ext);
}

fn fill_field(field: &mut [u8], src: &[u8]) {
    for i in 0..field.len() {
        match src.get(i) {
            Some(b'*') => {
                field[i..].fill(b'?');
                return;
            }
            Some(&c) => field[i] = c,
            None => return,
        }
    }
}

impl Index<u16> for Sys{
    type Output = u8;
    fn index(&self, index:u16) -> &Self::Output {
        match index {
            0x0000..=0xFFFF => &self.mem[index as usize],
            _ => unreachable!(),
        }

    }
}

impl IndexMut<u16> for Sys{
    fn index_mut(&mut self, index:u16) -> &mut Self::Output{
        match index {
            0x0000..=0xFFFF => &mut self.mem[index as usize],
            _ => unreachable!(),
        }
    }
}
