
use south_common::chell::chell_definition;

#[chell_definition(id = 0, address = south_common::chell)]
mod groundstation {
    mod lst {
        #[chv(u32)]
        struct Uptime;

        #[chv(i8)]
        struct Rssi;

        #[chv(u8)]
        struct Lqi;

        #[chv(u32)]
        struct PacketsSent;

        #[chv(u32)]
        struct PacketsGood;

        #[chv(u32)]
        struct PacketsRejectedChecksum;

        #[chv(u32)]
        struct PacketsRejectedOther;
    }
}
