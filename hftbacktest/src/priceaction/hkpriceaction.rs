use std::{
    any::Any, collections::HashMap, fmt::{Debug, Formatter}, hash::Hash, ops::Div, time::{Duration, SystemTime, UNIX_EPOCH}
};

use crate::types::Side;

use super::{nanos_to_ymdhms, KLine, PriceAction, Swings,Imbalance};


#[derive(Debug, Clone)]
pub struct TickFlows {
    pub buy_tick_qtys: HashMap<i64, f64>,
    pub sell_tick_qtys: HashMap<i64, f64>,
    // pub buy_imbalance: f64,
    // pub sell_imbalance: f64,
}

impl Default for TickFlows {
    fn default() -> Self {
        Self {
            buy_tick_qtys: HashMap::new(),
            sell_tick_qtys: HashMap::new(),
        }
    }   
}

impl TickFlows {
    pub fn new(tick:i64, qtys: f64, side:Side) -> Self {
        let mut buy_tick_qtys: HashMap<i64, f64> = HashMap::new();
        let mut sell_tick_qtys: HashMap<i64, f64> = HashMap::new();
        if side == Side::Buy{
            buy_tick_qtys.insert(tick, qtys);
            sell_tick_qtys.insert(tick, 0.0);
        }else{
            sell_tick_qtys.insert(tick, qtys);
            buy_tick_qtys.insert(tick, 0.0);
        }
        Self {
            buy_tick_qtys,
            sell_tick_qtys,
        }
    }

    pub fn trade(&mut self, tick:i64, qty:f64, side:Side){
        if side == Side::Buy{
            let buy_tick_qty = self.buy_tick_qtys.entry(tick).or_insert(0.0);
            *buy_tick_qty += qty;
            self.sell_tick_qtys.entry(tick).or_insert(0.0);
        }else{
            let sell_tick_qty = self.sell_tick_qtys.entry(tick).or_insert(0.0);
            *sell_tick_qty += qty;
            self.buy_tick_qtys.entry(tick).or_insert(0.0);
        }
    }

    pub fn cal_poc_sellrate_buyrate(&self) -> (i64,f64,f64,f64) {
        let mut prices:Vec<i64> = self.buy_tick_qtys.keys().cloned().collect();
        prices.sort();
        let veclen = prices.len();
        if veclen < 2 {
            return (0,0.0,0.0,0.0);
        }

        let mut sellrate = self.sell_tick_qtys.get(&prices[1]).unwrap().max(0.01);
        sellrate = sellrate.div(self.sell_tick_qtys.get(&prices[0]).unwrap());

        let mut buyrate = self.buy_tick_qtys.get(&prices[veclen-2]).unwrap().max(0.01);
        buyrate = buyrate.div(self.buy_tick_qtys.get(&prices[veclen-1]).unwrap());

        let mut poc_tick = 0i64;
        let mut poc_qty = 0.0;
        let mut poc_hashmap:HashMap<i64, f64> = HashMap::new();
        for tick in prices.iter(){
            let value = poc_hashmap.entry(*tick / 10).or_insert(0.0);
            *value += self.buy_tick_qtys.get(tick).unwrap() + self.sell_tick_qtys.get(tick).unwrap();
            if *value > poc_qty {
                poc_qty = *value;
                poc_tick = *tick / 10;
            }
        }
        return (poc_tick,poc_qty,sellrate,buyrate);

    }
}


#[derive(Debug, Clone)]
pub struct HkPriceAction {
    // pub klines: HashMap<i64, Vec<KLine>>,
    pub kmaps: HashMap<i64, HashMap<i64, KLine>>,
    pub last_open_time: HashMap<i64,i64>,
    pub tick_flows: HashMap<i64, HashMap<i64,TickFlows>>,
    pub ema_periods: Vec<i64>,
    // pub emas: HashMap<i64, HashMap<i64, FixedSizeEma<f64>>>,
    pub intervals: Vec<i64>,
    pub price_tick_qtys: HashMap<i64, (f64,u128)>,
    pub swings: Swings,
    pub last_tick: i64,
    pub last_tick_qty: f64,
    pub last_side: Side,
    pub last_tick_time: i64,
    pub imbalance: Imbalance,
}

