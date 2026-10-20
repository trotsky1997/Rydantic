//! Named model routes, ordered transient failover and atomic admission budgets.
//! Ledgers are process-local. Reservations bound admission; upstream usage can
//! exceed an estimate. Unknown/cancelled attempts conservatively consume their reservation.
use crate::{
    BoxFuture, Capabilities, Error, ModelProvider, ModelRequest, ModelResponse, Result, RunContext,
    Usage,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    sync::{Arc, Mutex},
};
use tracing::Instrument;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Price {
    pub input_microusd_per_million: u64,
    pub output_microusd_per_million: u64,
}
impl Price {
    pub fn cost(self, usage: Usage) -> Result<u64> {
        let input = u128::from(self.input_microusd_per_million) * u128::from(usage.input_tokens);
        let output = u128::from(self.output_microusd_per_million) * u128::from(usage.output_tokens);
        let total = input.checked_add(output).ok_or(Error::Budget)?;
        u64::try_from(total.div_ceil(1_000_000)).map_err(|_| Error::Budget)
    }
}

#[derive(Clone)]
pub struct Route {
    pub provider: Arc<dyn ModelProvider>,
    pub model: String,
    /// Operator-maintained rates; subscription usage may intentionally have no dollar estimate.
    pub price: Option<Price>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub max_requests: u64,
    pub max_tokens: u64,
    pub max_cost_microusd: Option<u64>,
    /// Admission estimate for this ledger, independent of other ledgers.
    pub reservation_tokens: u64,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            max_requests: 10_000,
            max_tokens: 10_000_000,
            max_cost_microusd: None,
            reservation_tokens: 4096,
        }
    }
}

pub struct ClientPolicy {
    pub id: String,
    pub key: String,
    pub models: HashSet<String>,
    pub budget: Budget,
}
impl fmt::Debug for ClientPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientPolicy")
            .field("id", &self.id)
            .field("key", &"[redacted]")
            .field("models", &self.models)
            .field("budget", &self.budget)
            .finish()
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    pub requests: u64,
    pub tokens: u64,
    pub reserved_tokens: u64,
    pub known_cost_microusd: u64,
    pub reserved_cost_microusd: u64,
}
struct Ledger {
    global: BudgetSnapshot,
    clients: BTreeMap<String, BudgetSnapshot>,
}
struct Inner {
    routes: BTreeMap<String, Vec<Route>>,
    policies: Vec<ClientPolicy>,
    global: Budget,
    ledger: Mutex<Ledger>,
}

#[derive(Clone)]
pub struct Gateway {
    inner: Arc<Inner>,
}
#[derive(Clone)]
pub struct GatewayClient {
    inner: Arc<Inner>,
    id: String,
}

