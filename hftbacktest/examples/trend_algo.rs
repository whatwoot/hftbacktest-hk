use std::{collections::HashMap, fmt::Debug, ops::{Add, Div, Mul, Sub}};

use chrono_tz::America::New_York;
use hftbacktest::prelude::*;

pub fn trendtrading<MD, I, R, PA>(
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
    const K_TIME:i64 = 5 * 60 * 1_000_000_000;
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
    let mut position_rec = 0.0;
    // Running interval in nanoseconds
    while hbt.elapse(100_000_000).unwrap() {
        int += 1;
        if int % 10 == 0 {
            // Records every 1-sec.
            recorder.record(hbt).unwrap();
        }

        let depth = hbt.depth(0);
        let position = hbt.position(0);
        if position != position_rec {
            position_rec = position;
            println!("----->position:{}",position);
        }
        let state = hbt.state_values(0);
        let best_bid = depth.best_bid();
        let best_ask = depth.best_ask();
        let best_ask_tick = depth.best_ask_tick();
        let best_bid_tick = depth.best_bid_tick();
        let equity = state.balance.add(position.mul(best_bid.add(best_ask).div(2.0))).sub(state.fee);
        
        let orders = hbt.orders(0).clone();
        if depth.best_bid_tick() == INVALID_MIN || depth.best_ask_tick() == INVALID_MAX {
            // Market depth is incomplete.
            println!("+++++++++Market depth is incomplete.");
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

        //最近10根K线的最低最高，突破最高K线的最低点，开空;突破最低K线的最高点，开多
        let (max_price,max_price_time,min_price,min_price_time) = find_max_price(&kmaps, 3, last_open_time - K_TIME);
        let last_swing_time = swings[swings.len()-1].0;       
        trade_direction(&swings,&kmaps,last_open_time,&mut direction,&mut direction_time);
        if position == 0.0 {
            let lastk = kmaps.get(&(last_open_time-5 * 60 * 1_000_000_000)).unwrap();//&klines[klines.len()-2];
            let newk = kmaps.get(&last_open_time).unwrap();// &klines[klines.len()-1]; 
            if direction >= 1 { // long
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
                // todo

                // 突破最近低点K线的最高点，开多
                if !orders.contains_key(&((best_ask / tick_size) as u64)) 
                && (direction == 2 
                    || (best_ask_tick > kmaps.get(&min_price_time).unwrap().high_tick + 50 
                        && newk.open_tick < kmaps.get(&min_price_time).unwrap().high_tick
                        && newk.open_time > last_swing_time + 5 * 5 * 60 * 1_000_000_000)
                ){
                    
                // if open >= 3 && newk.close_tick > lastk.high_tick && !orders.contains_key(&((best_ask / tick_size) as u64)){
                    let (_,_,min_stop_price,_) = find_max_price(&kmaps, 3*12, last_open_time);//最近3小时最低最高价
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
                    stop_loss = best_ask_tick - 5000;    
                    // stop_loss = if min_stop_price > stop_loss.add(1000) {min_stop_price.sub(1000)} else {stop_loss};
                    stop_profit = best_ask_tick + 100000;
                    println!("Long-open::{}:best_bid:{:.1},best_ask:{:.1},stop_loss:{},stop_profit:{},direction:{},equity:{}",open_tim_str,best_bid,best_ask,stop_loss,stop_profit,direction,equity);
                    direction = 0;
                    open_time = last_open_time;
                    open_tick = best_ask_tick;
                    
                }
            }else if direction <= -1 {
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

                // 突破最近高点K线的最低点，开空
                if !orders.contains_key(&((best_bid / tick_size) as u64))
                && (direction == -2 
                    || (best_bid_tick < kmaps.get(&max_price_time).unwrap().low_tick - 50 
                        && newk.open_tick > kmaps.get(&max_price_time).unwrap().low_tick
                        && newk.open_time > last_swing_time + 5 * 5 * 60 * 1_000_000_000)
                ){
                // if open >= 3 && newk.close_tick < lastk.low_tick && !orders.contains_key(&((best_bid / tick_size) as u64)){
                    let (max_stop_price,_,_,_) = find_max_price(&kmaps, 3*12, last_open_time);//最近3小时最低最高价
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
                    stop_loss = best_ask_tick + 5000;
                    // stop_loss = if max_stop_price < stop_loss.sub(1000) {max_stop_price.add(1000)} else {stop_loss};
                    stop_profit = best_ask_tick - 100000;
                    println!("Short-open::{}:best_bid:{:.1},best_ask:{:.1},stop_loss:{},stop_profit:{}:max_price:{},direction:{},equity:{}",open_tim_str,best_bid,best_ask,stop_loss,stop_profit,max_price,direction,equity);
                    
                    open_time = last_open_time;
                    direction = 0;
                    open_tick = best_bid_tick;
                }
            }
        }else if position > 0.0 { //止盈止损
            stop_price(&swings ,&kmaps, direction, best_bid_tick,last_open_time,open_time, open_tick, position, &mut stop_loss,&mut stop_profit);
            if (direction == -2 || best_bid_tick <= stop_loss || best_bid_tick >= stop_profit) && direction != 2 && !orders.contains_key(&((best_bid / tick_size) as u64)){ //卖出止损
                // 平仓指令：如果当前持仓为正，发送卖出指令
                hbt.submit_sell_order(
                    0,
                    (best_bid / tick_size).round() as u64,
                    best_bid,
                    position.mul(1.0),
                    TimeInForce::GTC,
                    OrdType::Market,
                    false,
                ).unwrap();
                println!("Long-stop::{}: best_bid:{},stop_loss:{},stop_profit:{},equity:{}",open_tim_str,best_bid,stop_loss,stop_profit,equity);
                stop_loss = 0;
                open_tick = 0;
                open_time = i64::MAX;

                // stop_loss = best_bid_tick + 5000;
                // stop_profit = best_bid_tick - 100000;                
                // open_time = last_open_time;
                // direction = 0;
                // open_tick = best_bid_tick;
            }
            // if (direction == -2 || best_bid_tick >= stop_profit)  && !orders.contains_key(&((best_bid / tick_size) as u64)){ //卖出止盈
            //     // 平仓指令：如果当前持仓为正，发送卖出指令
            //     hbt.submit_sell_order(
            //         0,
            //         (best_bid / tick_size).round() as u64,
            //         best_bid,
            //         position,
            //         TimeInForce::GTX,
            //         OrdType::Market,
            //         false,
            //     ).unwrap();
            //     println!("Long-stop_profit::{}: best_bid:{},stop_loss:{},stop_profit:{},balance:{}",open_tim_str,best_bid,stop_loss,stop_profit,hbt.state_values(0).balance);
            //     stop_profit = 0;
            //     stop_loss = 0;
            //     open_tick = 0;
            //     open_time = i64::MAX;
            // }               
        }else{ //position < 0.0 空单止盈止损
            stop_price(&swings ,&kmaps, direction, best_ask_tick,last_open_time,open_time, open_tick, position, &mut stop_loss,&mut stop_profit);
            if (direction == 2 || best_ask_tick >= stop_loss || best_ask_tick <= stop_profit) && direction != -2  && !orders.contains_key(&((best_ask / tick_size) as u64)){ //买入止损
                // 平仓指令：如果当前持仓为负，发送买入指令
                hbt.submit_buy_order(
                    0,
                    (best_ask / tick_size).round() as u64,
                    best_ask,
                    -position.mul(1.0), // 使用当前持仓数量的绝对值
                    TimeInForce::GTC,
                    OrdType::Market,
                    false,
                ).unwrap();
                println!("Short-stop::{}: best_bid:{},stop_loss:{},stop_profit:{},equity:{}",open_tim_str,best_ask,stop_loss,stop_profit,equity);
                stop_loss = 0;
                open_tick = 0;
                open_time = i64::MAX;

                // stop_loss = best_ask_tick - 5000;
                // stop_profit = best_ask_tick + 100000;                
                // open_time = last_open_time;
                // direction = 0;
                // open_tick = best_ask_tick;
            }
            // if (direction == 2 || best_ask_tick <= stop_profit)  && !orders.contains_key(&((best_ask / tick_size) as u64)){ //买入止盈
            //     // 平仓指令：如果当前持仓为负，发送买入指令
            //     hbt.submit_buy_order(
            //         0,
            //         (best_ask / tick_size).round() as u64,
            //         best_ask,
            //         -position, // 使用当前持仓数量的绝对值
            //         TimeInForce::GTC,
            //         OrdType::Market,
            //         false,
            //     ).unwrap();               
            //     println!("Short-stop_profit:{}: best_ask:{},stop_loss:{},stop_profit:{},balance:{}",open_tim_str,best_ask,stop_loss,stop_profit,hbt.state_values(0).balance);
            //     stop_profit = 0;
            //     stop_loss = 0;
            //     open_tick = 0;
            //     open_time = i64::MAX;
            // }
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
    // *direction = 0;
    let newk = bars.get(&last_open_time).unwrap();
    // let trend_1 = (bars.get(&(swings[swings.len()-1].0)).unwrap().emas[1] - bars.get(&(swings[swings.len()-3].0)).unwrap().emas[1]) * 1000 / bars.get(&(swings[swings.len()-3].0)).unwrap().emas[1];
    // let trend_2 = (bars.get(&(swings[swings.len()-2].0)).unwrap().emas[1] - bars.get(&(swings[swings.len()-4].0)).unwrap().emas[1]) * 1000 / bars.get(&(swings[swings.len()-4].0)).unwrap().emas[1];
    let trend_1 = (swings[swings.len()-1].1 - swings[swings.len()-3].1) * 1000 / swings[swings.len()-3].1;
    let trend_2 = (swings[swings.len()-2].1 - swings[swings.len()-4].1) * 1000 / swings[swings.len()-4].1;
    let (max_price,max_price_time,min_price,min_price_time) = find_max_price(bars, 8*12, last_open_time);//最近6小时最低最高价
    // if max_price_time > min_price_time && newk.emas[2] > newk.emas[0]{
    //     *direction = -1;
    //     *direction_time = last_open_time;
    // }else if min_price_time > max_price_time  && newk.emas[0] > newk.emas[2]{
    //     *direction = 1;
    //     *direction_time = last_open_time;
    // }else{
    //     *direction = 0;
    //     *direction_time = last_open_time;
    // }
    // 如果是高点，寻找前第3个低点，是否是最近6小时最低点，如果是，则寻找低点做多
    // {
    //     let last_bars = (newk.open_time - swings[swings.len()-1].0) / (5 * 60 * 1_000_000_000);
    //     let (max_price,max_price_time,min_price,min_price_time) = find_max_price(bars, last_bars as usize, last_open_time);//最近6小时最低最高价
    //     if swings[swings.len()-1].1 > swings[swings.len()-2].1 //高点
    //     && min_price > swings[swings.len()-2].1 //低点高于前低
    //     // && trend_1 >= 1 //高点上升
    //     && trend_2 >= 1{ //低点上升
    //         *direction = 1;
    //         *direction_time = last_open_time;        
    //     } else if  swings[swings.len()-1].1 < swings[swings.len()-2].1 
    //     && max_price < swings[swings.len()-2].1 //高点低于前高
    //     // && trend_1 <= -1 //低点下降
    //     && trend_2 <= -1{ //高点下降
    //         *direction = -1;
    //         *direction_time = last_open_time;
    //     } else{ //震荡趋势        
    //         *direction = 0;
    //         *direction_time = last_open_time;
    //     }
    // }

    {
        let (max_price,max_price_time,min_price,min_price_time) = find_max_price(bars, 6*12, last_open_time);//最近6小时最低最高价
        //最近最高，并且连续5根阴线，立即做空
        if max_price_time > min_price_time && *direction != -2 {
            // 最高点之前必须有一波高潮，前2个小时运动超过500,否则不是最终的高点
            let (pre_max_price,_,pre_min_price,_) = find_max_price(bars, 2*12, if last_open_time <= max_price_time.add(2*5*60*1_000_000_000) {last_open_time}else{max_price_time.add(2*5*60*1_000_000_000)});
            if pre_max_price < pre_min_price.add(6000) {
                return;
            }
            let (continue_ups,continue_ups_time,continue_downs,continue_downs_time) = stats_continus_kline(bars, max_price_time, last_open_time - 5 * 60 * 1_000_000_000);
            // println!("1111,continue_ups:{},continue_downs:{},starttime:{};endtime:{}",continue_ups,continue_downs,nanos_to_ymdhms(max_price_time),nanos_to_ymdhms(last_open_time));
            if continue_downs >= 5 {
                let (swing_low_time,swing_low) = find_max_swing_next(swings, continue_downs_time, -1);
                if continue_downs >= 10 || (swing_low > 0 && newk.close_tick < swing_low) {
                    *direction = -2;
                    *direction_time = last_open_time;
                    //超过最高点2小时后，是否还执行这条规则？
                    println!("-----111::max:starttime:{};endtime:{}",nanos_to_ymdhms(max_price_time),nanos_to_ymdhms(last_open_time));
                }
                
            }
            // M顶
            // if find_m_w_shape(bars, max_price_time, last_open_time, 1) {
            //     *direction = -2;
            //     *direction_time = last_open_time;
            //     //超过最高点2小时后，是否还执行这条规则？
            //     println!("-----MMM::max:starttime:{};endtime:{}",nanos_to_ymdhms(max_price_time),nanos_to_ymdhms(last_open_time));
            // }

            let (is_m,left_k_time,mid_k_time,right_k_time) = is_mw_model(bars, swings, last_open_time, 1);
            if is_m {
                *direction = -2;
                *direction_time = last_open_time;
                println!("-----MMMM::left:{},mid:{},right:{},newk:{}",nanos_to_ymdhms(left_k_time),nanos_to_ymdhms(mid_k_time),nanos_to_ymdhms(right_k_time),nanos_to_ymdhms(last_open_time));
            }
        }

        // 最近最低，连续5根阳线，立即做多
        if min_price_time > max_price_time && *direction != 2  {
            // 最低点之前必须有一波高潮，前2个小时运动超过600,否则不是最终的低点
            let (pre_max_price,_,pre_min_price,_) = find_max_price(bars, 2*12, if last_open_time <= min_price_time.add(2*5*60*1_000_000_000) {last_open_time}else{min_price_time.add(2*5*60*1_000_000_000)});
            if pre_max_price < pre_min_price.add(6000) {
                return;
            }
            
            let (continue_ups,continue_ups_time,continue_downs,continue_downs_time) = stats_continus_kline(bars, min_price_time, last_open_time - 5 * 60 * 1_000_000_000);
            // println!("2222,continue_ups:{},continue_downs:{},starttime:{};endtime:{}",continue_ups,continue_downs,nanos_to_ymdhms(min_price_time),nanos_to_ymdhms(last_open_time));
            if continue_ups >=5 {
                let (swing_high_time,swing_high) = find_max_swing_next(swings, continue_ups_time, 1);
                if continue_ups >=10 || (swing_high > 0 && newk.close_tick > swing_high) {
                    *direction = 2;
                    *direction_time = last_open_time;
                    println!("+++++222::min:starttime:{};endtime:{}",nanos_to_ymdhms(min_price_time),nanos_to_ymdhms(last_open_time));
                }
                
            }
            // W底
            // if find_m_w_shape(bars, min_price_time, last_open_time, -1) {
            //     *direction = 2;
            //     *direction_time = last_open_time;
            //     println!("+++++WWW::min:starttime:{};endtime:{}",nanos_to_ymdhms(min_price_time),nanos_to_ymdhms(last_open_time));
            // }
            let (is_w,left_k_time,mid_k_time,right_k_time) = is_mw_model(bars, swings, last_open_time, -1);
            if is_w {
                *direction = 2;
                *direction_time = last_open_time;
                println!("+++++WWWW::left:{},mid:{},right:{},newk:{}",nanos_to_ymdhms(left_k_time),nanos_to_ymdhms(mid_k_time),nanos_to_ymdhms(right_k_time),nanos_to_ymdhms(last_open_time));
            }
            
        }

        

        
    }
}

// 止损止盈:参考最近的低点或者高点
// 1. 动态止盈，及时运动到平衡点，止盈位置太远，会错过很多机会
// 2. 连续下跌，快速到了止盈位置，止盈后继续下跌。应该要分析下降k线，是否遇到供应堆积（继续）/需求堆积（止盈），或者当前是否高潮（继续到微单/大单为止）
// 3. 及时移动到平衡位，出现底部大单/微单/poc/需求堆积，立即止损。
// 4. 突破最近的一系列高/低点，继续运动，不要止盈；而反向突破，及时止损。
fn stop_price(swings: &Vec<(i64,i64)>, bars:&HashMap<i64, &KLine>, direction:i32, tick:i64, tick_time:i64, open_time:i64, open_tick:i64, position:f64, stop_loss:&mut i64,stop_profit:&mut i64){
    let last_swing_direction = swings[swings.len()-1].1 - swings[swings.len()-2].1;
    let last_swing_high_time = if last_swing_direction > 0 {swings[swings.len()-1].0}else{swings[swings.len()-2].0};
    let last_swing_low_time = if last_swing_direction > 0 {swings[swings.len()-2].0}else{swings[swings.len()-1].0};

    let lastk = bars.get(&(tick_time - 5 * 60 * 1_000_000_000)).unwrap();
    if position == 0.0 {
        return;
    }

    if position > 0.0{
        // if *stop_loss == 0 && direction == 1{
        //     *stop_loss = tick - 1000;
        //     *stop_profit = tick + 2000;
        //     if *stop_loss < last_swing_low - 100 {
        //         *stop_loss = last_swing_low - 100;
        //     }
        // }

        // // 如果前一根K线的低点站上盈亏平衡，则止损点设为盈亏平衡
        // if lastk.low_tick > open_tick.mul(10009).div(10000) + 500
        // && open_tick.mul(10008).div(10000) > *stop_loss 
        // && position > 0.0 
        // && lastk.open_time > open_time{ //不能用刚开仓时的前一根K线的数据来调整止损
        //     *stop_loss = open_tick.mul(10009).div(10000);
        // }
        
        // 如果新的低点高于止损，则把止损升高
        // if last_swing_low_time > open_time 
        // && *stop_loss < bars.get(&last_swing_low_time).unwrap().low_tick{
        //     *stop_loss = bars.get(&last_swing_low_time).unwrap().low_tick;
        //     // *stop_profit += 2000;
        // }

        // 如果当前价格接近止盈，则动态提升止盈止损
        if tick >= *stop_profit - 50 && position > 0.0 {
            // println!("2.stop_profit:{};stop_loss:{};tick:{}",stop_profit,stop_loss,tick);
            *stop_loss = tick - 2000;
            // *stop_profit += 500;            
        }
        
        // 止损移动到最近3根波段的最低点
        let mut lowest_time = swings[swings.len()-1].0;
        if swings[swings.len()-1].1 < swings[swings.len()-2].1 {
            lowest_time = if swings[swings.len()-3].1 < swings[swings.len()-1].1 { swings[swings.len()-3].0}else{lowest_time};
            lowest_time = if swings[swings.len()-5].1 < swings[swings.len()-3].1 { swings[swings.len()-5].0}else{lowest_time};
        }else{  
            lowest_time =  swings[swings.len()-2].0;       
            lowest_time = if swings[swings.len()-4].1 < swings[swings.len()-2].1 { swings[swings.len()-4].0}else{lowest_time};
            lowest_time = if swings[swings.len()-6].1 < swings[swings.len()-4].1 { swings[swings.len()-6].0}else{lowest_time};
        };
        
        // if *stop_loss < bars.get(&lowest_time).unwrap().low_tick
        // && lowest_time > open_time
        // && position > 0.0 {
        //     *stop_loss = bars.get(&lowest_time).unwrap().low_tick;
        // }


        // let (max_price,_,min_price,_) = find_max_price(bars, 12*12, tick_time - 5 * 60 * 1_000_000_000);
        // // 如果价格达到最近12小时的最高，则设置最新止损
        // if tick > max_price && position > 0.0 && *stop_loss < tick - 5000 {
        //     *stop_loss = tick - 2000;
        // }


    }else if position < 0.0{
        // if *stop_loss == 0 && direction == -1{
        //     *stop_loss = tick + 1000;
        //     *stop_profit = tick - 2000;
        //     if *stop_loss > last_swing_heigh + 100 {
        //         *stop_loss = last_swing_heigh + 100;
        //     }
        // }

        // // 如果前一根K线的高点低于盈亏平衡，则止损点设为盈亏平衡
        // if lastk.high_tick < open_tick.mul(9991).div(10000) - 500
        // && *stop_loss > open_tick.mul(9992).div(10000) 
        // && position < 0.0 
        // && lastk.open_time > open_time{ //不能用刚开仓时的前一根K线的数据来调整止损
        //     *stop_loss = open_tick.mul(9991).div(10000);
        // }

        // // 如果高点降低，则把止损下降，止盈相应下降
        // if last_swing_high_time > open_time 
        // && *stop_loss > bars.get(&last_swing_high_time).unwrap().high_tick{
        //     *stop_loss = bars.get(&last_swing_high_time).unwrap().high_tick;
        //     // *stop_profit += 2000;
        // }
        // if *stop_loss > last_swing_heigh + 100 && position < 0.0 && tick_time - open_time > 5 * 60 * 1_000_000_000 {
        //     *stop_loss = last_swing_heigh + 100;
        //     // *stop_profit -= 2000;
        // }

        // 当前价格接近止盈，则动态提升止盈止损
        if tick <= *stop_profit + 50 && position < 0.0 {
            *stop_loss = tick + 2000;
            // *stop_profit -=  500;
        }

        // 止损移动到最近3根波段的最高点
        let mut highest_time = swings[swings.len()-1].0;
        if swings[swings.len()-1].1 > swings[swings.len()-2].1 {
            highest_time = if swings[swings.len()-3].1 > swings[swings.len()-1].1 { swings[swings.len()-3].0}else{highest_time};
            highest_time = if swings[swings.len()-5].1 > swings[swings.len()-3].1 { swings[swings.len()-5].0}else{highest_time};
        }else{  
            highest_time =  swings[swings.len()-2].0;       
            highest_time = if swings[swings.len()-4].1 > swings[swings.len()-2].1 { swings[swings.len()-4].0}else{highest_time};
            highest_time = if swings[swings.len()-6].1 > swings[swings.len()-4].1 { swings[swings.len()-6].0}else{highest_time};
        };
        
        // if *stop_loss > bars.get(&highest_time).unwrap().high_tick
        // && highest_time > open_time
        // && position > 0.0 {
        //     *stop_loss = bars.get(&highest_time).unwrap().high_tick;
        // }
        
        let (_,_,min_price,_) = find_max_price(bars, 12*12, tick_time - 5 * 60 * 1_000_000_000);
        // 如果价格达到最近6小时的最低，则设置最新止损
        // if tick < min_price && position < 0.0 && *stop_loss > tick + 5000 {
        //     *stop_loss = tick + 2000;
        // }

    }
}

fn find_max_price(bars:&HashMap<i64, &KLine>, nums:usize, last_time:i64)->(i64,i64,i64,i64){
    let mut max_price = 0;
    let mut max_price_time = 0;
    let mut min_price = i64::MAX;
    let mut min_price_time = 0;
    for i in 0..nums{
        let k = bars.get(&(last_time - i as i64 * 5 * 60 * 1_000_000_000));
        match k {
            Some(k) => {
                if k.high_tick > max_price {
                    max_price = k.high_tick;
                    max_price_time = k.open_time;
                }
                if k.low_tick < min_price {
                    min_price = k.low_tick;
                    min_price_time = k.open_time;
                }
            },
            None => {
                return (max_price,max_price_time,min_price,min_price_time);
            }            
        }
    }
    (max_price,max_price_time,min_price,min_price_time)
}

fn stats_continus_kline(bars:&HashMap<i64, &KLine>, start_time:i64, end_time:i64) -> (i8,i64,i8,i64){
    let mut up = 0;
    let mut up_time = 0i64;
    let mut down = 0;
    let mut down_time = 0i64;
    let mut latest_up = 0;
    let mut latest_down = 0;
    let mut last_high = i64::MAX;
    let mut last_low = 0;
    let mut test_time = start_time;
    while  test_time < end_time {
        let k = bars.get(&test_time);
        match k {
            Some(k) => {
                if k.close_tick > k.open_tick || (k.low_tick > last_low && latest_up > 0){//阳线，或者最小阴线
                    latest_up += 1;
                    latest_down = 0;
                    up = if latest_up > up {latest_up}else{up};
                    if up >= 5 && up == latest_up{
                        up_time = k.open_time;
                    }
                    // up = if up >= 5 && latest_up == 4 { up + latest_up}else{up};
                }else if k.close_tick < k.open_tick || (k.high_tick < last_high && latest_down > 0){//阴线，或者最小阳线
                    latest_down += 1;
                    latest_up = 0;
                    down = if latest_down > down {latest_down}else{down};
                    if down >= 5 && down == latest_down {
                        down_time = k.open_time;
                    }
                    // down = if down >= 5 && latest_down == 4 { down + latest_down}else{down};
                }
                last_high = k.high_tick;
                last_low = k.low_tick;
            },
            None => {
                return (up,up_time,down,down_time);
            }            
        }
        test_time += 5 * 60 * 1_000_000_000;
    }
    (up,up_time,down,down_time)

}

// 查找指定时间之后的第一个波段点
// h_or_l: 1:高点，-1:低点
fn find_max_swing_next(swings: &Vec<(i64,i64)>,start_time:i64, h_or_l:i8)->(i64,i64){
    let mut max = 0;
    let mut max_time = 0;
    let slen = swings.len();
    let last_high = swings[slen-1].1 > swings[slen-2].1;
    for i in 1..=slen{
        if swings[slen-i].0 < start_time {
            break;
        }
        if h_or_l == 1 && (last_high && i%2 == 1 || !last_high && i%2 == 0){
            max = swings[slen-i].1;
            max_time = swings[slen-i].0;
        }else if h_or_l == -1 && (last_high && i%2 == 0 || !last_high && i%2 == 1){
            max = swings[slen-i].1;
            max_time = swings[slen-i].0;
        }
    }
    (max_time,max)
}

// 寻找指定时间之前的第一个高/低点
fn find_max_swing_prev(swings: &Vec<(i64,i64)>,start_time:i64, h_or_l:i8)->(i64,i64){
    let mut max = 0;
    let mut max_time = 0;
    let slen = swings.len();
    let last_high = swings[slen-1].1 > swings[slen-2].1;
    for i in 1..=slen{
        if h_or_l == 1 && (last_high && i%2 == 1 || !last_high && i%2 == 0){
            max = swings[slen-i].1;
            max_time = swings[slen-i].0;
        }else if h_or_l == -1 && (last_high && i%2 == 0 || !last_high && i%2 == 1){
            max = swings[slen-i].1;
            max_time = swings[slen-i].0;
        }
        if swings[slen-i].0 < start_time {
            break;
        }
    }
    (max_time,max)
}

fn find_m_w_shape(bars:&HashMap<i64, &KLine>, start_time:i64, end_time:i64, up_or_down:i8)->bool{
    let mut is_mw = false;
    if bars.len() <= 3 || bars.get(&start_time).is_none() || end_time.sub(start_time).div(5*60*1_000_000_000) < 10 {
        return is_mw;
    }
    // let max_bar = bars.iter().max_by(|a,b| a.1.high_tick.cmp(&b.1.high_tick)).unwrap();
    let max_bar = bars.get(&start_time).unwrap();
    let newk = bars.get(&end_time).unwrap();

    let (_,max_k_time,_,min_k_time) = find_max_price(bars, end_time.sub(start_time).div(5*60*1_000_000_000) as usize, end_time);
    if max_k_time == 0 || min_k_time == 0 {
        return is_mw;
    }

    let max_k = bars.get(&max_k_time).unwrap();
    let min_k = bars.get(&min_k_time).unwrap();
    // println!("find_m_w_shape-----11111-->max:{},min:{},diff:{}",nanos_to_ymdhms(max_k_time),nanos_to_ymdhms(min_k_time),max_k.high_tick.sub(max_bar.high_tick).abs());

    if up_or_down == 1 
    && start_time < min_k_time.sub(4*5*60*1_000_000_000) // M低点在最开始的2根K线之后
    && min_k_time < max_k_time.sub(4*5*60*1_000_000_000) // M点高点在低点2根K线之后
    && max_k.high_tick.sub(max_bar.high_tick).abs() < 2000 // 两个高点差距在200之内
    && newk.close_tick < min_k.low_tick 
    { // 超过M点颈线，M型成立
        println!("MMMMMMMMMMMMMMMMMMM");
        is_mw = true;
    }else if up_or_down == -1 
    && start_time < max_k_time.sub(4*5*60*1_000_000_000) // W高点在最开始的2根K线之后
    && max_k_time < min_k_time.sub(4*5*60*1_000_000_000) // W低点在高点2根K线之后
    && min_k.low_tick.sub(max_bar.low_tick).abs() < 2000 // 两个低点差距在200之内
    && newk.high_tick > max_k.high_tick
    { // 右线超越高点
        println!("WWWWWWWWWWWWWWWWWWWW");
        is_mw = true;
    }
    is_mw
}

// m_or_w: 1:M型，-1:W型
fn is_mw_model(bars:&HashMap<i64, &KLine>, swings:&Vec<(i64,i64)>, last_time:i64, m_or_w:i8)->(bool,i64,i64,i64){
    let mut is_mw = false;
    if bars.len() <= 6*12 {
        return (is_mw,0,0,0);
    }
    // let max_bar = bars.iter().max_by(|a,b| a.1.high_tick.cmp(&b.1.high_tick)).unwrap();

    let last_swing_h_or_l = if swings[swings.len()-1].1 > swings[swings.len()-2].1 {1}else{-1};
    let newk = bars.get(&last_time).unwrap();
    let last_close_k = bars.get(&(last_time - 5 * 60 * 1_000_000_000)).unwrap();

    let (_,max_k_time,_,min_k_time) = find_max_price(bars, 36, last_time);//最近3小时最低最高价
    if max_k_time == 0 || min_k_time == 0 {
        return (is_mw,0,0,0);
    }
    let right_k = if m_or_w>0 { bars.get(&max_k_time).unwrap() } else { bars.get(&min_k_time).unwrap() };
    
    // 高点或低点的左边必须有12根低于高点，或高于低点，表明该高点或低点是一个确定的高/低点，不会在短暂向前就有更高/更低点
    if right_k.open_time < last_time.sub(24 * 5*60*1_000_000_000) {
        return (is_mw,0,0,0);
    }
    let mut left_k_time = 0;
    for i in 1..swings.len(){
        if swings[swings.len()-i].0 >= right_k.open_time {
            continue;
        }        
        if swings[swings.len()-i].0 <= right_k.open_time.sub(48 * 12 * 5 * 60 * 1_000_000_000) {//超过48小时停止
            break;
        }
        // 如果是M型，寻找高点。波段最后是高点，则奇数次是高点保留，偶数次是低点跳过。波段最后是低点，则偶数次是高点保留，奇数次是低点跳过
        if m_or_w > 0 && (last_swing_h_or_l > 0 && i%2 == 0 || last_swing_h_or_l < 0 && i%2 == 1){ //
            continue;
        }
        // 如果是W型，寻找低点。波段最后是高点，则奇数次是高点跳过，偶数次是低点保留。波段最后是低点，则偶数次是高点跳过，奇数次是低点保留
        if m_or_w < 0 && (last_swing_h_or_l > 0 && i%2 == 1 || last_swing_h_or_l < 0 && i%2 == 0){ //
            continue;
        }
        
        // M，比较左右shoulder高点
        if m_or_w > 0 && swings[swings.len()-i].1.sub(right_k.high_tick).abs() < 2000 {//如果右边高点和左边高点差距在200之内
            left_k_time = swings[swings.len()-i].0;
            break;
        }
        // W，比较左右低点
        if m_or_w < 0 && swings[swings.len()-i].1.sub(right_k.low_tick).abs() < 2000 {//如果右边低点和左边低点差距在200之内
            left_k_time = swings[swings.len()-i].0;
            break;
        }
    }
    if left_k_time == 0 {
        return (is_mw,0,0,0);
    }

    // 查找从左肩往前3个小时的最高/低点(包含左肩），确保左肩是在之前3个小时之内的最高点或最低点
    let (_,max_leftk_time,_,min_leftk_time) = find_max_price(bars, 36, left_k_time.add(2*5*60*1_000_000_000));
    // println!("####is_mw::left:{},max:{},min:{},right:{}",nanos_to_ymdhms(left_k_time),nanos_to_ymdhms(max_leftk_time),nanos_to_ymdhms(min_leftk_time),nanos_to_ymdhms(right_k.open_time));
    left_k_time = if m_or_w > 0 {max_leftk_time}else{min_leftk_time};

    // 如果左肩之前3小时的高/低点不是之前获得的第一个左肩，那么左肩更新为当前新的，并与右肩比较，不能相差超过2000
    if left_k_time == 0 || bars.get(&left_k_time).is_none() 
    || (m_or_w > 0 && bars.get(&left_k_time).unwrap().high_tick.sub(right_k.high_tick).abs() > 2000) // 左肩高点在右肩高点之上
    || (m_or_w < 0 && bars.get(&left_k_time).unwrap().low_tick.sub(right_k.low_tick).abs() > 2000) // 左肩低点在右肩低点之下
    {
        return (is_mw,0,0,0);
    }
    // 寻找中间的高低点
    let (_,max_midk_time,_,min_midk_time) = find_max_price(bars, right_k.open_time.sub(left_k_time).div(5*60*1_000_000_000).sub(1) as usize, right_k.open_time.sub(5*60*1_000_000_000));
    if max_midk_time == 0 || min_midk_time == 0 {
        return (is_mw,0,0,0);
    }
    let mid_k = if m_or_w > 0 {bars.get(&min_midk_time).unwrap()}else{bars.get(&max_midk_time).unwrap()};

    if m_or_w > 0 && last_close_k.close_tick < mid_k.low_tick && newk.close_tick < mid_k.low_tick {
        is_mw = true;
    }else if m_or_w < 0 && last_close_k.close_tick > mid_k.high_tick && newk.close_tick < mid_k.high_tick {
        is_mw = true;
    }
    // println!("is_mw_model::{}:left:{},mid:{},right:{},newk:{}",m_or_w,nanos_to_ymdhms(left_k_time),nanos_to_ymdhms(mid_k.open_time),nanos_to_ymdhms(right_k.open_time),nanos_to_ymdhms(newk.open_time));
    (is_mw,left_k_time,mid_k.open_time,right_k.open_time)

}
