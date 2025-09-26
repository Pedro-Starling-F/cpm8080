use core::ops::{Index, IndexMut};
use crate::{CPM, CPU};
pub struct Sys {
    os: CPM,
    mem: [u8; 0x10000],
}
impl Sys {
    pub fn new(com_file: &[u8]) -> Sys {
        let com_file_len = com_file.len();
        let mut mem_arr = [0xfd; 0x10000];
        mem_arr[5] = 8;
        mem_arr[0x100..0x100 + com_file_len].copy_from_slice(&com_file[0..com_file_len]);
        Sys {
            os: CPM(0),
            mem: mem_arr,
        }
    }
    pub fn run_instruction(&mut self, cpu:&mut CPU, os:&mut CPM) {
        cpu.next(self);
        if cpu.get_regs().pc == 0x0005  {
            let c_reg = cpu.get_regs().c;
            os.0 = c_reg;
            os.syscall(cpu, self);
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