impl HkPriceAction{
    pub fn new(intervals:Vec<i64>, ema_periods: Vec<i64>) -> Self {
        // let mut klines: HashMap<i64, Vec<KLine>> = HashMap::new();
        let mut kmaps: HashMap<i64, HashMap<i64, KLine>> = HashMap::new();
        let mut tick_flows: HashMap<i64, HashMap<i64,TickFlows>> = HashMap::new();
        // let mut emas: HashMap<i64, HashMap<i64, FixedSizeEma<f64>>> = HashMap::new();
        let mut last_open_time: HashMap<i64,i64> = HashMap::new();
        // let ema_operators: HashMap<i64, HashMap<i64, ExponentialMovingAverage>> = HashMap::new();
        for interval in intervals.iter(){
            // klines.entry(*interval).or_insert(Vec::new());
            kmaps.entry(*interval).or_insert(HashMap::new());
            tick_flows.entry(*interval).or_insert(HashMap::new());
            // emas.entry(*interval).or_insert(HashMap::new());
            // for ema_period in ema_periods.iter(){
            //     emas.get_mut(interval).unwrap().entry(*ema_period).or_insert(FixedSizeEma::new(256 as usize));
            // }

            last_open_time.insert(*interval, 0);
        }

        Self{
            // klines,
            kmaps,
            last_open_time,
            tick_flows,
            ema_periods,
            // emas,
            intervals,
            price_tick_qtys: HashMap::new(),
            swings: Swings::default(),
            last_tick: 0,
            last_tick_qty: 0.0,
            last_side: Side::None,
            last_tick_time: 0,
            imbalance: Imbalance::new(30),
        }
    }

    pub fn update_kline(&mut self, interval:i64, tick:i64, tick_qty:f64, tick_time:i64, side:Side){
        // let klines = self.klines.get_mut(&interval).unwrap();
        let kmaps = self.kmaps.get_mut(&interval).unwrap();
        let open_time = self.last_open_time.get(&interval).unwrap();
        let tick_flow = self.tick_flows.get_mut(&interval).unwrap();
        if *open_time == 0 {
            let new_kline = KLine::new(tick, tick_qty, tick_time, interval, side, self.ema_periods.len());
            // *open_time = new_kline.open_time;
            self.last_open_time.entry(interval).and_modify(|e| *e = new_kline.open_time);
            let new_open_time = new_kline.open_time;
            kmaps.insert(new_open_time, new_kline);

            tick_flow.insert(new_open_time, TickFlows::new(tick as i64, tick_qty, side));

            self.update_emas(interval, tick, new_open_time,false);
            return;
        }
        // let last_kline = klines.last_mut().unwrap();
        let last_kline = kmaps.get_mut(&open_time).unwrap();
        if last_kline.close_time < tick_time{
            let last_open_time = last_kline.open_time;
            let (poc_tick,poc_qty,sellrate,buyrate) = tick_flow.get(&last_open_time).unwrap().cal_poc_sellrate_buyrate();
            last_kline.poc_price = poc_tick;
            last_kline.poc_qty = poc_qty;
            last_kline.top_buy_rate = buyrate;
            last_kline.top_sell_rate = sellrate;
            
            println!("kline::{}, interval: {}, o: {}, h: {}, c: {}, l: {}", nanos_to_ymdhms(last_kline.open_time), interval,  last_kline.open_tick, last_kline.high_tick, last_kline.close_tick, last_kline.low_tick);
            let new_kline = KLine::new(tick, tick_qty, tick_time, interval, side, self.ema_periods.len());

            let new_open_time = new_kline.open_time;
            self.last_open_time.entry(interval).and_modify(|e| *e = new_open_time);
            
            kmaps.insert(new_open_time, new_kline);
            // klines.push(new_kline);

            tick_flow.insert(new_open_time, TickFlows::new(tick as i64, tick_qty, side));
            

            self.update_emas(interval, tick, new_open_time, true);
            self.remove_old_trades(tick_time as u128,24*60*60*1000_000_000);
            // let topprices = self.top_trades(5);
            // println!("top price 1: {} : {}", topprices[0].0,topprices[0].1);
            // println!("top price 2: {} : {}", topprices[1].0,topprices[1].1);
            // println!("top price 3: {} : {}", topprices[2].0,topprices[2].1);
            // println!("top price 4: {} : {}", topprices[3].0,topprices[3].1);
            // println!("top price 5: {} : {}", topprices[4].0,topprices[4].1);
            // let pre_tick_flow = self.tick_flows.get(&interval).unwrap().get(&last_open_time as &i64).unwrap();
            // for tick in pre_tick_flow.buy_tick_qtys.keys() {
                // println!("{:.3}   {}   {:.3}",pre_tick_flow.sell_tick_qtys.get(tick).unwrap(),tick,pre_tick_flow.buy_tick_qtys.get(tick).unwrap());
            // }
            // panic!("test:{}",last_open_time);
            if interval == self.intervals[0] {                
                self.swings(interval);
            }

        }else{
            last_kline.update(tick, tick_qty, tick_time, side);
            let open_time = last_kline.open_time;
            tick_flow.get_mut(&open_time).unwrap().trade(tick as i64, tick_qty, side);
            self.update_emas(interval, tick, open_time, false);
        }
    }

