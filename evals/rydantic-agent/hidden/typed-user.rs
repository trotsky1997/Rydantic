use eval_candidate::parse_user;
use serde_json::json;
#[test]
fn independent_boundaries() {
    for (name, age) in [("ab",18),("八个字以内",120),("12345678",50),("  ",20)] {
        let u = parse_user(json!({"name":name,"age":age})).unwrap();
        assert_eq!(u.name, name); assert_eq!(u.age,age);
    }
    for value in [json!({}),json!({"name":null,"age":18}),json!({"name":"ab","age":null}),
        json!({"name":"a","age":18}),json!({"name":"123456789","age":18}),
        json!({"name":"ab","age":17}),json!({"name":"ab","age":121}),
        json!({"name":"ab","age":"18"}),json!({"name":"ab","age":18.5})] {
        assert!(parse_user(value.clone()).is_err(), "accepted {value}");
    }
}
