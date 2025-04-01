use algo::gridtrading;
use trend_algo::trendtrading;
use hftbacktest::{
    live::{
        Instrument,
        LiveBot,
        LiveBotBuilder,
        LoggingRecorder,
        ipc::iceoryx::IceoryxUnifiedChannel,
    },
    prelude::{Bot, ErrorKind, HashMapMarketDepth, HkPriceAction},
};
use tracing::error;

mod algo;
mod trend_algo;

const ORDER_PREFIX: &str = "prefix";

fn prepare_live() -> LiveBot<IceoryxUnifiedChannel, HashMapMarketDepth, HkPriceAction> {
    let price_action = HkPriceAction::new(vec![5*60*1000000000,15*60*1000000000,30*60*1000000000], vec![6,12,24]);
    let mut hbt = LiveBotBuilder::new()
        .register(Instrument::new(
            "binancefutures",
            "btcusdt",//"1000SHIBUSDT",
            0.1,//0.000001,
            0.001,//1.0,
            HashMapMarketDepth::new(0.1,0.001),//(0.000001, 1.0),
            0,
            price_action.clone(),
        ))
        .error_handler(|error| {
            match error.kind {
                ErrorKind::ConnectionInterrupted => {
                    error!("ConnectionInterrupted");
                }
                ErrorKind::CriticalConnectionError => {
                    error!("CriticalConnectionError");
                }
                ErrorKind::OrderError => {
                    let error = error.value();
                    error!(?error, "OrderError");
                }
                ErrorKind::Custom(errno) => {
                    error!(%errno, "custom");
                }
            }
            Ok(())
        })
        .build()
        .unwrap();

    hbt
}

fn main() {
    tracing_subscriber::fmt::init();

    let mut hbt = prepare_live();

    let relative_half_spread = 2.0;//0.0005;
    let relative_grid_interval = 2.0;//0.0005;
    let grid_num = 10;
    let min_grid_step = 0.1;//0.000001; // tick size
    let skew = relative_half_spread / grid_num as f64;
    let order_qty = 0.001;//1.0;
    let max_position = grid_num as f64 * order_qty;

    let mut recorder = LoggingRecorder::new();
    // gridtrading(
    trendtrading(
        &mut hbt,
        &mut recorder,
        relative_half_spread,
        relative_grid_interval,
        grid_num,
        min_grid_step,
        skew,
        order_qty,
        max_position,
    )
    .unwrap();
    hbt.close().unwrap();
}