    pub fn update_emas(&mut self, interval:i64, tick:i64, open_time:i64, closed:bool){
        let klen = self.kmaps.get(&interval).unwrap().len();
        
        for (_i,ema_period) in self.ema_periods.iter().enumerate(){
            if klen < 2 {//第一个k线
                let ema = self.kmaps.get_mut(&interval).unwrap().get_mut(&open_time).unwrap().emas.get_mut(_i).unwrap(); 
                // let ema = kline.emas.get_mut(_i).unwrap();
                *ema = tick;
            }else{
                let last_ema = *self.kmaps.get(&interval).unwrap().get(&(open_time - interval)).unwrap().emas.get(_i).unwrap();
                let ema = self.kmaps.get_mut(&interval).unwrap().get_mut(&open_time).unwrap().emas.get_mut(_i).unwrap();
                let k = 2.0 / (*ema_period as f64 * 1.0 + 1.0);
                *ema = ((tick - last_ema) as f64 * k + last_ema as f64).round() as i64;
                // *ema = tick * k + last_ema * (1.0 - k); 
                // if closed {                           
                //     // let last_ema = *ema.last().unwrap();
                //     // // println!("update_emas:closed:interval: {}, ema_period: {},k: {}, ema: {}", interval, ema_period, k, last_ema);
                //     // let ema_value = (tick * k) + (last_ema * (1.0 - k));
                //     // ema.push(ema_value);
                // }else{
                //     let mut closed_ema = 0.0;
                    
                //     if emalen > 1 {
                //         closed_ema = *ema.get_second_last_value().unwrap();
                //     }
                //     let last_ema = ema.last_mut().unwrap();
                //     if emalen == 1 {                    
                //         *last_ema = tick;
                //     }else{
                //         *last_ema = (tick * k) + (closed_ema * (1.0 - k));
                //         // println!("222update_emas:open:interval: {}, ema_period: {},k: {}, closed_ema: {}, tick: {}, last_ema: {}", interval, ema_period, k, closed_ema,tick, *last_ema);
                //         // panic!("222update");
                //     }
                // }
            }
            // let ema = self.emas.get_mut(&interval).unwrap().get_mut(&ema_period).unwrap();
            // let emalen = ema.len();
            // if emalen == 0 {
            //     ema.push(tick);
            //     return;
            // }
            
        }
    }

