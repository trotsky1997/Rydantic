use rydantic_ai::{
    gateway::{self, Budget, ClientPolicy, Gateway, Price, Route},
    Api, Capabilities, HttpProvider, ModelProvider,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    net::SocketAddr,
    sync::Arc,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    listen: SocketAddr,
    #[serde(default)]
    global: Budget,
    routes: BTreeMap<String, Vec<Upstream>>,
    clients: Vec<Client>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upstream {
    base_url: String,
    model: String,
    #[serde(default)]
    api: Api,
    capabilities: Option<Capabilities>,
    api_key_env: Option<String>,
    price: Option<Price>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Client {
    id: String,
    key_env: String,
    models: HashSet<String>,
    #[serde(default)]
    budget: Budget,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
    let file = std::env::args()
        .nth(1)
        .ok_or("Usage: rydantic-ai-gateway CONFIG.json")?;
    let config: Config = serde_json::from_slice(&std::fs::read(file)?)?;
    let mut routes = BTreeMap::new();
    for (alias, upstreams) in config.routes {
        let mut targets = Vec::new();
        for upstream in upstreams {
            let key = upstream
                .api_key_env
                .map(|name| {
                    std::env::var(&name).map_err(|_| {
                        format!("missing provider credential environment variable {name}")
                    })
                })
                .transpose()?;
            let mut provider = HttpProvider::new(upstream.api, &upstream.base_url, key)?;
            if let Some(capabilities) = upstream.capabilities {
                provider = provider.with_capabilities(capabilities);
            }
            let provider: Arc<dyn ModelProvider> = Arc::new(provider);
            targets.push(Route {
                provider,
                model: upstream.model,
                price: upstream.price,
            });
        }
        routes.insert(alias, targets);
    }
    let clients = config
        .clients
        .into_iter()
        .map(|client| {
            let key = std::env::var(&client.key_env).map_err(|_| {
                format!(
                    "missing gateway credential environment variable {}",
                    client.key_env
                )
            })?;
            Ok(ClientPolicy {
                id: client.id,
                key,
                models: client.models,
                budget: client.budget,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let gateway = Gateway::new(routes, clients, config.global)?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    println!("RydanticAI gateway listening on {}", listener.local_addr()?);
    axum::serve(listener, gateway::router(gateway))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
