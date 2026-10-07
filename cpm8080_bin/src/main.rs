use cpm8080_lib::*;
use std::env;
use std::fs;
use std::io::IsTerminal;
use std::panic;
use std::process::{Command, Stdio};

/// Puts the terminal in raw mode so CP/M programs get every key unbuffered and
/// unechoed, and restores it on drop or panic.
struct RawTerminal(Option<String>);

impl RawTerminal {
    fn enable() -> RawTerminal {
        if !std::io::stdin().is_terminal() {
            return RawTerminal(None);
        }
        let saved = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
        if let Some(settings) = &saved {
            stty(&["raw", "-echo"]);
            let settings = settings.clone();
            let hook = panic::take_hook();
            panic::set_hook(Box::new(move |info| {
                stty(&[&settings]);
                hook(info);
            }));
        }
        RawTerminal(saved)
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        if let Some(settings) = &self.0 {
            stty(&[settings]);
        }
    }
}

fn stty(args: &[&str]) {
    let _ = Command::new("stty").args(args).stdin(Stdio::inherit()).status();
}

fn main() {
    #[cfg(feature = "log")]
    simple_logger::init_with_level(log::Level::Trace).unwrap();
    let args: Vec<String> = env::args().collect();
    let file = fs::read(args[1].clone()).unwrap();
    let mut sys = Sys::new(&file);
    sys.set_command_line(&args[2..]);
    let mut os = CPM::new(env::current_dir().unwrap());
    let mut cpu = CPU::new(Some(0x0100), Some(INITIAL_SP));
    let _terminal = RawTerminal::enable();
    while sys.run_instruction(&mut cpu, &mut os) {}
}
