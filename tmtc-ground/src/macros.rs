#[macro_export]
macro_rules! parse_beacon {
    ($data: ident, $beacon:ident, $nats_sender:ident $(, ($($field:ident),*))?) => {
        paste::paste! {
            match $beacon.from_bytes($data, &mut crc_ccitt) {
                Ok(()) => {
                    info!("{} Received at {}", stringify!([<$beacon:snake:upper>]), $beacon.timestamp);
                    $($(
                        if let Some(value) = $beacon.$field {
                            info!("Telemetry: {}: {:#?}", stringify!($field), value);
                        } else {
                            warn!("No telemetry received for {}", stringify!($field));
                        }
                    )*)?
                    match $beacon.serialize(&CborSerializer) {
                        Ok(serialized) => {
                            for value in serialized {
                                let _ = $nats_sender.send(value).await;
                            }
                        },
                        Err(_) => error!("could not serialize received value")
                    }
                }
                Err(e) => {
                    match e {
                        ParseError::WrongId => (),
                        ParseError::BadCRC => error!("{} with bad crc received", stringify!($beacon)),
                        ParseError::OutOfMemory => error!("{} could not be parsed: not enough bytes", stringify!($beacon)),
                    }
                }
            }
        }
    }
}


#[macro_export]
macro_rules! print_lst_values {
    ($lst_telem:ident, ($($field:ident),*)) => {
        paste::paste! { $(
            info!("LST {}: {:?}", stringify!($field), $lst_telem.[<$field: snake>]);
        )* }
    }
}

#[macro_export]
macro_rules! pub_lst_values {
    ($nats_sender: ident, $lst_telem:ident, $timestamp: ident, ($($field:ident),*)) => {
        paste::paste! {
            $(
                let serialized = $lst_telem.[<$field: snake>].serialize_ground(&ground_tm_defs::groundstation::lst::$field, $timestamp, &CborSerializer)
                                    .expect("could not serialize value");
                for v in serialized {
                    let _ = $nats_sender.send(v).await;
                }
            )*
        }
    }
}
