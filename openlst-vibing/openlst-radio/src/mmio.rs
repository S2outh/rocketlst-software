pub trait RegisterIo {
    fn read8(&self, addr: u16) -> u8;
    fn write8(&mut self, addr: u16, value: u8);
}

#[cfg(feature = "cc1110-real-mmio")]
#[allow(dead_code)]
pub struct VolatileRegisterIo {
    base_addr: usize,
}

#[cfg(feature = "cc1110-real-mmio")]
#[allow(dead_code)]
impl VolatileRegisterIo {
    pub unsafe fn new(base_addr: usize) -> Self {
        Self { base_addr }
    }

    #[inline]
    fn ptr(&self, addr: u16) -> *mut u8 {
        (self.base_addr + addr as usize) as *mut u8
    }
}

#[cfg(feature = "cc1110-real-mmio")]
impl RegisterIo for VolatileRegisterIo {
    fn read8(&self, addr: u16) -> u8 {
        unsafe { core::ptr::read_volatile(self.ptr(addr)) }
    }

    fn write8(&mut self, addr: u16, value: u8) {
        unsafe {
            core::ptr::write_volatile(self.ptr(addr), value);
        }
    }
}

#[cfg_attr(feature = "cc1110-real-mmio", allow(dead_code))]
pub struct MockRegisterIo {
    memory: [u8; 0x10000],
}

#[cfg_attr(feature = "cc1110-real-mmio", allow(dead_code))]
impl MockRegisterIo {
    pub fn new() -> Self {
        Self {
            memory: [0; 0x10000],
        }
    }
}

impl Default for MockRegisterIo {
    fn default() -> Self {
        Self::new()
    }
}

impl RegisterIo for MockRegisterIo {
    fn read8(&self, addr: u16) -> u8 {
        self.memory[addr as usize]
    }

    fn write8(&mut self, addr: u16, value: u8) {
        self.memory[addr as usize] = value;
    }
}

pub mod cc1110_addr {
    pub const CLKCON: u16 = 0xC6;
    pub const SLEEP: u16 = 0xBE;

    pub const T1CTL: u16 = 0xE4;
    pub const T1CC0L: u16 = 0xDA;
    pub const T1CC0H: u16 = 0xDB;

    pub const U0CSR: u16 = 0x86;
    pub const U0BAUD: u16 = 0xC2;
    pub const U0GCR: u16 = 0xC5;
    pub const U0DBUF: u16 = 0xC1;

    pub const U1CSR: u16 = 0xF8;
    pub const U1BAUD: u16 = 0xFA;
    pub const U1GCR: u16 = 0xF9;
    pub const U1DBUF: u16 = 0xF7;

    pub const RFST: u16 = 0xE1;
    pub const RFIF: u16 = 0xE9;
    pub const PKTCTRL0: u16 = 0xDF15;
    pub const PKTCTRL1: u16 = 0xDF16;
}
