use std::collections::HashMap;

use chrono::{Datelike, NaiveDateTime, TimeZone, Timelike, Utc};
use chrono_tz::Asia::Shanghai;



pub use hkpriceaction::HkPriceAction;

use crate::types::Side;
pub mod hkpriceaction;
mod imbalance;
pub use imbalance::Imbalance;

#[derive(Debug, Clone, Default)]
pub struct KLine {
    pub open_tick: i64,
    pub high_tick: i64,
    pub low_tick: i64,
    pub close_tick: i64,
    pub buy_volume: f64,
    pub sell_volume: f64,
    pub open_time: i64,
    pub close_time: i64, 

    // OrderFlow
    pub poc_price: i64,
    pub poc_qty: f64,
    pub top_buy_rate: f64,
    pub top_sell_rate: f64,
    pub delta: f64,
    pub max_delta: f64,
    pub min_delta: f64,

    pub emas: Vec<i64>,
}

impl KLine {
    pub fn new(tick:i64, qty:f64, timestamp:i64,interval:i64,side:Side, ema_nums:usize) -> Self{
        let tick_qty = qty;
        let mut emas = Vec::with_capacity(ema_nums);
        for _ in 0..ema_nums{
            emas.push(0);
        }
        Self{
            open_tick: tick,
            high_tick: tick,
            low_tick: tick,
            close_tick: tick,
            buy_volume:  if side == Side::Buy { tick_qty } else {0.0},
            sell_volume: if side == Side::Sell { tick_qty } else {0.0},
            open_time: timestamp / interval * interval,
            close_time: timestamp / interval * interval + interval - 1,

            poc_price: 0,
            poc_qty: 0.0,
            top_buy_rate: 0.0,//0。5-28
            top_sell_rate: 0.0,//0。5-28
            delta: if side == Side::Buy { tick_qty } else {0.0 - tick_qty},
            max_delta: if side == Side::Buy { tick_qty } else {0.0},
            min_delta: if side == Side::Sell { 0.0 - tick_qty } else {0.0},
            emas,
            }
        }

    pub fn update(&mut self, tick:i64, qty:f64, timestamp:i64, side:Side){
        if timestamp < self.open_time || timestamp > self.close_time{
            return;
        }
        let tick_qty = qty;
        self.high_tick = self.high_tick.max(tick);
        self.low_tick = self.low_tick.min(tick);
        self.close_tick = tick;
        self.buy_volume +=  if side == Side::Buy { tick_qty } else {0.0};
        self.sell_volume += if side == Side::Sell { tick_qty } else {0.0};
        self.delta = self.buy_volume - self.sell_volume;
        self.max_delta = self.max_delta.max(self.delta);
        self.min_delta = self.min_delta.min(self.delta);
    }
}

// impl Default for KLine {
//     fn default() -> Self {
//         Self {
//             open_tick: 0.0,
//             high_tick: 0.0,
//             low_tick: 0.0,
//             close_tick: 0.0,
//             volume: 0.0,
//             open_time: 0,
//             close_time: 0,

//             poc: 0.0,
//             top_buy_rate: 0.0,
//             top_sell_rate: 0.0,
//             delta: 0.0,
//             max_delta: 0.0,
//             min_delta: 0.0,
//             delta_volume: 0.0,
//         }
//     }
// }

// 第一根K线如果是阳线，高点>shortema>midema>longema,则为多头,寻找高点
// 第一根K线如果是阴线，shortema<midema<longema<低点,则为空头,寻找低点
// 其它K线，忽略，直到有明确的多或空
// 找到第一根后，赋值cur_tick,cur_opentime,high_or_low
// 每根K线结束，判断high_or_low方向是否有新高或新低，如果没有，judge_time+1，如果有，judge_time=0，更新cur_tick,cur_opentime,high_or_low
#[derive(Debug, Clone, Default)]
pub struct Swings {
    swing_hights: Vec<(i64, i64)>,//opentime,price_tick
    swing_lows: Vec<(i64, i64)>,//opentime,price_tick   
    ext_tick: i64,
    ext_opentime: i64,
    high_or_low: i8, //1:high, -1:low
    judge_time: i8, //如果连续5次没有新高或新低，则判断结束，把cur_tick,cur_opentime加入swing_hights或swing_lows
    wait_kines: Vec<(i64, i64, i64)>,//opentime,high_tick,low_tich
    last_high_low: i64, //上一个高点的低值,在计算新的低点时，要求新低点必须低于last_high_low-100
    last_low_high: i64, //上一个低点的高值，在计算新的高点时，要求新高点必须高于last_low_high+100
}

pub trait PriceAction {
    fn order_flow(&mut self, px:f64, tick_size:f64, qty:f64, timestamp:i64, side:Side);
    // fn kines(&self, intevrval:i64, nums:usize) -> &[KLine];
    // fn emas(&self, intevrval:i64, exporid:i64, nums:i32) -> &[f64];
    fn swings(&self, nums:usize) -> Vec<(i64, i64)>;
    fn kmaps(&self, intevrval:i64, nums:usize) -> (HashMap<i64, &KLine>, i64);
    fn last_acc_trades(&self) -> (i64, f64, i64, Side);
}

#[derive(Debug, Clone)]
pub struct FixedSizeEma<T> {
    max_size: usize,
    data: Vec<T>,
}

impl<T> FixedSizeEma<T> {
    fn new(max_size: usize) -> Self {
        Self {
            max_size,
            data: Vec::new(),
        }
    }

    fn push(&mut self, value: T) {
        if self.data.len() == self.max_size {
            self.data.remove(0);
        }
        self.data.push(value);
    }

    fn get(&self) -> &Vec<T> {
        &self.data
    }

    fn get_mut(&mut self) -> &mut Vec<T> {
        &mut self.data
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn last(&self) -> Option<&T> {
        self.data.last()
    }

    fn last_mut(&mut self) -> Option<&mut T> {
        self.data.last_mut()
    }

    fn get_second_last_value(&self) -> Option<&T> {
        if self.data.len() < 2 {
            return None;
        }
        self.data.get(self.data.len() - 2)
    }
}

pub fn nanos_to_ymdhms(nanos: i64) -> String {
    // Convert nanoseconds to seconds and nanoseconds
    let secs = nanos / 1_000_000_000;
    let nsecs = (nanos % 1_000_000_000) as u32;

    // Create a NaiveDateTime from seconds and nanoseconds
    let naive_datetime = NaiveDateTime::from_timestamp(secs, nsecs);

    // // Convert NaiveDateTime to DateTime<Utc>
    // let datetime = Utc.from_utc_datetime(&naive_datetime);

    // Convert NaiveDateTime to DateTime<Shanghai>
    let datetime = Shanghai.from_utc_datetime(&naive_datetime);

    // Format the DateTime to a string
    datetime.format("%m-%d %H:%M:%S%.9f").to_string()

    // Extract year, month, and day
    // let year = datetime.year();
    // let month = datetime.month();
    // let day = datetime.day();
    // let hour = datetime.hour();
    // let minute = datetime.minute();
    // let second = datetime.second();

    // (year, month, day, hour, minute, second)
}


