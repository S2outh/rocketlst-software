
use south_common::tmtc_system::telemetry_definition;

#[telemetry_definition(id = 0, address = south_common::tmtc_system)]
mod groundstation {
    mod lst {
        #[tmv(u32)]
        struct Uptime;

        #[tmv(i8)]
        struct Rssi;

        #[tmv(u8)]
        struct Lqi;

        #[tmv(u32)]
        struct PacketsSent;

        #[tmv(u32)]
        struct PacketsGood;

        #[tmv(u32)]
        struct PacketsRejectedChecksum;

        #[tmv(u32)]
        struct PacketsRejectedOther;
    }
}
