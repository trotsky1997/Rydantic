use rydantic_ai::{
    graph::{Graph, GraphResult, Node, Transition},
    BoxFuture, Result, RunContext,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Step {
    Prepare,
    Finish,
}
struct Prepare;
struct Finish;
impl Node<i64, (), Step, i64> for Prepare {
    fn run<'a>(
        &'a self,
        state: &'a mut i64,
        _: &'a (),
        _: &'a RunContext,
    ) -> BoxFuture<'a, Result<Transition<Step, i64>>> {
        Box::pin(async move {
            *state += 1;
            Ok(Transition::Pause(Step::Finish))
        })
    }
}
impl Node<i64, (), Step, i64> for Finish {
    fn run<'a>(
        &'a self,
        state: &'a mut i64,
        _: &'a (),
        _: &'a RunContext,
    ) -> BoxFuture<'a, Result<Transition<Step, i64>>> {
        Box::pin(async move { Ok(Transition::End(*state * 2)) })
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let graph = Graph::new(10)?
        .node(Step::Prepare, [Step::Finish], Prepare)?
        .node(Step::Finish, [], Finish)?;
    if let GraphResult::Paused(checkpoint) = graph
        .run(20, &(), Step::Prepare, RunContext::default())
        .await?
    {
        let saved = serde_json::to_string(&checkpoint).expect("serializable checkpoint");
        let restored = serde_json::from_str(&saved).expect("valid checkpoint");
        if let GraphResult::Completed { output, steps, .. } =
            graph.resume(restored, &(), RunContext::default()).await?
        {
            println!("output={output}, steps={steps}");
        }
    }
    Ok(())
}