    pub fn record_trade(&mut self, price: f64, quantity: f64, timestamp: u128) {
        let floor_price = price.floor() as i64;
        self.price_tick_qtys.entry(floor_price).and_modify(|e| {
            e.0 += quantity;
            e.1 = timestamp;
        }).or_insert((quantity, timestamp));
        // println!("record_trade:price: {}, quantity: {}, len: {}", price, quantity, self.price_tick_qtys.len());
    }

    fn remove_old_trades(&mut self, now:u128, retention_time:u128) {//nanos
        // let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        self.price_tick_qtys.retain(|_, &mut (_, timestamp)| now - timestamp <= retention_time);
    }

    fn top_trades(&self, first:usize) -> Vec<(i64, f64)> {
        let mut trades: Vec<_> = self.price_tick_qtys.iter().collect();
        trades.sort_by(|a, b| {
            b.1.0.partial_cmp(&a.1.0).unwrap()// First by quantity (large first)
                .then_with(|| b.1.1.cmp(&a.1.1)) // Then by timestamp first (recent first)
        });
        trades.into_iter().take(first).map(|(&price, &(quantity, _))| (price, quantity)).collect()
    }

    fn swings(&mut self, interval:i64){
        if interval != self.intervals[0] {
            return;
        }

        let kmaps = self.kmaps.get(&interval).unwrap();
        if kmaps.len() < 3 {
            return;
        }
        let open_time = self.last_open_time.get(&interval).unwrap();
        let k: &KLine = kmaps.get(&(open_time - interval)).unwrap();
        

        if self.swings.high_or_low == 0 {

            let emas = &k.emas;
            println!("emas:{} {} {},tick {} {}", emas[0], emas[1], emas[2],k.close_tick, k.open_tick );
            if k.close_tick > k.open_tick && emas[0] > emas[1] && emas[1] > emas[2] {
                self.swings.high_or_low = 1;
                self.swings.ext_tick = k.high_tick;
                self.swings.last_high_low = k.low_tick;
                self.swings.last_low_high = k.low_tick - 600; //保证第一个高点能够通过
            } else if k.close_tick < k.open_tick && emas[0] < emas[1] && emas[1] < emas[2] {
                self.swings.high_or_low = -1;
                self.swings.ext_tick = k.low_tick;
                self.swings.last_low_high = k.high_tick;
                self.swings.last_high_low = k.low_tick + 600;
            }
            self.swings.ext_opentime = k.open_time;
        } else {
            self.swings.wait_kines.push((k.open_time, k.high_tick, k.low_tick));
            if self.swings.high_or_low == 1 {
                if self.swings.swing_lows.len()>0 && k.low_tick < self.swings.swing_lows.last().unwrap_or(&(0i64,0i64)).1 {
                    self.swings.high_or_low = -1;
                    self.swings.ext_tick = k.low_tick;
                    self.swings.ext_opentime = k.open_time;
                    self.swings.last_low_high = k.high_tick;
                    self.swings.judge_time = 0;
                    self.swings.wait_kines.clear();
                    self.swings.swing_lows.last_mut().unwrap().1 = 0;
                    return;
                }else if k.high_tick > self.swings.ext_tick {
                    self.swings.ext_tick = k.high_tick;
                    self.swings.ext_opentime = k.open_time;
                    self.swings.last_high_low = k.low_tick;
                    self.swings.judge_time = 0;
                    self.swings.wait_kines.clear();
                }else{
                    self.swings.judge_time += 1;
                }

                if self.swings.ext_opentime - self.swings.swing_lows.last().unwrap_or(&(0i64,0i64)).0 > 3 * 5 * 60 * 1000_000_000 
                && (match self.swings.swing_lows.last() {
                    Some(h) => k.low_tick <= h.1,
                    None => false
                }
                || k.high_tick < self.swings.last_high_low
                || (self.swings.ext_tick - k.low_tick > 2000 && k.open_time != self.swings.ext_opentime && k.low_tick < self.swings.last_high_low) ){
                    if self.swings.swing_hights.len() > 0 && self.swings.swing_hights.last().unwrap().1 == 0 {
                        self.swings.swing_hights.last_mut().unwrap().0 = self.swings.ext_opentime;
                        self.swings.swing_hights.last_mut().unwrap().1 = self.swings.ext_tick;
                    }else{
                        self.swings.swing_hights.push((self.swings.ext_opentime, self.swings.ext_tick));
                    }                    

                    self.swings.high_or_low = -1;
                    println!("{} {} Hi", nanos_to_ymdhms(self.swings.swing_hights.last().unwrap().0),self.swings.swing_hights.last().unwrap().1);
                    println!("emas:{} {} {} opentime:{}", k.emas[0], k.emas[1], k.emas[2],nanos_to_ymdhms(k.open_time));
                    self.swings.ext_tick = k.low_tick;
                    self.swings.ext_opentime = k.open_time;
                    self.swings.last_low_high = k.high_tick;
                    self.swings.judge_time = 0;
                    
                    self.swings.wait_kines.clear();
                }

            }else{
                if self.swings.swing_hights.len()>0 && k.high_tick > self.swings.swing_hights.last().unwrap_or(&(0i64,0i64)).1 {
                    self.swings.high_or_low = 1;
                    self.swings.ext_tick = k.high_tick;
                    self.swings.ext_opentime = k.open_time;
                    self.swings.last_high_low = k.low_tick;
                    self.swings.judge_time = 0;
                    self.swings.wait_kines.clear();
                    self.swings.swing_hights.last_mut().unwrap().1 = 0;
                    return;
                }else if k.low_tick < self.swings.ext_tick {
                    self.swings.ext_tick = k.low_tick;
                    self.swings.ext_opentime = k.open_time;
                    self.swings.last_low_high = k.high_tick;
                    self.swings.judge_time = 0;
                    self.swings.wait_kines.clear();
                }else{
                    self.swings.judge_time += 1; 
                }


                if self.swings.ext_opentime - self.swings.swing_hights.last().unwrap_or(&(0i64,0i64)).0 > 3 * 5 * 60 * 1000_000_000  
                && (match self.swings.swing_hights.last() {
                    Some(h) => k.high_tick >= h.1,
                    None => false
                }
                || k.low_tick > self.swings.last_low_high
                || (k.high_tick - self.swings.ext_tick > 2000 && k.open_time != self.swings.ext_opentime && k.high_tick > self.swings.last_low_high)) {
                    if self.swings.swing_lows.len() > 0 && self.swings.swing_lows.last().unwrap().1 == 0 {
                        self.swings.swing_lows.last_mut().unwrap().0 = self.swings.ext_opentime;
                        self.swings.swing_lows.last_mut().unwrap().1 = self.swings.ext_tick;
                    }else{
                        self.swings.swing_lows.push((self.swings.ext_opentime, self.swings.ext_tick));
                    }                    

                    self.swings.high_or_low = 1;
                    println!("{} {} Lo", nanos_to_ymdhms(self.swings.swing_lows.last().unwrap().0),self.swings.swing_lows.last().unwrap().1);
                    println!("emas:{} {} {}", k.emas[0], k.emas[1], k.emas[2]);
                    self.swings.ext_tick = k.high_tick;
                    self.swings.ext_opentime = k.open_time;
                    self.swings.last_high_low = k.low_tick;

                    self.swings.judge_time = 0;
                    
                    self.swings.wait_kines.clear();
                }
            }
        }
    }


}

