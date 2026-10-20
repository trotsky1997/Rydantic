use eval_candidate::parse_user;
use serde_json::json;
#[test]
fn independent_normalized_boundaries() {
    let u = parse_user(json!({"name":"  八字  ","age":"18"})).unwrap();
    assert_eq!(u.name,"八字"); assert_eq!(u.age,18);
    let u = parse_user(json!({"name":"12345678","age":120})).unwrap();
    assert_eq!(u.age,120);
    for value in [json!({}),json!({"name":null,"age":18}),json!({"name":"ab","age":null}),
        json!({"name":" a ","age":18}),json!({"name":"   ","age":18}),
        json!({"name":"123456789","age":18}),json!({"name":"ab","age":"bad"}),
        json!({"name":"ab","age":"17"}),json!({"name":"ab","age":"121"})] {
        assert!(parse_user(value.clone()).is_err(), "accepted {value}");
    }
}
