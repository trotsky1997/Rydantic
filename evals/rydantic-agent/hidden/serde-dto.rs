use eval_candidate::{decode, encode, Message};
#[test]
fn independent_roundtrip_and_rejections() {
    let message = Message { label: "雪 😀".into(), count: u32::MAX };
    assert_eq!(decode(&encode(&message).unwrap()).unwrap(), message);
    for raw in [r#"{}"#, r#"{"label":"x","count":-1}"#, r#"{"label":"x","count":4294967296}"#, r#"{"label":"x","count":"2"}"#, r#"{"label":null,"count":2}"#] {
        assert!(decode(raw).is_err(), "accepted {raw}");
    }
}
