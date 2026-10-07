pub use cpm8080_core::cpm::CPM;
pub use i8080_core::cpu::CPU;
pub mod sys;
pub use sys::{Sys, INITIAL_SP};
#[cfg(feature = "webp")]
pub mod webp {
    extern crate wee_alloc;
    #[global_allocator]
    static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;
    extern crate js_sys;
    extern crate wasm_bindgen;
    extern crate wasm_bindgen_futures;
    extern crate web_sys;
    use crate::{CPM, CPU};
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::Clamped;
    use wasm_bindgen::JsCast;
    use web_sys::ImageData;
    pub fn window() -> web_sys::Window {
        web_sys::window().expect("no global `window` exists")
    }

    pub fn request_animation_frame(f: &Closure<dyn FnMut()>) {
        window()
            .request_animation_frame(f.as_ref().unchecked_ref())
            .expect("should register `requestAnimationFrame` OK");
    }

    #[wasm_bindgen]
    pub fn run(x: Box<[u8]>) -> Result<(), JsValue> {
        let mut cpu = CPU::new();
        let mut memory = &mut x;
        Ok(())
    }
}
