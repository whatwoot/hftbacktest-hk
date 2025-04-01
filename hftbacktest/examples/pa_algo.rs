use std::{collections::HashMap, fmt::Debug, ops::{Add, Div, Mul}};

use hftbacktest::prelude::*;

pub fn patrading<MD, I, R, PA>(
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
    let mut stop_loss = 0i64;
    let mut stop_profit = 0i64;
    let mut direction = 0;
    let mut direction_time = 0i64;
    let mut open_time = 0i64;
    let mut open_tick = 0i64;
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
        let best_ask_tick = depth.best_ask_tick();
        let best_bid_tick = depth.best_bid_tick();
        
        let orders = hbt.orders(0).clone();
        if depth.best_bid_tick() == INVALID_MIN || depth.best_ask_tick() == INVALID_MAX {
            // Market depth is incomplete.
            continue;
        }

        let price_action = hbt.price_action(0);
        // let klines = price_action.kines(5 * 60 * 1_000_000_000, 48);
        let (kmaps,last_open_time) = price_action.kmaps(5 * 60 * 1_000_000_000, 360);
        if kmaps.len() < 3 {
            continue;
        }
        let open_tim_str = nanos_to_ymdhms(last_open_time );
        // let short_emas = price_action.emas(5 * 60 * 1_000_000_000, 6, 24);
        // let mid_emas = price_action.emas(5 * 60 * 1_000_000_000, 12, 24);
        // let long_emas = price_action.emas(5 * 60 * 1_000_000_000, 24, 24);
        let swings = price_action.swings(24);
        if swings.len() < 5 {
            continue;
        }

        // println!("swings:: {:#?}", swings);        
        if position == 0.0 {
            trade_direction(&swings,&kmaps,last_open_time,&mut direction,&mut direction_time);

            let lastk = kmaps.get(&(last_open_time-5 * 60 * 1_000_000_000)).unwrap();//&klines[klines.len()-2];
            let newk = kmaps.get(&last_open_time).unwrap();// &klines[klines.len()-1]; 
            if direction == 1 { // long
                // 问题：开仓时，离低点太高了，直接就被反转了
                // 信号线上影线长，不能开仓
                // 阳线，delta > 0, 底部poc,大单/微单，需求堆积，且最低价大于前一低点
                
                let mut open = 0i32;
                if lastk.open_tick < lastk.close_tick {
                    open += 1;
                }
                if lastk.delta > 0.0 {
                    open += 1;
                }
                if lastk.poc_price < lastk.close_tick {
                    open += 1;
                }
                if lastk.top_sell_rate < 0.6 || lastk.top_sell_rate > 28.0 {
                    open += 1;
                }
                // 上影线>1/3,不开仓
                if lastk.high_tick - lastk.close_tick >= (lastk.high_tick - lastk.low_tick) / 3 {
                    open -= 1;
                }
                //需求堆积
                if open >= 3 && newk.close_tick > lastk.high_tick && !orders.contains_key(&((best_ask / tick_size) as u64)){
                    stop_price(&swings ,&kmaps, direction, best_ask_tick, last_open_time,open_time,open_tick, position, &mut stop_loss,&mut stop_profit);
                    hbt.submit_buy_order(
                        0,
                        (best_ask / tick_size).round() as u64,
                        best_ask,
                        order_qty,
                        TimeInForce::GTC,
                        OrdType::Market,
                        false,
                    ).unwrap();
                    println!("Long-open::{}:best_bid:{},best_ask:{},stop_loss:{},stop_profit:{}",open_tim_str,best_bid,best_ask,stop_loss,stop_profit);
                    direction = 0;
                    open_time = last_open_time;
                    open_tick = best_ask_tick;
                    
                }
            }else if direction == -1 {
                // 阴线，delta < 0, 顶部poc,大单/微单，供应堆积，且最高价低于前一高点
                let mut open = 0i32;
                if lastk.close_tick < lastk.open_tick {
                    open += 1;
                }
                if lastk.delta < 0.0 {
                    open += 1;
                }
                if lastk.poc_price > lastk.open_tick {
                    open += 1;
                }
                if lastk.top_buy_rate < 0.6 || lastk.top_buy_rate > 28.0 {
                    open += 1;
                }
                // 下影线>1/3,不开仓
                if lastk.close_tick - lastk.low_tick >= (lastk.high_tick - lastk.low_tick) / 3 {
                    open -= 1;
                }
                //供应堆积
                if open >= 3 && newk.close_tick < lastk.low_tick && !orders.contains_key(&((best_bid / tick_size) as u64)){
                    stop_price(&swings ,&kmaps, direction, best_ask_tick,last_open_time,open_time, open_tick, position, &mut stop_loss,&mut stop_profit);
                    hbt.submit_sell_order(
                        0,
                        (best_bid / tick_size).round() as u64,
                        best_bid,
                        order_qty,
                        TimeInForce::GTC,
                        OrdType::Market,
                        false,
                    ).unwrap();
                    
                    open_time = last_open_time;
                    direction = 0;
                    open_tick = best_bid_tick;
                    println!("Short-open::{}:best_bid:{},best_ask:{},stop_loss:{},stop_profit:{}",open_tim_str,best_bid,best_ask,stop_loss,stop_profit);
                }
            }
        }else if position > 0.0 { //止盈止损
            stop_price(&swings ,&kmaps, direction, best_bid_tick,last_open_time,open_time, open_tick, position, &mut stop_loss,&mut stop_profit);
            if best_bid_tick <= stop_loss  && !orders.contains_key(&((best_bid / tick_size) as u64)){ //卖出止损
                // 平仓指令：如果当前持仓为正，发送卖出指令
                hbt.submit_sell_order(
                    0,
                    (best_bid / tick_size).round() as u64,
                    best_bid,
                    position,
                    TimeInForce::GTC,
                    OrdType::Market,
                    false,
                ).unwrap();
                println!("Long-stop_loss::{}: best_bid:{},stop_loss:{},stop_profit:{}",open_tim_str,best_bid,stop_loss,stop_profit);
                stop_loss = 0;
                open_tick = 0;
            }
            if best_bid_tick >= stop_profit  && !orders.contains_key(&((best_bid / tick_size) as u64)){ //卖出止盈
                // 平仓指令：如果当前持仓为正，发送卖出指令
                hbt.submit_sell_order(
                    0,
                    (best_bid / tick_size).round() as u64,
                    best_bid,
                    position,
                    TimeInForce::GTX,
                    OrdType::Market,
                    false,
                ).unwrap();
                println!("Long-stop_profit::{}: best_bid:{},stop_loss:{},stop_profit:{}",open_tim_str,best_bid,stop_loss,stop_profit);
                stop_profit = 0;
                stop_loss = 0;
                open_tick = 0;
            }               
        }else{ //position < 0.0 空单止盈止损
            stop_price(&swings ,&kmaps, direction, best_ask_tick,last_open_time,open_time, open_tick, position, &mut stop_loss,&mut stop_profit);
            if best_ask_tick >= stop_loss  && !orders.contains_key(&((best_ask / tick_size) as u64)){ //买入止损
                // 平仓指令：如果当前持仓为负，发送买入指令
                hbt.submit_buy_order(
                    0,
                    (best_ask / tick_size).round() as u64,
                    best_ask,
                    -position, // 使用当前持仓数量的绝对值
                    TimeInForce::GTC,
                    OrdType::Market,
                    false,
                ).unwrap();
                println!("Short-stop_loss::{}: best_bid:{},stop_loss:{},stop_profit:{}",open_tim_str,best_ask,stop_loss,stop_profit);
                stop_loss = 0;
                open_tick = 0;
            }
            if best_ask_tick <= stop_profit  && !orders.contains_key(&((best_ask / tick_size) as u64)){ //买入止盈
                // 平仓指令：如果当前持仓为负，发送买入指令
                hbt.submit_buy_order(
                    0,
                    (best_ask / tick_size).round() as u64,
                    best_ask,
                    -position, // 使用当前持仓数量的绝对值
                    TimeInForce::GTC,
                    OrdType::Market,
                    false,
                ).unwrap();               
                println!("Short-stop_profit:{}: best_ask:{},stop_loss:{},stop_profit:{}",open_tim_str,best_ask,stop_loss,stop_profit);
                stop_profit = 0;
                stop_loss = 0;
                open_tick = 0;
            }
        }
       
    }
    Ok(())
}

