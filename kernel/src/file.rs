// Copyright (c) 2020 Alex Chi
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

//! File trait and file in core-os including file in filesystem, device, pipe and symbol link

use crate::uart::UART;
use core::ops::DerefMut;

/// File trait
pub trait File : Send + Sync {
    /// read from file to content and returns number of characters read
    fn read(&mut self, content: &mut [u8]) -> i32;
    /// write content to file and returns number of characters written
    fn write(&mut self, content: &[u8]) -> i32;
}

/// Console file
pub struct Console {}

impl File for Console {
    /// read from console
    fn read(&mut self, content: &mut [u8]) -> i32 {
        let mut uart = UART().lock();
        for i in 0..content.len() {
            match uart.get() {
                Some(ch) => { content[i] = ch; }
                _ => { return i as i32; }
            }
        }
        return content.len() as i32;
    }

    /// write to console
    fn write(&mut self, content: &[u8]) -> i32 {
        let mut uart = UART().lock();
        for i in 0..content.len() {
            uart.put(content[i]);
        }
        return content.len() as i32;
    }
}