impl PriceAction for HkPriceAction{
    fn order_flow(&mut self, px:f64, tick_size:f64, qty:f64, timestamp:i64, side:Side){
        let tick = (px / tick_size).round() as i64;
        //9.8.17.10.00:1725786600000000000
        //9.8.17.10.05:1725786900000000000
        //9.8 8:10:1725754200000000000
        //9.8 8:15:1725754500000000000
        if self.last_tick == tick && self.last_side == side {
            self.last_tick_qty += qty;
        }else {
            // if timestamp >= 1725754200000000000 && timestamp < 1725754500000000000 {
            //     // println!("{:.1}, {:.3}, {}, {:?}", self.last_tick, self.last_tick_qty, nanos_to_ymdhms(self.last_tick_time), self.last_side);
            //     let (buy_imanlance, sell_imbalance) = self.imbalance.cal_imbalance();
            //     if buy_imanlance.len() > 2 {
            //         println!("buy_imanlance:{:?}", buy_imanlance);
            //     }
            //     if sell_imbalance.len() > 2 {
            //         println!("sell_imbalance:{:?}", sell_imbalance);
            //     }
            // }


            self.imbalance.push(self.last_tick, self.last_tick_qty, self.last_side);
            
            self.last_tick = tick;
            self.last_side = side;
            self.last_tick_qty = qty;
            self.last_tick_time = timestamp;            
        }
        
        // if timestamp >= 1725754500000000000 { panic!("return...");}
        
        let tick_qty = qty;
        let tick_time = timestamp;
        let intervals: Vec<i64> = self.intervals.clone();
        for interval in intervals.iter(){
            self.update_kline(*interval, tick, tick_qty, tick_time, side);
            
        }
        

        self.record_trade(px, qty, timestamp as u128);

        // let price_tick_qty = self.price_tick_qtys.entry(tick as i64).or_insert(0.0);
        // *price_tick_qty += tick_qty;
        
    }

