//! Typed graph state and dependencies with explicit edges and serializable pause checkpoints.
use crate::{BoxFuture, Error, Result, RunContext};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
};
use tracing::Instrument;

pub enum Transition<K, O> {
    Next(K),
    Pause(K),
    End(O),
}

pub trait Node<S, D, K, O>: Send + Sync {
    fn run<'a>(
        &'a self,
        state: &'a mut S,
        deps: &'a D,
        context: &'a RunContext,
    ) -> BoxFuture<'a, Result<Transition<K, O>>>;
}

/// Contains only application state, the next node and the number of committed steps.
/// Dependencies, credentials and execution context are supplied again on resume.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint<S, K> {
    pub state: S,
    pub next: K,
    pub steps: u64,
}

pub enum GraphResult<S, K, O> {
    Completed { state: S, output: O, steps: u64 },
    Paused(Checkpoint<S, K>),
}

type NodeMap<S, D, K, O> = HashMap<K, (HashSet<K>, Arc<dyn Node<S, D, K, O>>)>;

pub struct Graph<S, D, K, O> {
    nodes: NodeMap<S, D, K, O>,
    max_steps: u64,
}

impl<S, D, K, O> Graph<S, D, K, O>
where
    S: Send,
    D: Send + Sync,
    K: Clone + Eq + Hash + Send + Sync,
    O: Send,
{
    pub fn new(max_steps: u64) -> Result<Self> {
        if max_steps == 0 {
            return Err(Error::Configuration(
                "graph max_steps must be positive".into(),
            ));
        }
        Ok(Self {
            nodes: HashMap::new(),
            max_steps,
        })
    }
    pub fn node(
        mut self,
        key: K,
        targets: impl IntoIterator<Item = K>,
        node: impl Node<S, D, K, O> + 'static,
    ) -> Result<Self> {
        if self.nodes.contains_key(&key) {
            return Err(Error::Graph("duplicate node".into()));
        }
        self.nodes
            .insert(key, (targets.into_iter().collect(), Arc::new(node)));
        Ok(self)
    }
    pub fn validate(&self) -> Result<()> {
        for (edges, _) in self.nodes.values() {
            if edges.iter().any(|key| !self.nodes.contains_key(key)) {
                return Err(Error::Graph("edge targets an unregistered node".into()));
            }
        }
        Ok(())
    }
    pub async fn run(
        &self,
        state: S,
        deps: &D,
        start: K,
        context: RunContext,
    ) -> Result<GraphResult<S, K, O>> {
        self.resume(
            Checkpoint {
                state,
                next: start,
                steps: 0,
            },
            deps,
            context,
        )
        .await
    }
    /// Resumes between nodes. A node's external side effects are application-owned.
    pub async fn resume(
        &self,
        checkpoint: Checkpoint<S, K>,
        deps: &D,
        context: RunContext,
    ) -> Result<GraphResult<S, K, O>> {
        self.validate()?;
        let Checkpoint {
            mut state,
            mut next,
            mut steps,
        } = checkpoint;
        loop {
            context.check()?;
            if steps >= self.max_steps {
                return Err(Error::Limit("graph step limit".into()));
            }
            let (targets, node) = self
                .nodes
                .get(&next)
                .ok_or_else(|| Error::Graph("unknown checkpoint/start node".into()))?;
            let transition = context
                .wait(node.run(&mut state, deps, &context))
                .instrument(tracing::info_span!("rydantic_ai.graph.node", step = steps))
                .await?;
            steps += 1;
            match transition {
                Transition::End(output) => {
                    return Ok(GraphResult::Completed {
                        state,
                        output,
                        steps,
                    })
                }
                Transition::Next(target) | Transition::Pause(target)
                    if !targets.contains(&target) =>
                {
                    return Err(Error::Graph("node returned an undeclared edge".into()))
                }
                Transition::Next(target) => next = target,
                Transition::Pause(target) => {
                    return Ok(GraphResult::Paused(Checkpoint {
                        state,
                        next: target,
                        steps,
                    }))
                }
            }
        }
    }
}
