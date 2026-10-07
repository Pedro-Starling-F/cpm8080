use cpm8080_lib::*;
use std::env;
use std::fs;

fn main() {
    #[cfg(feature = "log")]
    simple_logger::init_with_level(log::Level::Trace).unwrap();
    let args: Vec<String> = env::args().collect();
    let file = fs::read(args[1].clone()).unwrap();
    let mut sys = Sys::new(&file);
    let mut os = CPM(0);
    let mut cpu = CPU::new(Some(0x0100), Some(0xFFFF));
    while sys.run_instruction(&mut cpu, &mut os) {}
}
