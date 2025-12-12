use embassy_stm32::can::{self, BufferedCanFd, CanConfigurator, RxFdBuf, TxFdBuf, filter::{FilterType, StandardFilter}};
use embedded_can::StandardId;
use heapless::Vec;


/// Can peripheral in configuration stage
pub struct CanPeriphConfig<'d> {
    filters: Vec<StandardFilter, 8>,
    configurator: CanConfigurator<'d>
}

/// Marker struct for the error mode that can filters are full
#[derive(Debug)]
pub struct FiltersFullError;

impl<'d> CanPeriphConfig<'d> {
    /// # create an instance using a base can configurator
    ///
    /// this function takes a minimally configured CanConfigurator instance
    /// as well as the rodos id this device will identify itself to other devices
    /// to generate the CanConfigurator simply provide a periph bus, can rx and tx pins and an interrupt reference
    /// ```
    /// CanConfigurator::new(p.FDCAN1, p.PA11, p.PA12, Irqs);
    /// ```
    /// in principle the Can Interface can be put into active state and used
    /// directly following this function call, however you won't be able to\
    /// receive any messages without specifying at least one topic with
    /// ```
    /// pub fn add_receive_topic(&mut self, topic: u16)
    /// ```
    pub fn new(
        mut configurator: CanConfigurator<'d>,
    ) -> Self {
        // reject all can Ids by default
        configurator.set_config(
            can::config::FdCanConfig::default()
                .set_global_filter(can::config::GlobalFilter::reject_all()),
        );

        let filters = Vec::new();
        Self {
            filters, 
            configurator,
        }
    }
    /// # add topic filter
    pub fn add_receive_topic(&mut self, topic: u16) -> Result<&mut Self, FiltersFullError> {
        let filter = FilterType::DedicatedSingle(
            StandardId::new(topic).unwrap(),
        );
        let standard_filter = StandardFilter {
            filter,
            action: can::filter::Action::StoreInFifo0,
        };
        self.filters.push(standard_filter).map_err(|_| FiltersFullError)?;
        Ok(self)
    }
    /// # add topic filter range
    pub fn add_receive_topic_range(&mut self, range: (u16, u16)) -> Result<&mut Self, FiltersFullError> {
        let filter = FilterType::Range {
            from: StandardId::new(range.0).unwrap(),
            to: StandardId::new(range.1).unwrap(),
        };
        let standard_filter = StandardFilter {
            filter,
            action: can::filter::Action::StoreInFifo0,
        };
        self.filters.push(standard_filter).map_err(|_| FiltersFullError)?;
        Ok(self)
    }
    /// # set can bitrate
    ///
    /// This function simply calls the set_fd_data_bitrate function on the can configurator.
    /// In principle it does not matter whether you use this function or simply call
    /// set_fd_data_bitrate on the configurator before passing it to ::new()
    pub fn set_bitrate(&mut self, bitrate: u32) -> &mut Self {
        self.configurator.set_fd_data_bitrate(bitrate, true);
        self
    }
    /// # Activate the can transmitter for sending and receiving
    /// returning a bufferedSender and a bufferedReceiver
    pub fn activate<const TX_BUF_SIZE: usize, const RX_BUF_SIZE: usize>(
        mut self,
        tx_buf: &'static mut TxFdBuf<TX_BUF_SIZE>,
        rx_buf: &'static mut RxFdBuf<RX_BUF_SIZE>,
    ) -> BufferedCanFd<'d, TX_BUF_SIZE, RX_BUF_SIZE> {
        // fill up unused filter slots with disabled filters
        while !self.filters.is_full() {
            self.filters.push(StandardFilter::disable()).unwrap();
        }
        self.configurator
            .properties()
            .set_standard_filters(&self.filters.into_array().unwrap());

        // initialize buffered can
        self.configurator.into_normal_mode().buffered_fd(
            tx_buf,
            rx_buf
        )
    }
}