    // fn kines(&self, interval:i64, nums:i32) -> (&[KLine]{
    //     let klines = self.klines.get(&interval).unwrap();
    //     let len = klines.len();
    //     if len <= nums as usize {
    //         &klines[..]
    //     }else {
    //         &klines[len - nums as usize..]
    //     }
    // }

    fn kmaps(&self, interval:i64, nums:usize) -> (HashMap<i64, &KLine>, i64){
        let kmaps = self.kmaps.get(&interval).unwrap();
        let mut return_maps: HashMap<i64, &KLine> = HashMap::new();

        let len = if nums < kmaps.len() {nums} else {kmaps.len()};
        if len == 0 {
            return (return_maps, 0);
        }
        let open_time = self.last_open_time.get(&interval).unwrap();
        for i in 0..len{
            let kline = kmaps.get(&(open_time - interval * i as i64)).unwrap();
            return_maps.insert(open_time - interval * i as i64, kline);
        }

        return (return_maps, *open_time);
    }

    // //pub emas: HashMap<i64, HashMap<i64, FixedSizeEma<f64>>>,
    // fn emas(&self, interval:i64, exporid:i64, nums:i32) -> &[f64]{
    //     let emas = self.emas.get(&interval).unwrap().get(&exporid).unwrap().get();
    //     let len = emas.len();
    //     if len <= nums as usize {
    //         &emas[..]
    //     }else {
    //         &emas[len - nums as usize..]
    //     }
    // }

    fn swings(&self, nums:usize) -> Vec<(i64, i64)>{ 
        let mut swings: Vec<(i64, i64)> = Vec::new();       
        let swing_hights = &self.swings.swing_hights;
        let swing_lows = &self.swings.swing_lows;
        if swing_hights.len() == 0 || swing_lows.len() == 0 {
            return swings;
        }
        let hights_last_opentime = swing_hights.last().unwrap().0;
        let lows_last_opentime = swing_lows.last().unwrap().0;
        let hights_len = swing_hights.len();
        let lows_len = swing_lows.len();
        let mut lens = if hights_len < lows_len {hights_len}else{lows_len};
        lens = if lens > nums {nums}else{lens};
        for i in (1..=lens).rev() {
            if hights_last_opentime > lows_last_opentime {
                swings.push(swing_lows[lows_len - i]);
                swings.push(swing_hights[hights_len - i]);                
            }else{
                swings.push(swing_hights[hights_len - i]);
                swings.push(swing_lows[lows_len - i]);                
            }
        }
        swings
    }

    fn last_acc_trades(&self) -> (i64, f64, i64, Side){
        (self.last_tick, self.last_tick_qty, self.last_tick_time, self.last_side)
    }
}