use std::collections::{HashMap, hash_map::Entry};

use crate::{
    backtest::{
        BacktestError,
        assettype::AssetType,
        models::{FeeModel, LatencyModel},
        order::OrderBus,
        proc::{LocalProcessor, Processor},
        state::State,
    },
    depth::{L2MarketDepth, MarketDepth},
    priceaction::{KLine, PriceAction}, 
    types::{
        Event,
        LOCAL_ASK_DEPTH_CLEAR_EVENT,
        LOCAL_ASK_DEPTH_EVENT,
        LOCAL_ASK_DEPTH_SNAPSHOT_EVENT,
        LOCAL_BID_DEPTH_CLEAR_EVENT,
        LOCAL_BID_DEPTH_EVENT,
        LOCAL_BID_DEPTH_SNAPSHOT_EVENT,
        LOCAL_DEPTH_CLEAR_EVENT,
        LOCAL_EVENT,
        LOCAL_TRADE_EVENT,
        LOCAL_BUY_TRADE_EVENT,
        OrdType,
        Order,
        OrderId,
        Side,
        StateValues,
        Status,
        TimeInForce,
    },
};

/// The local model.
pub struct Local<AT, LM, MD, FM, PA>
where
    AT: AssetType,
    LM: LatencyModel,
    MD: MarketDepth,
    FM: FeeModel,
    PA: PriceAction,
{
    orders: HashMap<OrderId, Order>,
    orders_to: OrderBus,
    orders_from: OrderBus,
    depth: MD,
    state: State<AT, FM>,
    order_latency: LM,
    trades: Vec<Event>,
    last_feed_latency: Option<(i64, i64)>,
    last_order_latency: Option<(i64, i64, i64)>,
    price_action: PA,
}

impl<AT, LM, MD, FM, PA> Local<AT, LM, MD, FM, PA>
where
    AT: AssetType,
    LM: LatencyModel,
    MD: MarketDepth,
    FM: FeeModel,
    PA: PriceAction,
{
    /// Constructs an instance of `Local`.
    pub fn new(
        depth: MD,
        state: State<AT, FM>,
        order_latency: LM,
        last_trades_cap: usize,
        orders_to: OrderBus,
        orders_from: OrderBus,
        price_action: PA,
    ) -> Self {
        Self {
            orders: Default::default(),
            orders_to,
            orders_from,
            depth,
            state,
            order_latency,
            trades: Vec::with_capacity(last_trades_cap),
            last_feed_latency: None,
            last_order_latency: None,
            price_action,
        }
    }

    fn process_recv_order_(&mut self, order: Order) -> Result<(), BacktestError> {
        if order.status == Status::Filled {
            self.state.apply_fill(&order);
        }
        // Applies the received order response to the local orders.
        match self.orders.entry(order.order_id) {
            Entry::Occupied(mut entry) => {
                let local_order = entry.get_mut();
                if order.req == Status::Rejected {
                    if order.local_timestamp == local_order.local_timestamp {
                        if local_order.req == Status::New {
                            local_order.req = Status::None;
                            local_order.status = Status::Expired;
                        } else {
                            local_order.req = Status::None;
                        }
                    }
                } else {
                    local_order.update(&order);
                }
            }
            Entry::Vacant(entry) => {
                if order.req != Status::Rejected {
                    entry.insert(order);
                }
            }
        }
        Ok(())
    }
}

