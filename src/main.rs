#![no_std]
#![no_main]
#![feature(panic_info_message, asm)]
#![feature(global_asm)]
#![feature(format_args_nl)]

global_asm!(include_str!("asm/trap.S"));
global_asm!(include_str!("asm/boot.S"));
global_asm!(include_str!("asm/ld_symbols.S"));

mod alloc;
mod constant;
mod cpu;
mod init;
mod memory;
mod mmu;
mod nulllock;
mod print;
mod trap;
mod uart;

use riscv::{asm, register::*};

#[no_mangle]
extern "C" fn eh_personality() {}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
	print!("Aborting: ");
	if let Some(p) = info.location() {
		println!(
			"line {}, file {}: {}",
			p.line(),
			p.file(),
			info.message().unwrap()
		);
	} else {
		println!("no information available.");
	}
	abort();
}
#[no_mangle]
extern "C" fn abort() -> ! {
	loop {
		unsafe {
			asm!("wfi"::::"volatile");
		}
	}
}
#[no_mangle]
extern "C" fn kinit() {
	// memory::zero_volatile(constant::bss_range());
	uart::UART.lock().init();

	println!("mhartid {}", mhartid::read());

	use constant::*;
	unsafe {
		println!("TEXT:   0x{:x} -> 0x{:x}", TEXT_START, TEXT_END);
		println!("RODATA: 0x{:x} -> 0x{:x}", RODATA_START, RODATA_END);
		println!("DATA:   0x{:x} -> 0x{:x}", DATA_START, DATA_END);
		println!("BSS:    0x{:x} -> 0x{:x}", BSS_START, BSS_END);
		println!(
			"STACK:  0x{:x} -> 0x{:x}",
			KERNEL_STACK_START, KERNEL_STACK_END
		);
		println!(
			"HEAP:   0x{:x} -> 0x{:x}",
			HEAP_START,
			HEAP_START + HEAP_SIZE
		);
	}
	use mmu::EntryAttributes;
	use mmu::{Table, KERNEL_PGTABLE};
	let mut pgtable = KERNEL_PGTABLE.lock();
	pgtable.id_map_range(
		unsafe { TEXT_START },
		unsafe { TEXT_END },
		EntryAttributes::RX as usize,
	);
	pgtable.id_map_range(
		unsafe { RODATA_START },
		unsafe { RODATA_END },
		EntryAttributes::RX as usize,
	);
	pgtable.id_map_range(
		unsafe { DATA_START },
		unsafe { DATA_END },
		EntryAttributes::RW as usize,
	);
	pgtable.id_map_range(
		unsafe { BSS_START },
		unsafe { BSS_END },
		EntryAttributes::RW as usize,
	);
	pgtable.id_map_range(
		unsafe { KERNEL_STACK_START },
		unsafe { KERNEL_STACK_END },
		EntryAttributes::RW as usize,
	);
	pgtable.map(
		UART_BASE_ADDR,
		UART_BASE_ADDR,
		EntryAttributes::RW as usize,
		0,
	);
	// pgtable.walk();
	pgtable.id_map_range(
		unsafe { HEAP_START },
		unsafe { HEAP_START + HEAP_SIZE },
		EntryAttributes::RW as usize,
	);
	// CLINT
	//  -> MSIP
	pgtable.id_map_range(0x0200_0000, 0x0200_ffff, EntryAttributes::RW as usize);
	// PLIC
	pgtable.id_map_range(0x0c00_0000, 0x0c00_2000, EntryAttributes::RW as usize);
	pgtable.id_map_range(0x0c20_0000, 0x0c20_8000, EntryAttributes::RW as usize);
	use uart::*;
	/* TODO: use Rust primitives */
	unsafe {
		let root_ppn = &mut *pgtable as *mut Table as usize;
		let satp_val = cpu::build_satp(8, 0, root_ppn);
		mscratch::write(&mut cpu::KERNEL_TRAP_FRAME[0] as *mut cpu::TrapFrame as usize);
		sscratch::write(mscratch::read());
		cpu::KERNEL_TRAP_FRAME[0].satp = satp_val;
		let stack_addr = alloc::ALLOC.lock().allocate(1);
		cpu::KERNEL_TRAP_FRAME[0].trap_stack = stack_addr.add(alloc::PAGE_SIZE);
		pgtable.id_map_range(
			stack_addr as usize,
			unsafe { stack_addr.add(alloc::PAGE_SIZE) } as usize,
			EntryAttributes::RW as usize,
		);

		use cpu::TrapFrame;
		let _sz = core::mem::size_of::<TrapFrame>();
		pgtable.id_map_range(
			mscratch::read(),
			mscratch::read() + _sz,
			EntryAttributes::RW as usize,
		);
		unsafe {
			asm!("csrw satp, $0" :: "r"(satp_val));
			asm!("sfence.vma zero, zero");
		}
	}
	unsafe {
		let rval: usize;
		asm!("csrr $0, satp" :"=r"(rval));
		println!("{:X}", rval);
	}
}

#[no_mangle]
extern "C" fn kmain() -> usize {
	use constant::*;
	use uart::*;
	println!("Now in supervisor mode!");
	println!("Try writing to UART...");
	println!("Hello!");
	unsafe {
		// Set the next machine timer to fire.
		let mtimecmp = 0x0200_4000 as *mut u64;
		let mtime = 0x0200_bff8 as *const u64;
		// The frequency given by QEMU is 10_000_000 Hz, so this sets
		// the next interrupt to fire one second from now.
		mtimecmp.write_volatile(mtime.read_volatile() + 100000);
	
		// Let's cause a page fault and see what happens. This should trap
		// to m_trap under trap.rs
		let v = 0x0 as *mut u64;
		v.write_volatile(0);
	}
	loop {
		unsafe {
		}
	}
}

pub fn test_alloc() {
	let ptr = alloc::ALLOC.lock().allocate(64 * 4096);
	let ptr = alloc::ALLOC.lock().allocate(1);
	let ptr2 = alloc::ALLOC.lock().allocate(1);
	let ptr = alloc::ALLOC.lock().allocate(1);
	alloc::ALLOC.lock().deallocate(ptr);
	let ptr = alloc::ALLOC.lock().allocate(1);
	let ptr = alloc::ALLOC.lock().allocate(1);
	let ptr = alloc::ALLOC.lock().allocate(1);
	alloc::ALLOC.lock().deallocate(ptr2);
	alloc::ALLOC.lock().debug();
}