// 交易方向
// 1. 大多数做空时，都已经离高点很远了
// 2. 有的做空时，前面已经创出新低了 
// 3. 单根K线太长，反复止损开单
// 4. 空单时，前一个收盘要高于前前一根的最低价（防止已经是最低价了，再空就失败了）
// 5. 寻找高点做空时，如果已经有2根收盘高于前高了，则考虑寻找低点做多
fn trade_direction(swings: &Vec<(i64,i64)>, bars:&HashMap<i64, &KLine>,last_open_time: i64, direction:&mut i32, direction_time:&mut i64){
    if last_open_time == *direction_time {
        return;
    }
    let trend = bars.get(&(swings[swings.len()-2].0)).unwrap().emas[1] - bars.get(&(swings[swings.len()-4].0)).unwrap().emas[1];
    //如果高点在ema24之下，只能空
    //如果低点在ema24之上，只能多
    if swings[swings.len()-1].1 > swings[swings.len()-2].1 { //最新波段点为高点
        if trend > -100 {//低点升高，或者W底，寻找低点，做多 // 应该要当前还没有创出新高？
            *direction = 1;
            *direction_time = last_open_time;
        }else{
            *direction = 0;
            *direction_time = last_open_time;
        }
        // let low_diff = swings[swings.len()-2].1 - swings[swings.len()-4].1;
        // if low_diff >= -200  {//低点升高，或者W底，寻找低点，做多 // 应该要当前还没有创出新高？
        //     *direction = 1;
        //     *direction_time = last_open_time;
        // }else{
        //     *direction = 0;
        //     *direction_time = last_open_time;
        // }
    }else{ //波段低点
        if trend < 100 {//高点降低，或者M顶，寻找高点，做空 //应该要当前还没有创出新低？
            *direction = -1;
            *direction_time = last_open_time;
        }else{
            *direction = 0;
            *direction_time = last_open_time;
        }
        // let high_diff = swings[swings.len()-4].1 - swings[swings.len()-2].1;
        // if high_diff >= -200 {//高点降低，或者M顶，寻找高点，做空 //应该要当前还没有创出新低？
        //     *direction = -1;
        //     *direction_time = last_open_time;
        //     // println!("Short::bar-time:{};swing-time:{}",nanos_to_ymdhms(bars.last().unwrap().open_time), nanos_to_ymdhms(*direction_time));
        // // }else if swings[swings.len()-1].1 > 0.0m{//低点高于24ema，寻找低点，做多
        // //     *direction = 1;
        // //     *direction_time = last_open_time;
        // }else{
        //     *direction = 0;
        //     *direction_time = last_open_time;
        // }

    }
}