impl<AT, LM, MD, FM, PA> LocalProcessor<MD,PA> for Local<AT, LM, MD, FM, PA>
where
    AT: AssetType,
    LM: LatencyModel,
    MD: MarketDepth + L2MarketDepth,
    FM: FeeModel,
    PA: PriceAction,
{
    fn submit_order(
        &mut self,
        order_id: OrderId,
        side: Side,
        price: f64,
        qty: f64,
        order_type: OrdType,
        time_in_force: TimeInForce,
        current_timestamp: i64,
    ) -> Result<(), BacktestError> {
        if self.orders.contains_key(&order_id) {
            return Err(BacktestError::OrderIdExist);
        }

        let price_tick = (price / self.depth.tick_size()).round() as i64;
        let mut order = Order::new(
            order_id,
            price_tick,
            self.depth.tick_size(),
            qty,
            side,
            order_type,
            time_in_force,
        );
        order.req = Status::New;
        order.local_timestamp = current_timestamp;
        self.orders.insert(order.order_id, order.clone());

        let order_entry_latency = self.order_latency.entry(current_timestamp, &order);
        // Negative latency indicates that the order is rejected for technical reasons, and its
        // value represents the latency that the local experiences when receiving the rejection
        // notification.
        if order_entry_latency < 0 {
            // Rejects the order.
            order.req = Status::Rejected;
            let rej_recv_timestamp = current_timestamp - order_entry_latency;
            self.orders_from.append(order, rej_recv_timestamp);
        } else {
            let exch_recv_timestamp = current_timestamp + order_entry_latency;
            self.orders_to.append(order, exch_recv_timestamp);
        }
        Ok(())
    }

    fn modify(
        &mut self,
        order_id: OrderId,
        price: f64,
        qty: f64,
        current_timestamp: i64,
    ) -> Result<(), BacktestError> {
        let order = self
            .orders
            .get_mut(&order_id)
            .ok_or(BacktestError::OrderNotFound)?;

        if order.req != Status::None {
            return Err(BacktestError::OrderRequestInProcess);
        }

        let orig_price_tick = order.price_tick;
        let orig_qty = order.qty;

        let price_tick = (price / self.depth.tick_size()).round() as i64;
        order.price_tick = price_tick;
        order.qty = qty;

        order.req = Status::Replaced;
        order.local_timestamp = current_timestamp;

        let order_entry_latency = self.order_latency.entry(current_timestamp, order);
        // Negative latency indicates that the order is rejected for technical reasons, and its
        // value represents the latency that the local experiences when receiving the rejection
        // notification.
        if order_entry_latency < 0 {
            // Rejects the order.
            let mut order_ = order.clone();
            order_.req = Status::Rejected;
            order_.price_tick = orig_price_tick;
            order_.qty = orig_qty;
            let rej_recv_timestamp = current_timestamp - order_entry_latency;
            self.orders_from.append(order_, rej_recv_timestamp);
        } else {
            let exch_recv_timestamp = current_timestamp + order_entry_latency;
            self.orders_to.append(order.clone(), exch_recv_timestamp);
        }
        Ok(())
    }

    fn cancel(&mut self, order_id: OrderId, current_timestamp: i64) -> Result<(), BacktestError> {
        let order = self
            .orders
            .get_mut(&order_id)
            .ok_or(BacktestError::OrderNotFound)?;

        if order.req != Status::None {
            return Err(BacktestError::OrderRequestInProcess);
        }

        order.req = Status::Canceled;
        let order_entry_latency = self.order_latency.entry(current_timestamp, order);
        // Negative latency indicates that the order is rejected for technical reasons, and its
        // value represents the latency that the local experiences when receiving the rejection
        // notification.
        if order_entry_latency < 0 {
            // Rejects the order.
            let mut order_ = order.clone();
            order_.req = Status::Rejected;
            let rej_recv_timestamp = current_timestamp - order_entry_latency;
            self.orders_from.append(order_, rej_recv_timestamp);
        } else {
            let exch_recv_timestamp = current_timestamp + order_entry_latency;
            self.orders_to.append(order.clone(), exch_recv_timestamp);
        }
        Ok(())
    }

    fn clear_inactive_orders(&mut self) {
        self.orders.retain(|_, order| {
            order.status != Status::Expired
                && order.status != Status::Filled
                && order.status != Status::Canceled
        })
    }

    fn position(&self) -> f64 {
        self.state.values().position
    }

    fn state_values(&self) -> &StateValues {
        self.state.values()
    }

    fn depth(&self) -> &MD {
        &self.depth
    }

    fn price_action(&self) -> &PA{
        &self.price_action
    }

    fn orders(&self) -> &HashMap<u64, Order> {
        &self.orders
    }

    fn last_trades(&self) -> &[Event] {
        self.trades.as_slice()
    }

    fn clear_last_trades(&mut self) {
        self.trades.clear();
    }

    fn feed_latency(&self) -> Option<(i64, i64)> {
        self.last_feed_latency
    }

    fn order_latency(&self) -> Option<(i64, i64, i64)> {
        self.last_order_latency
    }
}

