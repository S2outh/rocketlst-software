#[cfg(feature = "cc1110-lowlevel")]
pub mod dma {
    #[repr(C, packed)]
    #[cfg_attr(not(test), allow(dead_code))]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct DmaDescriptor {
        pub src_h: u8,
        pub src_l: u8,
        pub dest_h: u8,
        pub dest_l: u8,
        pub len_h: u8,
        pub len_l: u8,
        pub trig_cfg: u8,
        pub inc_cfg: u8,
    }

    #[allow(dead_code)]
    impl DmaDescriptor {
        pub const fn new() -> Self {
            Self {
                src_h: 0,
                src_l: 0,
                dest_h: 0,
                dest_l: 0,
                len_h: 0,
                len_l: 0,
                trig_cfg: 0,
                inc_cfg: 0,
            }
        }

        pub fn set_source(&mut self, addr: u16) {
            self.src_h = ((addr >> 8) & 0xFF) as u8;
            self.src_l = (addr & 0xFF) as u8;
        }

        pub fn set_destination(&mut self, addr: u16) {
            self.dest_h = ((addr >> 8) & 0xFF) as u8;
            self.dest_l = (addr & 0xFF) as u8;
        }

        pub fn set_length(&mut self, len: u16) {
            self.len_h = ((len >> 8) & 0x1F) as u8;
            self.len_l = (len & 0xFF) as u8;
        }

        pub fn set_transfer_config(&mut self, trig_cfg: u8, inc_cfg: u8) {
            self.trig_cfg = trig_cfg;
            self.inc_cfg = inc_cfg;
        }
    }

    #[unsafe(no_mangle)]
    #[unsafe(link_section = ".cc1110_dma")]
    pub static mut DMA_DESC_RF: DmaDescriptor = DmaDescriptor::new();

    #[unsafe(no_mangle)]
    #[unsafe(link_section = ".cc1110_dma")]
    pub static mut DMA_DESC_AES_IN: DmaDescriptor = DmaDescriptor::new();

    #[unsafe(no_mangle)]
    #[unsafe(link_section = ".cc1110_dma")]
    pub static mut DMA_DESC_AES_OUT: DmaDescriptor = DmaDescriptor::new();

    #[cfg(test)]
    mod tests {
        use super::DmaDescriptor;

        #[test]
        fn descriptor_layout_is_8_bytes() {
            assert_eq!(core::mem::size_of::<DmaDescriptor>(), 8);
        }
    }
}

#[cfg(feature = "cc1110-lowlevel")]
pub mod isr {
    use core::sync::atomic::{AtomicU8, Ordering};

    pub const RF_IRQ_DONE: u8 = 1 << 0;
    pub const RF_IRQ_SFD: u8 = 1 << 1;
    pub const RF_IRQ_CS: u8 = 1 << 2;
    pub const RF_IRQ_TXUNF: u8 = 1 << 3;
    pub const TIMER1_IRQ: u8 = 1 << 4;
    pub const UART0_RX_IRQ: u8 = 1 << 5;
    pub const UART1_RX_IRQ: u8 = 1 << 6;

    static PENDING_IRQS: AtomicU8 = AtomicU8::new(0);

    #[inline]
    pub fn take_pending_irqs() -> u8 {
        PENDING_IRQS.swap(0, Ordering::AcqRel)
    }

    #[inline]
    pub fn raise(mask: u8) {
        PENDING_IRQS.fetch_or(mask, Ordering::AcqRel);
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn rf_isr() {
        raise(RF_IRQ_DONE);
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn t1_isr() {
        raise(TIMER1_IRQ);
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn uart0_rx_isr() {
        raise(UART0_RX_IRQ);
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn uart1_rx_isr() {
        raise(UART1_RX_IRQ);
    }
}
