use std::collections::VecDeque;

use openlst_core::constants::TIMER_COUNT_PERIOD_MS;
use openlst_core::hal::{Interface, RadioHal, RxPacket};
use openlst_core::telemetry::Telemetry;
use openlst_core::time::TimeSpec;

use crate::mmio::cc1110_addr;
use crate::mmio::RegisterIo;
#[cfg(feature = "cc1110-lowlevel")]
use crate::lowlevel::isr;

const CLKCON_OSC32K_RC: u8 = 1 << 7;
const CLKCON_OSC_HSXTAL: u8 = 0 << 6;
const CLKCON_TICKSPD_F_2: u8 = 1 << 3;
const CLKCON_CLKSPD_F_2: u8 = 1 << 0;
const CLKCON_TICKSPD_F: u8 = 0 << 3;
const CLKCON_CLKSPD_F: u8 = 0 << 0;
const SLEEP_XOSC_STB: u8 = 1 << 6;
const SLEEP_OSC_PD: u8 = 1 << 2;

const RFST_SRX: u8 = 0x02;
const RFST_STX: u8 = 0x03;

const RFIF_IM_TXUNF: u8 = 1 << 7;
const RFIF_IM_DONE: u8 = 1 << 4;
const RFIF_IM_CS: u8 = 1 << 3;
const RFIF_IM_SFD: u8 = 1 << 0;

#[derive(Clone, Copy)]
enum UartPort {
    Uart0,
    Uart1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum RfIrqEvent {
    TxUnderrun,
    Done,
    CarrierSense,
    StartFrameDelimiter,
}

#[derive(Clone, Debug, Default)]
struct RfDmaState {
    rx_dma_armed: bool,
    tx_dma_armed: bool,
    rf_mode_tx: bool,
    rf_rx_underway: bool,
    rf_rx_complete: bool,
    tx_underflow: bool,
    rx_buffer: Vec<u8>,
    tx_buffer: Vec<u8>,
    pending_uart_sel: u8,
    precise_tx_delay_ms: u8,
}

pub struct Cc1110SkeletonBackend<I: RegisterIo> {
    io: I,
    hwid: u16,
    uptime_seconds: u32,
    milliseconds: u16,
    rtc_set: bool,
    now: TimeSpec,
    callsign: [u8; 8],
    telemetry: Telemetry,
    rx_queue: VecDeque<RxPacket>,
    tx_log: Vec<(Interface, Vec<u8>)>,
    rebooted: bool,
    rf_dma: RfDmaState,
    pending_precise_tx: Option<(Vec<u8>, u8)>,
    uart0_rx_count: u32,
    uart1_rx_count: u32,
    radio_last_rssi: i8,
    radio_last_lqi: u8,
    radio_last_freqest: i8,
    pub radio_cs_count: u32,
    pub radio_packets_sent: u32,
    pub radio_packets_good: u32,
    pub radio_packets_rejected_checksum: u32,
    pub radio_packets_rejected_reserved: u32,
    pub radio_packets_rejected_other: u32,
}

impl<I: RegisterIo> Cc1110SkeletonBackend<I> {
    pub fn new(io: I, hwid: u16) -> Self {
        Self {
            io,
            hwid,
            uptime_seconds: 0,
            milliseconds: 0,
            rtc_set: false,
            now: TimeSpec::default(),
            callsign: *b"OPENLST\0",
            telemetry: Telemetry::default(),
            rx_queue: VecDeque::new(),
            tx_log: Vec::new(),
            rebooted: false,
            rf_dma: RfDmaState::default(),
            pending_precise_tx: None,
            uart0_rx_count: 0,
            uart1_rx_count: 0,
            radio_last_rssi: -128,
            radio_last_lqi: 0,
            radio_last_freqest: 0,
            radio_cs_count: 0,
            radio_packets_sent: 0,
            radio_packets_good: 0,
            radio_packets_rejected_checksum: 0,
            radio_packets_rejected_reserved: 0,
            radio_packets_rejected_other: 0,
        }
    }

