use std::{
    fs::File,
    io::{BufWriter, Error, Write},
    path::Path,
};

use hftbacktest_derive::NpyDTyped;
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    backtest::data::{POD, write_npy},
    depth::MarketDepth,
    prelude::PriceAction, 
    types::{Bot, Recorder},
};

#[repr(C)]
#[derive(NpyDTyped)]
struct Record {
    timestamp: i64,
    price: f64,
    position: f64,
    balance: f64,
    fee: f64,
    num_trades: i64,
    trading_volume: f64,
    trading_value: f64,
}

unsafe impl POD for Record {}

/// Provides recording of the backtesting strategy's state values, which are needed to compute
/// performance metrics.
pub struct BacktestRecorder {
    values: Vec<Vec<Record>>,
}

impl Recorder for BacktestRecorder {
    type Error = Error;

    fn record<MD, I,PA>(&mut self, hbt: &mut I) -> Result<(), Self::Error>
    where
        MD: MarketDepth,
        PA: PriceAction,
        I: Bot<MD,PA>,
    {
        let timestamp = hbt.current_timestamp();
        for asset_no in 0..hbt.num_assets() {
            let depth = hbt.depth(asset_no);
            let mid_price = (depth.best_bid() + depth.best_ask()) / 2.0;
            let state_values = hbt.state_values(asset_no);
            let values = unsafe { self.values.get_unchecked_mut(asset_no) };
            values.push(Record {
                timestamp,
                price: mid_price,
                balance: state_values.balance,
                position: state_values.position,
                fee: state_values.fee,
                trading_volume: state_values.trading_volume,
                trading_value: state_values.trading_value,
                num_trades: state_values.num_trades,
            });
        }
        Ok(())
    }
}

impl BacktestRecorder {
    /// Constructs an instance of `BacktestRecorder`.
    pub fn new<I, MD, PA>(hbt: &I) -> Self
    where
        MD: MarketDepth,
        PA: PriceAction,
        I: Bot<MD,PA>,
    {
        Self {
            values: {
                let mut vec = Vec::with_capacity(hbt.num_assets());
                for _ in 0..hbt.num_assets() {
                    vec.push(Vec::new());
                }
                vec
            },
        }
    }

    /// Saves record data into a CSV file at the specified path. It creates a separate CSV file for
    /// each asset, with the filename `{prefix}_{asset_no}.csv`.
    /// The columns are `timestamp`, `mid`, `balance`, `position`, `fee`, `trade_num`,
    /// `trade_amount`, `trade_qty`.
    pub fn to_csv<Prefix, P>(&self, prefix: Prefix, path: P) -> Result<(), Error>
    where
        Prefix: AsRef<str>,
        P: AsRef<Path>,
    {
        let prefix = prefix.as_ref();
        for (asset_no, values) in self.values.iter().enumerate() {
            let file_path = path.as_ref().join(format!("{prefix}{asset_no}.csv"));
            let mut file = BufWriter::new(File::create(file_path)?);
            writeln!(
                file,
                "timestamp,balance,position,fee,trading_volume,trading_value,num_trades,price",
            )?;
            for Record {
                timestamp,
                balance,
                position,
                fee,
                trading_volume,
                trading_value,
                num_trades,
                price: mid_price,
            } in values
            {
                writeln!(
                    file,
                    "{},{},{},{},{},{},{},{}",
                    timestamp,
                    balance,
                    position,
                    fee,
                    trading_volume,
                    trading_value,
                    num_trades,
                    mid_price,
                )?;
            }
        }
        Ok(())
    }

    pub fn to_npz<P>(&self, path: P) -> Result<(), Error>
    where
        P: AsRef<Path>,
    {
        let file = File::create(path)?;

        let mut zip = ZipWriter::new(file);

        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::DEFLATE)
            .compression_level(Some(9));

        for (asset_no, values) in self.values.iter().enumerate() {
            zip.start_file(format!("{asset_no}.npy"), options)?;
            write_npy(&mut zip, values)?;
        }

        zip.finish()?;
        Ok(())
    }

    
    /// 统计 self.values 中的状态数据，返回累计收益、最大回撤和夏普比率
    ///
    /// - 累计收益：最后一个记录的 balance 与第一个记录的 balance 之间的差值
    /// - 最大回撤：资产历史上 balance 相对于历史最高点的最大降低幅度
    /// - 夏普比率：用连续周期的收益率计算均值和标准差，夏普比率 = (均值 / 标准差) * sqrt(n)
    pub fn stats(&self, asset_no:usize) -> Result<(f64, f64, f64, f64, f64, f64), Error> {

        if self.values.len() < asset_no {
            return Ok((0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        }

        let asset_records = &self.values[asset_no];

        if asset_records.len() < 2 {
            return Ok((0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        }

        // 累计收益：最后记录 balance 与第一个记录的 balance 差值
        let first_record = asset_records.first().unwrap();
        let last_record = asset_records.last().unwrap();

        let initial = first_record.balance + first_record.position * first_record.price - first_record.fee;
        let final_equity = last_record.balance + last_record.position * last_record.price - last_record.fee;
        let cum_return = final_equity - initial;
        let fee = last_record.fee;

        // 最大回撤：依次遍历记录，更新历史峰值，计算从峰值的回撤，取最大值
        let mut trough = initial;
        let mut peak = initial;
        let mut max_dd = 0.0;
        for rec in asset_records {
            let rec_equity = rec.balance + rec.position * rec.price - rec.fee;
            if rec_equity > peak {
                peak = rec_equity;
            }
            let dd = peak - rec_equity;
            if dd > max_dd {
                max_dd = dd;
            }
            if rec_equity < trough {
                trough = rec_equity;
            }
        }

        // 先计算相邻记录之间的收益率（(当前 balance - 前一 balance) / 前一 balance），然后计算这些收益率的均值与标准差。
        // 若标准差不为 0，则夏普比率用均值除以标准差并乘以收益率数量的平方根来衡量。
        // 计算连续收益率数组
        let mut returns = Vec::new();
        for window in asset_records.windows(2) {
            let prev = window[0].balance + window[0].position * window[0].price - window[0].fee;
            let curr = window[1].balance + window[1].position * window[1].price - window[1].fee;
            if prev != 0.0 {
                let r = (curr - prev) / prev;
                returns.push(r);
            }
        }
        // 计算夏普比率（假设无风险利率为0）
        let mut sharpe = 0.0;
        let n = returns.len();
        if n > 0 {
            let mean = returns.iter().sum::<f64>() / (n as f64);
            let variance = returns.iter()
                .map(|r| (r - mean).powi(2))
                .sum::<f64>() / (n as f64);
            let std = variance.sqrt();
            sharpe = if std != 0.0 {
                mean / std * (n as f64).sqrt()
            } else {
                0.0
            };
        }

        Ok((peak, cum_return, trough, max_dd, sharpe, fee))
    }
}
