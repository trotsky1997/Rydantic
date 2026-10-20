use rydantic_ai::{
    graph::{Checkpoint, Graph, GraphResult, Node, Transition},
    BoxFuture, Error, Result, RunContext,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Key {
    Start,
    End,
}
struct Start;
struct End;
impl Node<i64, i64, Key, i64> for Start {
    fn run<'a>(
        &'a self,
        s: &'a mut i64,
        deps: &'a i64,
        _: &'a RunContext,
    ) -> BoxFuture<'a, Result<Transition<Key, i64>>> {
        Box::pin(async move {
            *s += deps;
            Ok(Transition::Pause(Key::End))
        })
    }
}
impl Node<i64, i64, Key, i64> for End {
    fn run<'a>(
        &'a self,
        s: &'a mut i64,
        deps: &'a i64,
        _: &'a RunContext,
    ) -> BoxFuture<'a, Result<Transition<Key, i64>>> {
        Box::pin(async move { Ok(Transition::End(*s * deps)) })
    }
}
#[tokio::test]
async fn checkpoint_roundtrip_resumes_with_fresh_dependencies() {
    let graph = Graph::new(4)
        .unwrap()
        .node(Key::Start, [Key::End], Start)
        .unwrap()
        .node(Key::End, [], End)
        .unwrap();
    let GraphResult::Paused(checkpoint) = graph
        .run(20, &1, Key::Start, RunContext::default())
        .await
        .unwrap()
    else {
        panic!("expected pause")
    };
    let bytes = serde_json::to_vec(&checkpoint).unwrap();
    let restored: Checkpoint<i64, Key> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored.steps, 1);
    let GraphResult::Completed {
        state,
        output,
        steps,
    } = graph
        .resume(restored, &2, RunContext::default())
        .await
        .unwrap()
    else {
        panic!("expected completion")
    };
    assert_eq!((state, output, steps), (21, 42, 2));
}
#[tokio::test]
async fn step_limits_survive_pause_and_resume() {
    let graph = Graph::new(1)
        .unwrap()
        .node(Key::Start, [Key::End], Start)
        .unwrap()
        .node(Key::End, [], End)
        .unwrap();
    let GraphResult::Paused(checkpoint) = graph
        .run(20, &1, Key::Start, RunContext::default())
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(matches!(
        graph.resume(checkpoint, &2, RunContext::default()).await,
        Err(Error::Limit(_))
    ));
}
#[tokio::test]
async fn unknown_nodes_and_undeclared_edges_are_rejected() {
    let graph = Graph::new(3)
        .unwrap()
        .node(Key::Start, [], Start)
        .unwrap()
        .node(Key::End, [], End)
        .unwrap();
    assert!(matches!(
        graph.run(0, &1, Key::Start, RunContext::default()).await,
        Err(Error::Graph(_))
    ));
    let missing = Graph::new(3)
        .unwrap()
        .node(Key::Start, [Key::End], Start)
        .unwrap();
    assert!(missing.validate().is_err());
    assert!(missing.node(Key::Start, [], Start).is_err());
}