    pub fn initialize_clock(&mut self) {
        self.io.write8(
            cc1110_addr::CLKCON,
            CLKCON_OSC32K_RC | CLKCON_OSC_HSXTAL | CLKCON_TICKSPD_F_2 | CLKCON_CLKSPD_F_2,
        );

        let sleep = self.io.read8(cc1110_addr::SLEEP) | SLEEP_XOSC_STB;
        self.io.write8(cc1110_addr::SLEEP, sleep);

        self.io.write8(
            cc1110_addr::CLKCON,
            CLKCON_OSC32K_RC | CLKCON_OSC_HSXTAL | CLKCON_TICKSPD_F | CLKCON_CLKSPD_F,
        );

        let sleep = self.io.read8(cc1110_addr::SLEEP) | SLEEP_OSC_PD;
        self.io.write8(cc1110_addr::SLEEP, sleep);
    }

    pub fn initialize_timer(&mut self, t1_period_ticks: u16) {
        self.io
            .write8(cc1110_addr::T1CC0L, (t1_period_ticks & 0xFF) as u8);
        self.io
            .write8(cc1110_addr::T1CC0H, ((t1_period_ticks >> 8) & 0xFF) as u8);
        self.io.write8(cc1110_addr::T1CTL, 0x02);
    }

    pub fn initialize_uart0(&mut self, baud: u8, gcr: u8) {
        self.initialize_uart(UartPort::Uart0, baud, gcr);
    }

    pub fn initialize_uart1(&mut self, baud: u8, gcr: u8) {
        self.initialize_uart(UartPort::Uart1, baud, gcr);
    }

    pub fn initialize_rf(&mut self) {
        self.io.write8(cc1110_addr::PKTCTRL0, 0x05);
        self.io.write8(cc1110_addr::PKTCTRL1, 0x04);
        self.io.write8(cc1110_addr::RFIF, 0);
        self.rf_dma = RfDmaState::default();
        self.rf_dma.precise_tx_delay_ms = 3;
        self.arm_rf_rx_dma();
        self.io.write8(cc1110_addr::RFST, RFST_SRX);
    }

    pub fn tick_1ms(&mut self) {
        #[cfg(feature = "cc1110-lowlevel")]
        self.service_lowlevel_irqs();

        self.milliseconds = self.milliseconds.saturating_add(1);
        if self.milliseconds >= TIMER_COUNT_PERIOD_MS {
            self.milliseconds = 0;
            self.uptime_seconds = self.uptime_seconds.saturating_add(1);
            self.now.seconds = self.now.seconds.saturating_add(1);
        }

        if let Some((payload, uart_sel)) = self.pending_precise_tx.take() {
            if self.rf_dma.precise_tx_delay_ms <= 1 {
                self.transmit_rf_now(&payload, uart_sel);
            } else {
                self.rf_dma.precise_tx_delay_ms = self.rf_dma.precise_tx_delay_ms.saturating_sub(1);
                self.pending_precise_tx = Some((payload, uart_sel));
            }
        }
    }

    #[cfg(feature = "cc1110-lowlevel")]
    fn service_lowlevel_irqs(&mut self) {
        let pending = isr::take_pending_irqs();
        if pending == 0 {
            return;
        }

        if (pending & isr::RF_IRQ_TXUNF) != 0 {
            self.handle_rf_irq(RfIrqEvent::TxUnderrun);
        }
        if (pending & isr::RF_IRQ_CS) != 0 {
            self.handle_rf_irq(RfIrqEvent::CarrierSense);
        }
        if (pending & isr::RF_IRQ_SFD) != 0 {
            self.handle_rf_irq(RfIrqEvent::StartFrameDelimiter);
        }
        if (pending & isr::RF_IRQ_DONE) != 0 {
            self.handle_rf_irq(RfIrqEvent::Done);
        }
    }

