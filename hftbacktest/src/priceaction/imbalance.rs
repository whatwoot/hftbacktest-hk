use std::{collections::HashMap, ops::{Add, Sub}};

use crate::types::Side;

#[derive(Debug, Clone, Default)]
pub struct Imbalance {
    pub ticks: Vec<i64>,
    pub buy_qtys: HashMap<i64, f64>,
    pub sell_qtys: HashMap<i64, f64>,
    pub ticks_max_len: usize,
}

impl Imbalance{
    pub fn new(ticks_max_len: usize) -> Self {
        Self {
            ticks: Vec::new(),
            buy_qtys: HashMap::new(),
            sell_qtys: HashMap::new(),
            ticks_max_len,
        }
    }

    pub fn cal_imbalance(&self) -> (Vec<i64>, Vec<i64>) {
        let mut buy_imanlance:Vec<i64> = Vec::new();
        let mut sell_imbalance:Vec<i64> = Vec::new();
        if self.ticks.len() < 2 || self.buy_qtys.len() < 1 || self.sell_qtys.len() < 1 {
            return (buy_imanlance, sell_imbalance);
        }
        for tick in &self.ticks {
            let buy_qty = self.buy_qtys.get(tick).unwrap_or(&0.0);
            let sell_qty = self.sell_qtys.get(&tick.sub(1)).unwrap_or(&0.0);
            if *buy_qty >= 0.05 && *sell_qty >= 0.02 {
                if buy_qty / sell_qty > 3.0 {
                    buy_imanlance.push(*tick);
                }
                if sell_qty / buy_qty > 3.0 {
                    sell_imbalance.push(*tick);
                }
            }  
        }
        (buy_imanlance, sell_imbalance)
    }

    pub fn push(&mut self, tick: i64, qty: f64, side: Side) {
        match side {
            Side::Buy => {
                // 使用二分查找维护 ticks 的有序性插入 tick，同时更新 qtys。
                // 如果 tick 已存在，则更新对应的 qty。如果达到最大容量，则删除最小的 tick。
                match self.ticks.binary_search(&tick) {
                    // 如果存在，则只更新 qtys
                    Ok(_pos) => {
                        self.buy_qtys.insert(tick, self.buy_qtys.get(&tick).unwrap_or(&0.0).add(qty));
                    }
                    Err(pos) => {
                        // 如果容量已满，则移除第一个元素（最小的 tick）
                        if self.ticks.len() == self.ticks_max_len {
                            let removed_tick = self.ticks.remove(0);
                            self.buy_qtys.remove(&removed_tick);
                            // 由于已经移除一个元素，原先的 pos 需要调整：
                            let new_pos = if pos > 0 { pos - 1 } else { 0 };
                            self.ticks.insert(new_pos, tick);
                        } else {
                            self.ticks.insert(pos, tick);
                        }
                        self.buy_qtys.insert(tick, qty);
                    }
                }
            }
            Side::Sell => {
                match self.ticks.binary_search(&tick) {
                    // 如果存在，则只更新 qtys
                    Ok(_pos) => {
                        self.sell_qtys.insert(tick, self.sell_qtys.get(&tick).unwrap_or(&0.0).add(qty));
                    }
                    Err(pos) => {
                        // 如果容量已满，则移除第一个元素（最小的 tick）
                        if self.ticks.len() == self.ticks_max_len {
                            let removed_tick = self.ticks.remove(0);
                            self.sell_qtys.remove(&removed_tick);
                            // 由于已经移除一个元素，原先的 pos 需要调整：
                            let new_pos = if pos > 0 { pos - 1 } else { 0 };
                            self.ticks.insert(new_pos, tick);
                        } else {
                            self.ticks.insert(pos, tick);
                        }
                        self.sell_qtys.insert(tick, qty);
                    }
                }
            }
            _ => {}
        }
    }

}