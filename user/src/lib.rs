#![no_std]
#![feature(global_asm)]

pub mod print;
pub mod syscall;
mod syscall_internal;

use core::panic::PanicInfo;

global_asm!(include_str!("usys.S"));

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
extern "C" fn abort() -> ! {
	loop {
	}
}