impl<AT, LM, MD, FM, PA> Processor for Local<AT, LM, MD, FM, PA>
where
    AT: AssetType,
    LM: LatencyModel,
    MD: MarketDepth + L2MarketDepth,
    FM: FeeModel,
    PA: PriceAction,
{
    fn event_seen_timestamp(&self, event: &Event) -> Option<i64> {
        event.is(LOCAL_EVENT).then_some(event.local_ts)
    }

    fn process(&mut self, ev: &Event) -> Result<(), BacktestError> {
        // Processes a depth event
        if ev.is(LOCAL_BID_DEPTH_CLEAR_EVENT) {
            self.depth.clear_depth(Side::Buy, ev.px);
        } else if ev.is(LOCAL_ASK_DEPTH_CLEAR_EVENT) {
            self.depth.clear_depth(Side::Sell, ev.px);
        } else if ev.is(LOCAL_DEPTH_CLEAR_EVENT) {
            self.depth.clear_depth(Side::None, 0.0);
        } else if ev.is(LOCAL_BID_DEPTH_EVENT) || ev.is(LOCAL_BID_DEPTH_SNAPSHOT_EVENT) {
            self.depth.update_bid_depth(ev.px, ev.qty, ev.local_ts);
        } else if ev.is(LOCAL_ASK_DEPTH_EVENT) || ev.is(LOCAL_ASK_DEPTH_SNAPSHOT_EVENT) {
            self.depth.update_ask_depth(ev.px, ev.qty, ev.local_ts);
        }
        // Processes a trade event
        else if ev.is(LOCAL_TRADE_EVENT) {
            if self.trades.capacity() > 0 {
                self.trades.push(ev.clone());
            }
            if ev.is(LOCAL_BUY_TRADE_EVENT) {
                self.price_action.order_flow(ev.px, self.depth.tick_size(), ev.qty, ev.local_ts, Side::Buy);
            } else {
                self.price_action.order_flow(ev.px, self.depth.tick_size(), ev.qty, ev.local_ts, Side::Sell);
            }
        }

        // Stores the current feed latency
        self.last_feed_latency = Some((ev.exch_ts, ev.local_ts));

        Ok(())
    }

    fn process_recv_order(
        &mut self,
        timestamp: i64,
        wait_resp_order_id: Option<OrderId>,
    ) -> Result<bool, BacktestError> {
        // Processes the order part.
        let mut wait_resp_order_received = false;
        while !self.orders_from.is_empty() {
            let recv_timestamp = self.orders_from.earliest_timestamp().unwrap();
            if timestamp == recv_timestamp {
                let (order, _) = self.orders_from.pop_front().unwrap();

                // Updates the order latency only if it has a valid exchange timestamp. When the
                // order is rejected before it reaches the matching engine, it has no exchange
                // timestamp. This situation occurs in crypto exchanges.
                if order.exch_timestamp > 0 {
                    self.last_order_latency =
                        Some((order.local_timestamp, order.exch_timestamp, recv_timestamp));
                }

                if let Some(wait_resp_order_id) = wait_resp_order_id {
                    if order.order_id == wait_resp_order_id {
                        wait_resp_order_received = true;
                    }
                }

                self.process_recv_order_(order)?;
            } else {
                assert!(recv_timestamp > timestamp);
                break;
            }
        }
        Ok(wait_resp_order_received)
    }

    fn earliest_recv_order_timestamp(&self) -> i64 {
        self.orders_from.earliest_timestamp().unwrap_or(i64::MAX)
    }

    fn earliest_send_order_timestamp(&self) -> i64 {
        self.orders_to.earliest_timestamp().unwrap_or(i64::MAX)
    }
}
