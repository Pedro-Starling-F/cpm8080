use std::ops::IndexMut;
use i8080_core::cpu::CPU;
#[cfg(feature = "log")]
use log::{debug, error};
#[cfg(feature = "std")]
use std::print;
#[derive(Clone, Copy, Debug, Default)]
pub struct CPM(pub u8);
impl CPM {
    pub fn syscall(&self, cpu: &mut CPU, mem: &mut dyn IndexMut<u16, Output=u8>) {
        match self.0 {
            0x09 => self.c_writestr(cpu, mem),
            0x02 => self.c_write(cpu, mem),
            _ => {
                #[cfg(feature = "log")]
                error!(
                    "regs:{:x?}, instr:{:08b}, {:02x}",
                    cpu.get_regs(),
                    mem[cpu.get_regs().pc as usize],
                    mem[cpu.get_regs().pc as usize]
                );
                panic!(
                    "Unimplemented CPM syscall: {:02x}",
                    self.0,
                );
            }
        }
        cpu.ret(mem);
    }
    fn c_writestr(&self, cpu: &mut CPU, mem:&mut dyn IndexMut<u16, Output=u8>) {
        let off = cpu.get_regs().get_rp(0x10);
        let mut c: char = ' ';
        let mut count = 0;
        #[cfg(feature = "log")]
        debug!("\n");
        #[cfg(feature = "std")]
        print!("\n");
        while c != '$' {
            c = mem[off + count] as char;
            #[cfg(feature = "log")]
            debug!("{}", c);
            #[cfg(feature = "std")]
            print!("{}", c);
            count += 1;
        }
        #[cfg(feature = "log")]
        debug!("\n");
        #[cfg(feature = "std")]
        print!("\n");
    }
    fn c_write(&self, cpu: &mut CPU, _mem: &mut dyn IndexMut<u16, Output=u8>) {
        let c = cpu.get_regs().e as char;
        #[cfg(feature = "log")]
        debug!("{}", c);
        #[cfg(feature = "std")]
        print!("{}", c);
    }
}
