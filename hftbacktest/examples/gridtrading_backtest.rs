use std::vec;

use algo::gridtrading;
use hftbacktest::{
    backtest::{
        Backtest,
        ExchangeKind,
        L2AssetBuilder,
        assettype::LinearAsset,
        data::{DataSource, read_npz_file},
        models::{
            CommonFees,
            IntpOrderLatency,
            PowerProbQueueFunc3,
            ProbQueueModel,
            TradingValueFeeModel,
        },
        recorder::BacktestRecorder,
    },
    prelude::{ApplySnapshot, Bot, HashMapMarketDepth, HkPriceAction},
};

mod algo;

fn prepare_backtest() -> Backtest<HashMapMarketDepth,HkPriceAction> {
    // let latency_data = (20240501..20240532)
    //     .map(|date| DataSource::File(format!("latency_{date}.npz")))
    //     .collect();
    let latency_data = vec![DataSource::File("examples/usdm/feed_latency_20240801.npz".to_string())];

    let latency_model = IntpOrderLatency::new(latency_data, 0);
    let asset_type = LinearAsset::new(1.0);
    let queue_model = ProbQueueModel::new(PowerProbQueueFunc3::new(3.0));
    let price_action = HkPriceAction::new(vec![5*60*1000000000,15*60*1000000000,30*60*1000000000], vec![6,12,24]);

    let data = (20240908..20240913)
        .map(|date| DataSource::File(format!("examples/usdm/btcusdt_{date}.npz")))
        .collect();

    let hbt = Backtest::builder()
        .add_asset(
            L2AssetBuilder::new()
                .data(data)
                .latency_model(latency_model)
                .asset_type(asset_type)
                .fee_model(TradingValueFeeModel::new(CommonFees::new(-0.00005, 0.0007)))
                .exchange(ExchangeKind::NoPartialFillExchange)
                .queue_model(queue_model)
                .price_action(price_action)
                .depth(|| {
                    let mut depth = HashMapMarketDepth::new(0.1, 0.001);
                    depth.apply_snapshot(
                        &read_npz_file("examples/usdm/btcusdt_20240907_eod.npz", "data").unwrap(),
                    );
                    depth
                })
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();
    hbt
}

fn main() {
    tracing_subscriber::fmt::init();

    let relative_half_spread = 2.0;
    let relative_grid_interval = 1.0;
    let grid_num = 20;
    let min_grid_step = 0.1; // tick size
    let skew = relative_half_spread / grid_num as f64;
    let order_qty = 0.01;
    let max_position = grid_num as f64 * order_qty;

    let mut hbt = prepare_backtest();
    let mut recorder = BacktestRecorder::new(&hbt);
    gridtrading(
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
    recorder.to_csv("gridtrading", ".").unwrap();
}
