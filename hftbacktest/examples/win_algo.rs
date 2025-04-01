use std::{collections::HashMap, fmt::Debug, ops::Sub};

use hftbacktest::prelude::*;

/*
1. 如果某个价位卖交易量 > 1,之后连续5次卖家上升，然后第二个价位卖交易量 > 1（后续5次上升）,并且第二个价位卖价 > 第一个价位卖价，那么做多，止损点为第二价位-200个tick，止盈+1000个tick
2. 如果某个价格买交易量 > 1,之后连续5次买家下降，然后第二个价位买交易量 > 1（后续5次下降）,并且第二个价位买价 < 第一个价位买价，那么做空，止损点为第二价位+200个tick，止盈-1000个tick
3. 滑动止盈止损，突破万8，直接盈亏平衡，如果出现价位交易量 > 1，则更新止损位
*/
pub fn wintrading<MD, I, R, PA>(
    hbt: &mut I,
    recorder: &mut R,
    relative_half_spread: f64,
    relative_grid_interval: f64,
    grid_num: usize,
    min_grid_step: f64,
    skew: f64,
    order_qty: f64,
    max_position: f64,
) -> Result<(), i64>
where
    MD: MarketDepth,
    I: Bot<MD,PA>,
    <I as Bot<MD,PA>>::Error: Debug,
    R: Recorder,
    <R as Recorder>::Error: Debug,
    PA: PriceAction,
{
    let tick_size = hbt.depth(0).tick_size() as f64;
    // min_grid_step should be in multiples of tick_size.
    let min_grid_step = (min_grid_step / tick_size).round() * tick_size;
    let mut int = 0;
    let mut stop_loss = 0.0;
    let mut stop_profit = 0.0;
    let mut open_price = 0.0;//开仓价格


    let mut acc_buy_tick = 0i64;
    let mut below_buys = 0usize;//连续buy低于acc_buy_tick的次数,5次以上，做空
    let mut acc_sell_tick = 0i64;
    let mut above_sells = 0usize;//连续sell高于acc_sell_tick的次数,5次以上，做多
    let mut acc_height_1 = 0i64;
    let mut acc_low_1 = 0i64;


    let mut last_tick = 0i64;
    let mut last_tick_qty = 0.0;
    let mut last_tick_tiemstamp = 0i64;
    let mut last_side = Side::None;
    // Running interval in nanoseconds
    while hbt.elapse(100_000_000).unwrap() {
        int += 1;
        if int % 10 == 0 {
            // Records every 1-sec.
            recorder.record(hbt).unwrap();
        }

        let depth = hbt.depth(0);
        let position = hbt.position(0);

        let best_bid = depth.best_bid();
        let best_ask = depth.best_ask();
        let best_bid_tick = depth.best_bid_tick();
        let best_ask_tick = depth.best_ask_tick();
        
        let orders = hbt.orders(0).clone();
        if depth.best_bid_tick() == INVALID_MIN || depth.best_ask_tick() == INVALID_MAX {
            // Market depth is incomplete.
            continue;
        }

        let price_action = hbt.price_action(0);
        let (kmaps,last_open_time) = price_action.kmaps(5 * 60 * 1_000_000_000, 360);
        if kmaps.len() < 3 {
            continue;
        }
        let open_time = nanos_to_ymdhms(last_open_time );
        let mut last_height = 0i64;
        let mut last_low = i64::MAX;
        let findlen = if kmaps.len() > 10 {10} else {kmaps.len()};
        for i in 1..findlen {//最近5根K线的最高点和最低点
            let kline = kmaps.get(&(last_open_time - i as i64 * 5 * 60 * 1_000_000_000)).unwrap();
            if last_height < kline.high_tick {
                last_height = kline.high_tick;
            }
            if last_low > kline.low_tick {
                last_low = kline.low_tick;
            }
        }

        let (tick,tick_qty,tick_tiemstamp,side) = price_action.last_acc_trades();
        
        if last_tick_tiemstamp != 0 && last_tick_tiemstamp != tick_tiemstamp {
            if last_side == Side::Buy {
                if last_tick_qty > 5.0 {
                    acc_buy_tick = last_tick;
                    below_buys = 0;//与下面的else重复赋值，避免添加其它逻辑时遗漏
                }
                if acc_buy_tick > 0 && last_tick < acc_buy_tick {
                    below_buys += 1;
                }else{
                    below_buys = 0;
                }
                if acc_height_1 == 0 && below_buys >= 5{
                    acc_height_1 = acc_buy_tick;
                }
            }else if last_side == Side::Sell {
                if last_tick_qty > 5.0 {
                    acc_sell_tick = last_tick; 
                    above_sells = 0;  
                }
                if acc_sell_tick > 0 && last_tick > acc_sell_tick {
                    above_sells += 1;
                }else{
                    above_sells = 0;
                }
                if acc_low_1 == i64::MAX && above_sells >= 5{
                    acc_low_1 = acc_sell_tick;
                }
            }
        }
        last_tick = tick;
        last_tick_qty = tick_qty;
        last_tick_tiemstamp = tick_tiemstamp;
        last_side = side;       

        // println!("swings:: {:#?}", swings);        
        if position == 0.0 {
            // long
            if above_sells >= 5 
            && best_ask_tick >= acc_sell_tick 
            && acc_sell_tick <= last_low 
            && acc_sell_tick > acc_low_1
            && !orders.contains_key(&((best_ask / tick_size) as u64)){
                stop_loss = (acc_sell_tick - 200) as f64 * tick_size;
                stop_profit = stop_loss + 1000.0 * tick_size;
                hbt.submit_buy_order(
                    0,
                    (best_ask / tick_size).round() as u64,
                    best_ask,
                    order_qty,
                    TimeInForce::GTC,
                    OrdType::Market,
                    false,
                ).unwrap();
                open_price = best_ask;
                
                println!("Long-open::{}:best_bid:{},best_ask:{},stop_loss:{},stop_profit:{},acc_sell_tick:{},above_sells:{},last_low:{}",open_time,best_bid,best_ask,stop_loss,stop_profit,acc_sell_tick,above_sells,last_low);
                acc_sell_tick = 0;
                above_sells = 0;
                acc_buy_tick = 0;
                below_buys = 0;
            }

            // short
            if below_buys >= 5 
            && best_bid_tick <= acc_buy_tick 
            && acc_buy_tick >= last_height 
            && acc_buy_tick < acc_height_1
            && !orders.contains_key(&((best_bid / tick_size) as u64)){
                stop_loss = (acc_buy_tick + 200) as f64 * tick_size;
                stop_profit = stop_loss - 1000.0 * tick_size;
                hbt.submit_sell_order(
                    0,
                    (best_bid / tick_size).round() as u64,
                    best_bid,
                    order_qty,
                    TimeInForce::GTC,
                    OrdType::Market,
                    false,
                ).unwrap();
                open_price = best_bid;
                // acc_sell_tick = 0;
                // above_sells = 0;
                // acc_buy_tick = 0;
                // below_buys = 0;
                println!("Short-open::{}:best_bid:{},best_ask:{},stop_loss:{},stop_profit:{},acc_buy_tick:{},below_buys:{}",open_time,best_bid,best_ask,stop_loss,stop_profit,acc_buy_tick,below_buys);
                acc_sell_tick = 0;
                above_sells = 0;
                acc_buy_tick = 0;
                below_buys = 0;
            }
        } else { //滑动止盈止损
            if position > 0.0 {
                // 滑动止损
                if open_price > 0.0 && best_bid > open_price + 50.0 {//即时跳过手续费门槛
                    stop_loss = open_price + 40.0;
                    // stop_profit = stop_loss + 1000.0 * tick_size;
                }
                if acc_sell_tick > 0 && acc_sell_tick as f64 * tick_size > stop_loss+200.0 && above_sells > 2 {
                    stop_loss = (acc_sell_tick - 200) as f64 * tick_size;
                    // stop_profit = stop_loss + 1000.0 * tick_size;

                    acc_sell_tick = 0;
                    above_sells = 0;
                }
                if acc_buy_tick >0 && last_tick > acc_buy_tick && acc_buy_tick as f64 * tick_size  > stop_loss+200.0{
                    stop_loss = (acc_buy_tick - 200) as f64 * tick_size;
                    // stop_profit = stop_loss + 1000.0 * tick_size;
                }

                // // 如果走势反转为空，立即止损
                // if acc_buy_tick > 0 && below_buys >=5 {
                //     stop_loss = best_bid;
                // }

                // 止盈
                if best_bid >= stop_profit && !orders.contains_key(&((best_bid / tick_size) as u64)) {
                    hbt.submit_sell_order(
                        0,
                        (best_bid / tick_size).round() as u64,
                        best_bid,
                        position,
                        TimeInForce::GTC,
                        OrdType::Market,
                        false,
                    ).unwrap();
                    println!("Long-stop_profit::{}: best_ask:{},stop_profit:{}",open_time,best_ask,stop_profit);
                }

                // 止损
                if best_bid <= stop_loss && !orders.contains_key(&((best_bid / tick_size) as u64)) {
                    hbt.submit_sell_order(
                        0,
                        (best_bid / tick_size).round() as u64,
                        best_bid,
                        position,
                        TimeInForce::GTC,
                        OrdType::Market,
                        false,
                    ).unwrap();
                    println!("Long-stop_loss::{}: best_bid:{},stop_loss:{}",open_time,best_bid,stop_loss);
                }
            } else {
                // 滑动止损
                if open_price > 0.0 && best_ask < open_price - 50.0 {//即时跳过手续费门槛
                    stop_loss = open_price - 40.0;
                    // stop_profit = stop_loss - 1000.0 * tick_size;
                }
                if acc_buy_tick > 0 && acc_buy_tick as f64 * tick_size < stop_loss-200.0 && below_buys > 2 {
                    stop_loss = (acc_buy_tick + 200) as f64 * tick_size;
                    // stop_profit = stop_loss - 1000.0 * tick_size;

                    acc_buy_tick = 0;
                    below_buys = 0;
                }
                if acc_sell_tick >0 && last_tick < acc_sell_tick && acc_sell_tick as f64 * tick_size  < stop_loss - 200.0{
                    stop_loss = (acc_sell_tick + 200) as f64 * tick_size;
                    // stop_profit = stop_loss + 1000.0 * tick_size;
                }
                // // 如果走势反转为多，立即止损
                // if acc_sell_tick > 0 && above_sells >=5 {
                //     stop_loss = best_ask;
                // }

                //止盈
                if best_ask <= stop_profit && !orders.contains_key(&((best_ask / tick_size) as u64)) {
                    hbt.submit_buy_order(
                        0,
                        (best_ask / tick_size).round() as u64,
                        best_ask,
                        -position,
                        TimeInForce::GTC,
                        OrdType::Market,
                        false,
                    ).unwrap();
                    println!("Short-stop_profit::{}: best_bid:{},stop_profit:{}",open_time,best_ask,stop_profit);
                }

                //止损
                if best_ask >= stop_loss && !orders.contains_key(&((best_ask / tick_size) as u64)) {
                    hbt.submit_buy_order(
                        0,
                        (best_ask / tick_size).round() as u64,
                        best_ask,
                        -position,
                        TimeInForce::GTC,
                        OrdType::Market,
                        false,
                    ).unwrap();
                    println!("Short-stop_loss::{}: best_bid:{},stop_loss:{}",open_time,best_ask,stop_loss);
                }
            }
        }

        if below_buys >= 5 && acc_buy_tick != acc_height_1 {
            acc_height_1 = 0;
        }
        if above_sells >= 5 && acc_sell_tick != acc_low_1 {
            acc_low_1 = i64::MAX;
        }
       
    }
    Ok(())
}