impl Gateway {
    pub fn new(
        routes: BTreeMap<String, Vec<Route>>,
        policies: Vec<ClientPolicy>,
        global: Budget,
    ) -> Result<Self> {
        if routes.is_empty()
            || routes.iter().any(|(name, routes)| {
                name.is_empty()
                    || name.len() > 256
                    || routes.is_empty()
                    || routes
                        .iter()
                        .any(|r| r.model.is_empty() || r.model.len() > 256)
            })
            || global.reservation_tokens == 0
        {
            return Err(Error::Configuration(
                "gateway requires named routes and positive reservations".into(),
            ));
        }
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        for policy in &policies {
            if policy.id.is_empty()
                || policy.key.len() < 16
                || !ids.insert(policy.id.clone())
                || !keys.insert(policy.key.clone())
                || policy.budget.reservation_tokens == 0
            {
                return Err(Error::Configuration(
                    "unique client IDs/keys (16+ bytes) and positive reservations are required"
                        .into(),
                ));
            }
            if policy.models.is_empty() || policy.models.iter().any(|m| !routes.contains_key(m)) {
                return Err(Error::Configuration(
                    "client policy references an unknown route".into(),
                ));
            }
            if policy.budget.max_cost_microusd.is_some() || global.max_cost_microusd.is_some() {
                for name in &policy.models {
                    if routes[name].iter().any(|r| r.price.is_none()) {
                        return Err(Error::Configuration(
                            "cost limits require prices for every allowed upstream".into(),
                        ));
                    }
                }
            }
        }
        let clients = policies
            .iter()
            .map(|p| (p.id.clone(), BudgetSnapshot::default()))
            .collect();
        Ok(Self {
            inner: Arc::new(Inner {
                routes,
                policies,
                global,
                ledger: Mutex::new(Ledger {
                    global: BudgetSnapshot::default(),
                    clients,
                }),
            }),
        })
    }
    pub fn client(&self, key: &str) -> Result<GatewayClient> {
        self.inner
            .policies
            .iter()
            .find(|p| equal_key(p.key.as_bytes(), key.as_bytes()))
            .map(|p| GatewayClient {
                inner: self.inner.clone(),
                id: p.id.clone(),
            })
            .ok_or(Error::Unauthorized)
    }
    pub fn snapshot(&self, id: &str) -> Result<BudgetSnapshot> {
        self.inner
            .ledger
            .lock()
            .map_err(|_| Error::Budget)?
            .clients
            .get(id)
            .copied()
            .ok_or(Error::Unauthorized)
    }
    pub fn global_snapshot(&self) -> Result<BudgetSnapshot> {
        Ok(self.inner.ledger.lock().map_err(|_| Error::Budget)?.global)
    }
}

fn equal_key(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(
            a.get(i).copied().unwrap_or_default() ^ b.get(i).copied().unwrap_or_default(),
        );
    }
    diff == 0
}
fn admission(
    current: BudgetSnapshot,
    budget: &Budget,
    tokens: u64,
    cost: u64,
) -> Result<BudgetSnapshot> {
    let next = BudgetSnapshot {
        requests: current.requests.checked_add(1).ok_or(Error::Budget)?,
        reserved_tokens: current
            .reserved_tokens
            .checked_add(tokens)
            .ok_or(Error::Budget)?,
        reserved_cost_microusd: current
            .reserved_cost_microusd
            .checked_add(cost)
            .ok_or(Error::Budget)?,
        ..current
    };
    if next.requests > budget.max_requests
        || next
            .tokens
            .checked_add(next.reserved_tokens)
            .ok_or(Error::Budget)?
            > budget.max_tokens
        || budget.max_cost_microusd.is_some_and(|limit| {
            next.known_cost_microusd
                .checked_add(next.reserved_cost_microusd)
                .is_none_or(|n| n > limit)
        })
    {
        return Err(Error::Budget);
    }
    Ok(next)
}

#[derive(Clone, Copy)]
struct ReservationAmounts {
    tokens: u64,
    cost: u64,
}
impl ReservationAmounts {
    fn estimate(tokens: u64, price: Option<Price>) -> Result<Self> {
        let cost = price
            .map(|p| {
                p.cost(Usage {
                    input_tokens: tokens,
                    output_tokens: tokens,
                })
            })
            .transpose()?
            .unwrap_or(0);
        Ok(Self { tokens, cost })
    }
}

