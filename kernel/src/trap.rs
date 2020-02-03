// Copyright (c) 2020 Alex Chi
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

//! Machine mode and supervisor mode traps

use crate::{println, print, info, panic};
use crate::process::{TrapFrame, self, Process, CPU, my_proc, my_cpu, yield_cpu};
use crate::arch;
use crate::symbols::*;
use crate::page;
use crate::nulllock::Mutex;
use crate::syscall;
use crate::process::Register::a0;

#[no_mangle]
extern "C" fn m_trap() -> () {
    panic!("machine mode trap");
}

/// Process interrupt from supervisor mode
#[no_mangle]
extern "C" fn kerneltrap(
    epc: usize,
    tval: usize,
    cause: usize,
    hart: usize,
    _status: usize
) -> () {
    use riscv::register::*;
    if sstatus::read().spp() != sstatus::SPP::Supervisor {
        panic!("not from supervisor mode");
    }
    // We're going to handle all traps in machine mode. RISC-V lets
    // us delegate to supervisor mode, but switching out SATP (virtual memory)
    // gets hairy.
    let is_async = {
        if cause >> 63 & 1 == 1 {
            true
        } else {
            false
        }
    };
    // The cause contains the type of trap (sync, async) as well as the cause
    // number. So, here we narrow down just the cause number.
    let cause_num = cause & 0xfff;
    let mut return_pc = epc;
    if is_async {
        // Asynchronous trap
        match cause_num {
            3 => {
                // Machine software
                println!("Machine software interrupt CPU#{}", hart);
            }
            7 => unsafe {
                info!("Timer interrupt interrupt CPU#{}", hart);
                // Machine timer
                let mtimecmp = 0x0200_4000 as *mut u64;
                let mtime = 0x0200_bff8 as *const u64;
                // The frequency given by QEMU is 10_000_000 Hz, so this sets
                // the next interrupt to fire one second from now.
                mtimecmp.write_volatile(mtime.read_volatile() + 10_000_000);
            },
            9 => {
                // Machine external (interrupt from Platform Interrupt Controller (PLIC))
                // println!("Machine external interrupt CPU#{}", hart);
                // We will check the next interrupt. If the interrupt isn't available, this will
                // give us None. However, that would mean we got a spurious interrupt, unless we
                // get an interrupt from a non-PLIC source. This is the main reason that the PLIC
                // hardwires the id 0 to 0, so that we can use it as an error case.
                let mut PLIC = crate::plic::PLIC().lock();
                if let Some(interrupt) = PLIC.next() {
                    // If we get here, we've got an interrupt from the claim register. The PLIC will
                    // automatically prioritize the next interrupt, so when we get it from claim, it
                    // will be the next in priority order.
                    match interrupt {
                        10 => { // Interrupt 10 is the UART interrupt.
                            // We would typically set this to be handled out of the interrupt context,
                            // but we're testing here! C'mon!
                            // We haven't yet used the singleton pattern for my_uart, but remember, this
                            // just simply wraps 0x1000_0000 (UART).
                            let mut my_uart = crate::uart::UART().lock();
                            // If we get here, the UART better have something! If not, what happened??
                            if let Some(c) = my_uart.get() {
                                drop(my_uart);
                                // If you recognize this code, it used to be in the lib.rs under kmain(). That
                                // was because we needed to poll for UART data. Now that we have interrupts,
                                // here it goes!
                                match c {
                                    8 => {
                                        // This is a backspace, so we
                                        // essentially have to write a space and
                                        // backup again:
                                        print!("{} {}", 8 as char, 8 as char);
                                    }
                                    10 | 13 => {
                                        // Newline or carriage-return
                                        println!();
                                    }
                                    _ => {
                                        print!("{}", c as char);
                                    }
                                }
                            }
                        }
                        // Non-UART interrupts go here and do nothing.
                        _ => {
                            println!("Non-UART external interrupt: {}", interrupt);
                        }
                    }
                    // We've claimed it, so now say that we've handled it. This resets the interrupt pending
                    // and allows the UART to interrupt again. Otherwise, the UART will get "stuck".
                    PLIC.complete(interrupt);
                }
            }
            _ => {
                panic!("Unhandled async trap CPU#{} -> {}\n", hart, cause_num);
            }
        }
    } else {
        // Synchronous trap
        match cause_num {
            2 => {
                // Illegal instruction
                panic!(
                    "Illegal instruction CPU#{} -> 0x{:08x}: 0x{:08x}\n",
                    hart, epc, tval
                );
            }
            8 => {
                // Environment (system) call from User mode
                println!("E-call from User mode! CPU#{} -> 0x{:08x}", hart, epc);
                return_pc += 4;
            }
            9 => {
                // Environment (system) call from Supervisor mode
                println!("E-call from Supervisor mode! CPU#{} -> 0x{:08x}", hart, epc);
                return_pc += 4;
            }
            11 => {
                // Environment (system) call from Machine mode
                panic!("E-call from Machine mode! CPU#{} -> 0x{:08x}\n", hart, epc);
            }
            // Page faults
            12 => {
                // Instruction page fault
                println!(
                    "Instruction page fault CPU#{} -> 0x{:08x}: 0x{:08x}",
                    hart, epc, tval
                );
                return_pc += 4;
            }
            13 => {
                // Load page fault
                println!(
                    "Load page fault CPU#{} -> 0x{:08x}: 0x{:08x}",
                    hart, epc, tval
                );
                return_pc += 4;
            }
            15 => {
                // Store page fault
                println!(
                    "Store page fault CPU#{} -> 0x{:08x}: 0x{:08x}",
                    hart, epc, tval
                );
                return_pc += 4;
            }
            _ => {
                panic!("Unhandled sync trap CPU#{} -> {}\n", hart, cause_num);
            }
        }
    };
    // Finally, return the updated program counter
    // return_pc
}

