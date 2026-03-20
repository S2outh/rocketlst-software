mod cc1110_backend;
#[cfg(feature = "cc1110-lowlevel")]
mod lowlevel;
mod mmio;

use cc1110_backend::Cc1110SkeletonBackend;
#[cfg(feature = "cc1110-real-mmio")]
use mmio::VolatileRegisterIo;
#[cfg(not(feature = "cc1110-real-mmio"))]
use mmio::MockRegisterIo;

use openlst_core::constants::{ESP_MAX_PAYLOAD, MSG_TYPE_RADIO_IN};
use openlst_core::hal::{Interface, RxPacket};
use openlst_core::protocol::{encode_command, radio, CommandHeader};
use openlst_core::runtime::{RadioRuntime, RuntimeConfig};

#[cfg(feature = "cc1110-real-mmio")]
fn read_mmio_base() -> usize {
    std::env::var("OPENLST_MMIO_BASE")
        .ok()
        .and_then(|raw| {
            let trimmed = raw
                .strip_prefix("0x")
                .or_else(|| raw.strip_prefix("0X"))
                .unwrap_or(&raw);
            usize::from_str_radix(trimmed, 16).ok()
        })
        .unwrap_or(0)
}

fn main() {
    #[cfg(feature = "cc1110-real-mmio")]
    let mut hal = {
        let base = read_mmio_base();
        let io = unsafe { VolatileRegisterIo::new(base) };
        Cc1110SkeletonBackend::new(io, 0x1234)
    };

    #[cfg(not(feature = "cc1110-real-mmio"))]
    let mut hal = Cc1110SkeletonBackend::new(MockRegisterIo::new(), 0x1234);

    hal.initialize_clock();
    hal.initialize_timer(27_000);
    hal.initialize_uart0(24, 12);
    hal.initialize_uart1(24, 12);
    hal.initialize_rf();

    let mut frame = [0u8; ESP_MAX_PAYLOAD];
    let header = CommandHeader {
        hwid: 0x1234,
        seqnum: 1,
        system: MSG_TYPE_RADIO_IN,
        command: radio::GET_TIME,
    };
    let length = encode_command(header, &[], &mut frame).expect("encode");
    let rx_once = RxPacket::from_slice(Interface::Uart0, &frame[..length]).expect("rx packet");
    hal.enqueue_rx(rx_once);

    let mut runtime = RadioRuntime::new(RuntimeConfig::default());
    runtime.init(&mut hal);

    for _ in 0..1000 {
        runtime.tick_1ms(&mut hal);
        if hal.is_rebooted() {
            break;
        }
        hal.tick_1ms();
    }

    let _ = hal.tx_log().len();
}