    pub fn enqueue_rx(&mut self, packet: RxPacket) {
        match packet.interface {
            Interface::Uart0 => {
                self.uart0_rx_count = self.uart0_rx_count.saturating_add(1);
            }
            Interface::Uart1 => {
                self.uart1_rx_count = self.uart1_rx_count.saturating_add(1);
            }
            Interface::Rf => {}
        }
        self.rx_queue.push_back(packet);
    }

    pub fn arm_rf_rx_dma(&mut self) {
        self.rf_dma.rx_dma_armed = true;
        self.rf_dma.rx_buffer.clear();
    }

    pub fn arm_rf_tx_dma(&mut self, payload: &[u8]) {
        self.rf_dma.tx_dma_armed = true;
        self.rf_dma.tx_buffer.clear();
        self.rf_dma.tx_buffer.extend_from_slice(payload);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn rf_dma_push_rx_bytes(&mut self, payload: &[u8]) {
        if self.rf_dma.rx_dma_armed {
            self.rf_dma.rx_buffer.extend_from_slice(payload);
        }
    }

    pub fn handle_rf_irq(&mut self, event: RfIrqEvent) {
        let mut rfif = self.io.read8(cc1110_addr::RFIF);

        match event {
            RfIrqEvent::TxUnderrun => {
                rfif |= RFIF_IM_TXUNF;
                self.rf_dma.tx_underflow = true;
                self.rf_dma.rf_mode_tx = false;
            }
            RfIrqEvent::Done => {
                rfif |= RFIF_IM_DONE;
                if self.rf_dma.rf_mode_tx {
                    self.rf_dma.rf_mode_tx = false;
                    self.rf_dma.tx_dma_armed = false;
                    self.radio_packets_sent = self.radio_packets_sent.saturating_add(1);
                } else {
                    self.rf_dma.rf_rx_complete = true;
                    if self.rf_dma.rx_buffer.is_empty() {
                        self.radio_packets_rejected_other =
                            self.radio_packets_rejected_other.saturating_add(1);
                    } else if let Some(packet) =
                        RxPacket::from_slice(Interface::Rf, &self.rf_dma.rx_buffer)
                    {
                        self.rx_queue.push_back(packet);
                        self.radio_packets_good = self.radio_packets_good.saturating_add(1);
                    } else {
                        self.radio_packets_rejected_other =
                            self.radio_packets_rejected_other.saturating_add(1);
                    }
                    self.rf_dma.rf_rx_complete = false;
                    self.rf_dma.rf_rx_underway = false;
                    self.radio_last_rssi = -90;
                    self.radio_last_lqi = 100;
                    self.radio_last_freqest = 0;
                    self.arm_rf_rx_dma();
                }
            }
            RfIrqEvent::CarrierSense => {
                rfif |= RFIF_IM_CS;
                self.radio_cs_count = self.radio_cs_count.saturating_add(1);
            }
            RfIrqEvent::StartFrameDelimiter => {
                rfif |= RFIF_IM_SFD;
                if !self.rf_dma.rf_mode_tx {
                    self.rf_dma.rf_rx_underway = true;
                }
            }
        }

        self.io.write8(cc1110_addr::RFIF, rfif);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn inject_rf_dma_frame(&mut self, payload: &[u8], uart_sel: u8) {
        self.rf_dma.pending_uart_sel = uart_sel;
        self.arm_rf_rx_dma();
        self.handle_rf_irq(RfIrqEvent::StartFrameDelimiter);
        self.rf_dma_push_rx_bytes(payload);
        self.handle_rf_irq(RfIrqEvent::Done);
    }

    pub fn tx_log(&self) -> &[(Interface, Vec<u8>)] {
        &self.tx_log
    }

    pub fn is_rebooted(&self) -> bool {
        self.rebooted
    }

    fn initialize_uart(&mut self, port: UartPort, baud: u8, gcr: u8) {
        let (csr, baud_reg, gcr_reg) = match port {
            UartPort::Uart0 => (cc1110_addr::U0CSR, cc1110_addr::U0BAUD, cc1110_addr::U0GCR),
            UartPort::Uart1 => (cc1110_addr::U1CSR, cc1110_addr::U1BAUD, cc1110_addr::U1GCR),
        };

        self.io.write8(baud_reg, baud);
        self.io.write8(gcr_reg, gcr);
        self.io.write8(csr, (1 << 7) | (1 << 6));
    }

    fn uart_write(&mut self, interface: Interface, payload: &[u8]) {
        let dbuf = match interface {
            Interface::Uart0 => cc1110_addr::U0DBUF,
            Interface::Uart1 => cc1110_addr::U1DBUF,
            Interface::Rf => return,
        };
        for byte in payload {
            self.io.write8(dbuf, *byte);
        }
    }

    fn transmit_rf_now(&mut self, payload: &[u8], uart_sel: u8) {
        self.rf_dma.pending_uart_sel = uart_sel;
        self.arm_rf_tx_dma(payload);
        self.rf_dma.rf_mode_tx = true;
        self.io.write8(cc1110_addr::RFST, RFST_STX);
        self.handle_rf_irq(RfIrqEvent::Done);
    }
}

impl<I: RegisterIo> RadioHal for Cc1110SkeletonBackend<I> {
    fn hwid_flash(&self) -> u16 {
        self.hwid
    }

    fn watchdog_clear(&mut self) {}

    fn reboot_now(&mut self) {
        self.rebooted = true;
    }

    fn uptime_seconds(&self) -> u32 {
        self.uptime_seconds
    }

    fn rtc_set(&self) -> bool {
        self.rtc_set
    }

    fn get_time(&self) -> TimeSpec {
        self.now
    }

    fn set_time(&mut self, time: TimeSpec) {
        self.now = time;
        self.rtc_set = true;
    }

    fn get_callsign(&self) -> [u8; 8] {
        self.callsign
    }

    fn set_callsign(&mut self, callsign: [u8; 8]) {
        self.callsign = callsign;
    }

    fn telemetry_snapshot(&self) -> Telemetry {
        let mut telemetry = self.telemetry;
        telemetry.uptime = self.uptime_seconds;
        telemetry.uart0_rx_count = self.uart0_rx_count;
        telemetry.uart1_rx_count = self.uart1_rx_count;
        telemetry.last_rssi = self.radio_last_rssi;
        telemetry.last_lqi = self.radio_last_lqi;
        telemetry.last_freqest = self.radio_last_freqest;
        telemetry.cs_count = self.radio_cs_count;
        telemetry.packets_sent = self.radio_packets_sent;
        telemetry.packets_good = self.radio_packets_good;
        telemetry.packets_rejected_checksum = self.radio_packets_rejected_checksum;
        telemetry.packets_rejected_reserved = self.radio_packets_rejected_reserved;
        telemetry.packets_rejected_other = self.radio_packets_rejected_other;
        telemetry
    }

    fn adc_start_sample(&mut self) {}

    fn update_telemetry(&mut self) {
        self.telemetry.uptime = self.uptime_seconds;
    }

    fn radio_listen(&mut self) {
        self.arm_rf_rx_dma();
        self.io.write8(cc1110_addr::RFST, RFST_SRX);
    }

    fn poll_rx(&mut self) -> Option<RxPacket> {
        self.rx_queue.pop_front()
    }

    fn send(&mut self, interface: Interface, payload: &[u8]) {
        match interface {
            Interface::Uart0 | Interface::Uart1 => self.uart_write(interface, payload),
            Interface::Rf => {
                self.send_rf(payload, 0);
                return;
            }
        }
        self.tx_log.push((interface, payload.to_vec()));
    }

    fn send_rf(&mut self, payload: &[u8], uart_sel: u8) {
        self.transmit_rf_now(payload, uart_sel);
        self.tx_log.push((Interface::Rf, payload.to_vec()));
    }

    fn send_rf_precise(&mut self, payload: &[u8], uart_sel: u8) {
        self.pending_precise_tx = Some((payload.to_vec(), uart_sel));
        self.rf_dma.precise_tx_delay_ms = 3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mmio::MockRegisterIo;
    #[cfg(feature = "cc1110-lowlevel")]
    use crate::lowlevel::isr;

    #[test]
    fn clock_init_powers_down_hsrc() {
        let mut backend = Cc1110SkeletonBackend::new(MockRegisterIo::new(), 0x1201);
        backend.initialize_clock();
        let sleep = backend.io.read8(cc1110_addr::SLEEP);
        assert_ne!(sleep & SLEEP_OSC_PD, 0);
    }

    #[test]
    fn rf_send_is_recorded() {
        let mut backend = Cc1110SkeletonBackend::new(MockRegisterIo::new(), 0x1201);
        backend.send_rf(&[1, 2, 3], 1);
        assert_eq!(backend.tx_log().len(), 1);
        assert_eq!(backend.radio_packets_sent, 1);
    }

    #[test]
    fn rf_precise_send_is_delayed() {
        let mut backend = Cc1110SkeletonBackend::new(MockRegisterIo::new(), 0x1201);
        backend.send_rf_precise(&[1, 2, 3], 1);
        assert_eq!(backend.radio_packets_sent, 0);
        backend.tick_1ms();
        backend.tick_1ms();
        backend.tick_1ms();
        assert_eq!(backend.radio_packets_sent, 1);
    }

    #[test]
    fn rf_irq_rx_dma_path_enqueues_packet() {
        let mut backend = Cc1110SkeletonBackend::new(MockRegisterIo::new(), 0x1201);
        backend.initialize_rf();
        backend.inject_rf_dma_frame(&[0x22, 0x69, 0x02, 0xAA, 0xBB], 1);
        let packet = backend.poll_rx().expect("rf packet");
        assert_eq!(packet.interface, Interface::Rf);
        assert_eq!(packet.payload(), &[0x22, 0x69, 0x02, 0xAA, 0xBB]);
        assert_eq!(backend.radio_packets_good, 1);
    }

    #[test]
    fn rf_irq_carrier_sense_counts() {
        let mut backend = Cc1110SkeletonBackend::new(MockRegisterIo::new(), 0x1201);
        backend.handle_rf_irq(RfIrqEvent::CarrierSense);
        backend.handle_rf_irq(RfIrqEvent::CarrierSense);
        assert_eq!(backend.radio_cs_count, 2);
    }

    #[test]
    fn rf_irq_tx_underrun_sets_flag() {
        let mut backend = Cc1110SkeletonBackend::new(MockRegisterIo::new(), 0x1201);
        backend.handle_rf_irq(RfIrqEvent::TxUnderrun);
        let rfif = backend.io.read8(cc1110_addr::RFIF);
        assert_ne!(rfif & RFIF_IM_TXUNF, 0);
    }

    #[cfg(feature = "cc1110-lowlevel")]
    #[test]
    fn lowlevel_rf_done_irq_is_consumed() {
        let mut backend = Cc1110SkeletonBackend::new(MockRegisterIo::new(), 0x1201);
        backend.initialize_rf();
        backend.arm_rf_rx_dma();
        backend.rf_dma_push_rx_bytes(&[0x22, 0x69, 0x01, 0xAA]);

        isr::raise(isr::RF_IRQ_DONE);
        backend.tick_1ms();

        let packet = backend.poll_rx().expect("rx from lowlevel irq");
        assert_eq!(packet.interface, Interface::Rf);
        assert_eq!(packet.payload(), &[0x22, 0x69, 0x01, 0xAA]);
    }
}