struct Reservation {
    inner: Arc<Inner>,
    id: String,
    client: ReservationAmounts,
    global: ReservationAmounts,
    settled: bool,
}
impl Reservation {
    fn new(inner: Arc<Inner>, policy: &ClientPolicy, price: Option<Price>) -> Result<Self> {
        let client = ReservationAmounts::estimate(policy.budget.reservation_tokens, price)?;
        let global = ReservationAmounts::estimate(inner.global.reservation_tokens, price)?;
        {
            let mut ledger = inner.ledger.lock().map_err(|_| Error::Budget)?;
            let client = admission(
                *ledger.clients.get(&policy.id).ok_or(Error::Unauthorized)?,
                &policy.budget,
                client.tokens,
                client.cost,
            )?;
            let global = admission(ledger.global, &inner.global, global.tokens, global.cost)?;
            ledger.clients.insert(policy.id.clone(), client);
            ledger.global = global;
        }
        Ok(Self {
            inner,
            id: policy.id.clone(),
            client,
            global,
            settled: false,
        })
    }
    fn settle(mut self, usage: Usage, price: Option<Price>) -> Result<()> {
        let actual = ReservationAmounts {
            tokens: usage.total()?,
            cost: price.map(|p| p.cost(usage)).transpose()?.unwrap_or(0),
        };
        self.charge(actual, actual)?;
        self.settled = true;
        Ok(())
    }
    fn charge(&self, client: ReservationAmounts, global: ReservationAmounts) -> Result<()> {
        let mut ledger = self.inner.ledger.lock().map_err(|_| Error::Budget)?;
        let update = |s: BudgetSnapshot,
                      reserved: ReservationAmounts,
                      charged: ReservationAmounts|
         -> Result<BudgetSnapshot> {
            Ok(BudgetSnapshot {
                tokens: s.tokens.checked_add(charged.tokens).ok_or(Error::Budget)?,
                known_cost_microusd: s
                    .known_cost_microusd
                    .checked_add(charged.cost)
                    .ok_or(Error::Budget)?,
                reserved_tokens: s
                    .reserved_tokens
                    .checked_sub(reserved.tokens)
                    .ok_or(Error::Budget)?,
                reserved_cost_microusd: s
                    .reserved_cost_microusd
                    .checked_sub(reserved.cost)
                    .ok_or(Error::Budget)?,
                ..s
            })
        };
        let client = update(
            *ledger.clients.get(&self.id).ok_or(Error::Unauthorized)?,
            self.client,
            client,
        )?;
        let global = update(ledger.global, self.global, global)?;
        ledger.clients.insert(self.id.clone(), client);
        ledger.global = global;
        Ok(())
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self.charge(self.client, self.global);
        }
    }
}

impl GatewayClient {
    pub fn models(&self) -> Vec<String> {
        self.inner
            .policies
            .iter()
            .find(|p| p.id == self.id)
            .map(|p| {
                let mut m: Vec<_> = p.models.iter().cloned().collect();
                m.sort();
                m
            })
            .unwrap_or_default()
    }
}
impl ModelProvider for GatewayClient {
    fn capabilities(&self) -> Capabilities {
        let targets = self.models().into_iter().flat_map(|name| {
            self.inner.routes[&name]
                .iter()
                .map(|r| r.provider.capabilities())
                .collect::<Vec<_>>()
        });
        targets.fold(Capabilities::default(), |all, c| Capabilities {
            native_json_schema: all.native_json_schema && c.native_json_schema,
            output_token_limit: all.output_token_limit && c.output_token_limit,
        })
    }
    fn complete(
        &self,
        request: ModelRequest,
        context: RunContext,
    ) -> BoxFuture<'_, Result<ModelResponse>> {
        Box::pin(async move {
            request.validate()?;
            let policy = self
                .inner
                .policies
                .iter()
                .find(|p| p.id == self.id)
                .ok_or(Error::Unauthorized)?;
            if !policy.models.contains(&request.model) {
                return Err(Error::UnknownRoute);
            }
            let routes = self
                .inner
                .routes
                .get(&request.model)
                .ok_or(Error::UnknownRoute)?;
            for (index, route) in routes.iter().enumerate() {
                context.check()?;
                let reservation = Reservation::new(self.inner.clone(), policy, route.price)?;
                let emitted = context.emitted();
                let mut routed = request.clone();
                routed.model.clone_from(&route.model);
                let result = context.wait(route.provider.complete(routed, context.clone())).instrument(tracing::info_span!("rydantic_ai.gateway", route = %request.model, upstream = %route.model, attempt = index)).await;
                match result {
                    Ok(response) => {
                        reservation.settle(response.usage, route.price)?;
                        return Ok(response);
                    }
                    Err(error)
                        if error.retryable()
                            && context.emitted() == emitted
                            && index + 1 < routes.len() =>
                    {
                        drop(reservation);
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(Error::UnknownRoute)
        })
    }
}

#[cfg(feature = "http")]
mod server;
#[cfg(feature = "http")]
pub use server::router;
