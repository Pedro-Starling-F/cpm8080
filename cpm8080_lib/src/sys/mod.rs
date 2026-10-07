use core::ops::{Index, IndexMut};
use std::io::{Read, Write};
use crate::{CPM, CPU};

// BDOS entry point; the word at 0x0006 also tells programs where the TPA ends.
const BDOS_BASE: u16 = 0xFD00;
// BIOS jump table; the word at 0x0001 points at its WBOOT entry.
const BIOS_BASE: u16 = 0xFE00;
const BIOS_ENTRIES: u16 = 17;
const BIOS_END: u16 = BIOS_BASE + BIOS_ENTRIES * 3;

pub struct Sys {
    os: CPM,
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
        mem_arr[0x100..0x100 + com_file_len].copy_from_slice(&com_file[0..com_file_len]);
        Sys {
            os: CPM(0),
            mem: mem_arr,
        }
    }
    /// Runs one instruction, servicing BDOS/BIOS calls. Returns false once the
    /// program has exited through a warm boot.
    pub fn run_instruction(&mut self, cpu:&mut CPU, os:&mut CPM) -> bool {
        cpu.next(self);
        let pc = cpu.get_regs().pc;
        if pc == 0x0005 || pc == BDOS_BASE {
            let c_reg = cpu.get_regs().c;
            // P_TERMCPM is a warm boot
            if c_reg == 0x00 {
                return self.bios_call(cpu, 0x01);
            }
            os.0 = c_reg;
            os.syscall(cpu, self);
        } else if (BIOS_BASE..BIOS_END).contains(&pc) && (pc - BIOS_BASE) % 3 == 0 {
            return self.bios_call(cpu, ((pc - BIOS_BASE) / 3) as u8);
        }
        true
    }
    fn bios_call(&mut self, cpu: &mut CPU, func: u8) -> bool {
        match func {
            // BOOT, WBOOT: the program is done
            0x00 | 0x01 => {
                let _ = std::io::stdout().flush();
                return false;
            }
            // CONST: report no character pending
            0x02 => cpu.regs.a = 0x00,
            // CONIN: blocking read, CP/M expects CR as the line terminator
            0x03 => {
                let _ = std::io::stdout().flush();
                let mut buf = [0u8; 1];
                cpu.regs.a = match std::io::stdin().read(&mut buf) {
                    Ok(1) if buf[0] == b'\n' => b'\r',
                    Ok(1) => buf[0],
                    _ => 0x1A,
                };
            }
            // CONOUT
            0x04 => {
                let mut out = std::io::stdout();
                let _ = out.write_all(&[cpu.regs.c]);
                let _ = out.flush();
            }
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