// 止损止盈:参考最近的低点或者高点
// 1. 动态止盈，及时运动到平衡点，止盈位置太远，会错过很多机会
// 2. 连续下跌，快速到了止盈位置，止盈后继续下跌。应该要分析下降k线，是否遇到供应堆积（继续）/需求堆积（止盈），或者当前是否高潮（继续到微单/大单为止）
// 3. 及时移动到平衡位，出现底部大单/微单/poc/需求堆积，立即止损。
// 4. 突破最近的一系列高/低点，继续运动，不要止盈；而反向突破，及时止损。
fn stop_price(swings: &Vec<(i64,i64)>, bars:&HashMap<i64, &KLine>, direction:i32, tick:i64, tick_time:i64, open_time:i64, open_tick:i64, position:f64, stop_loss:&mut i64,stop_profit:&mut i64){
    let last_swing_direction = swings[swings.len()-1].1 - swings[swings.len()-2].1;
    let last_swing_heigh = if last_swing_direction > 0 {swings[swings.len()-1].1}else{swings[swings.len()-2].1};
    let last_swing_low = if last_swing_direction > 0 {swings[swings.len()-2].1}else{swings[swings.len()-1].1};

    let lastk = bars.get(&(tick_time - 5 * 60 * 1_000_000_000)).unwrap();

    if direction == 1 || position > 0.0{
        if *stop_loss == 0 && direction == 1{
            *stop_loss = tick - 1000;
            *stop_profit = tick + 2000;
            if *stop_loss < last_swing_low - 100 {
                *stop_loss = last_swing_low - 100;
            }
        }

        // 如果前一根K线的低点站上盈亏平衡，则止损点设为盈亏平衡
        if lastk.low_tick > open_tick.mul(10009).div(10000) 
        && open_tick.mul(10008).div(10000) > *stop_loss 
        && position > 0.0 {
            *stop_loss = open_tick.mul(10009).div(10000);
        }
        
        // // 如果低点升高，则把止损升高，止盈也升高
        // if *stop_loss < last_swing_low - 100 && position > 0.0 && tick_time - open_time > 5 * 60 * 1_000_000_000{
        //     println!("1.stop_profit:{};stop_loss:{};last_swing_low:{}",stop_profit,stop_loss,last_swing_low);
        //     *stop_loss = last_swing_low - 100;
        //     // *stop_profit += 2000;
        // }

        // 如果当前价格接近止盈，则动态提升止盈止损
        if tick >= *stop_profit - 50 && position > 0.0 {
            println!("2.stop_profit:{};stop_loss:{};tick:{}",stop_profit,stop_loss,tick);
            // *stop_loss = tick - 200;
            *stop_profit += 500;            
        }
        // // 如果前一根K线出现长上影线，移动止损
        // if (lastk.high_tick - lastk.close_tick >= (lastk.high_tick - lastk.low_tick) / 3  //上影线>1/3,close>open,但如果open>close,则更是阴线
        // || lastk.close_tick > lastk.low_tick + 20) //收盘接近低点
        // && (lastk.close_tick - tick).abs() < 5
        // && position > 0.0 {
        //     *stop_loss = tick - 200;
        // }

        let (max_price,min_price) = find_max_price(bars, 12*12, tick_time - 5 * 60 * 1_000_000_000);
        // 如果价格达到最近6小时的最高，则设置最新止损
        if tick > max_price && position > 0.0 {
            *stop_loss = tick - 200;
        }


    }else if direction == -1 || position < 0.0{
        if *stop_loss == 0 && direction == -1{
            *stop_loss = tick + 1000;
            *stop_profit = tick - 2000;
            if *stop_loss > last_swing_heigh + 100 {
                *stop_loss = last_swing_heigh + 100;
            }
        }

        // 如果前一根K线的高点低于盈亏平衡，则止损点设为盈亏平衡
        if lastk.high_tick < open_tick.mul(9991).div(10000) 
        && open_tick.mul(9992).div(10000) < *stop_loss 
        && position < 0.0 {
            *stop_loss = open_tick.mul(9991).div(10000);
        }

        // // 如果高点降低，则把止损下降，止盈相应下降
        // if *stop_loss > last_swing_heigh + 100 && position < 0.0 && tick_time - open_time > 5 * 60 * 1_000_000_000 {
        //     *stop_loss = last_swing_heigh + 100;
        //     // *stop_profit -= 2000;
        // }

        // 当前价格接近止盈，则动态提升止盈止损
        if tick <= *stop_profit + 50 && position < 0.0 {
            // *stop_loss = tick + 200;
            *stop_profit -=  500;
        }

        // // 如果前一根K线出现长下影线，提升止损
        // let lastk = bars.get(&(tick_time - 5 * 60 * 1_000_000_000)).unwrap();
        // if (lastk.close_tick - lastk.low_tick >= (lastk.high_tick - lastk.low_tick) / 3  //下影线>1/3,close<open,但如果open<close,则更是阳线
        // || lastk.close_tick > lastk.high_tick - 20 )//收盘接近高点
        // && (lastk.close_tick - tick).abs() < 5
        // && position < 0.0 {
        //     *stop_loss = tick + 200;
        // }
        
        let (_,min_price) = find_max_price(bars, 12*12, tick_time - 5 * 60 * 1_000_000_000);
        // 如果价格达到最近6小时的最低，则设置最新止损
        if tick < min_price && position < 0.0 {
            *stop_loss = tick + 200;
        }

    }
}

fn find_max_price(bars:&HashMap<i64, &KLine>, nums:usize, last_time:i64)->(i64,i64){
    let mut max_price = 0;
    let mut min_price = 0;
    for i in 0..nums{
        let k = bars.get(&(last_time - i as i64 * 5 * 60 * 1_000_000_000));
        match k {
            Some(k) => {
                if k.high_tick > max_price {
                    max_price = k.high_tick;
                }
                if k.low_tick < min_price {
                    min_price = k.low_tick;
                }
            },
            None => {
                return (max_price,min_price);
            }            
        }
    }
    (max_price,min_price)
}