/// Called by `uservec` in `trampoline.S`, return from user space.
#[no_mangle]
pub extern "C" fn usertrap() {
    use riscv::register::*;
    if sstatus::read().spp() != sstatus::SPP::User {
        panic!("not from user mode");
    }
    unsafe {
        stvec::write(kernelvec as usize, stvec::TrapMode::Direct);
    }
    let p = my_proc();
    p.trapframe.epc = sepc::read();
    let scause = scause::read().bits();
    if scause == 8 {
        p.trapframe.epc += 4;
        arch::intr_on();
        p.trapframe.regs[a0 as usize] = syscall::syscall() as usize;
        yield_cpu();
    } else {
        panic!("unexpected scause {}", scause);
    }

    usertrapret();
}

/// Jump to user space through trampoline after trapframe is properly set. Calls `userret` in `trampoline.S`.
#[inline]
fn trampoline_userret(tf: usize, satp_val: usize) -> ! {
    let uservec_offset = userret as usize - TRAMPOLINE_TEXT_START();
    let fn_addr = (TRAMPOLINE_START + uservec_offset) as *const ();
    let fn_addr: extern "C" fn(usize, usize) -> ! = unsafe { core::mem::transmute(fn_addr) };
    (fn_addr)(tf, satp_val)
}

/// Jump to user space through trampoline
pub fn usertrapret() -> ! {
    let satp_val: usize;
    {
        use riscv::register::*;
        arch::intr_off();

        // send syscalls, interrupts, and exceptions to trampoline.S
        unsafe {
            stvec::write(
                (uservec as usize - TRAMPOLINE_TEXT_START()) + TRAMPOLINE_START,
                stvec::TrapMode::Direct,
            );
        }

        // set up trapframe values that uservec will need when
        // the process next re-enters the kernel.
        let mut p = my_proc();
        let c = my_cpu();
        p.trapframe.satp = c.kernel_trapframe.satp;
        p.trapframe.sp = c.kernel_trapframe.sp;
        p.trapframe.trap = crate::trap::usertrap as usize;
        p.trapframe.hartid = c.kernel_trapframe.hartid;

        // println!("trap 0x{:x}", proc_cpu.process.trapframe.trap);

        // set S Previous Privilege mode to User.
        unsafe {
            sstatus::set_spie();
            sstatus::set_spp(sstatus::SPP::User);
        }

        // set S Exception Program Counter to the saved user pc.
        sepc::write(p.trapframe.epc);

        // tell trampoline.S the user page table to switch to.
        let root_ppn = &mut *p.pgtable as *mut page::Table as usize;
        satp_val = crate::arch::build_satp(8, 0, root_ppn);
    }
    // jump to trampoline.S at the top of memory, which 
    // switches to the user page table, restores user registers,
    // and switches to user mode with sret.
    // println!("jumping to trampoline 0x{:x} 0x{:x}...", trap_frame_addr , TRAPFRAME_START);
    trampoline_userret(TRAPFRAME_START, satp_val)
}

pub unsafe fn init() {
    use riscv::register::*;
    stvec::write(crate::symbols::kernelvec as usize, stvec::TrapMode::Direct);
}
