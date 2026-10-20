use rydantic::Model;
mod generated {
    include!("fixtures/enum.txt");
}
#[test]
fn maximum_enum_compiles_under_default_rust_recursion_limit() {
    let schema = serde_json::from_str(include_str!("fixtures/enum.json")).unwrap();
    assert_eq!(
        rydantic_codegen::generate(&schema).unwrap(),
        include_str!("fixtures/enum.txt")
    );
    for i in 0..64 {
        assert!(
            generated::RootModel::parse_value(serde_json::json!(format!("variant{i}"))).is_ok()
        );
    }
    assert!(generated::RootModel::parse_value(serde_json::json!("variant64")).is_err());
    let schema = serde_json::json!({"type":"string","enum":(0..65).map(|i|format!("variant{i}")).collect::<Vec<_>>()});
    assert_eq!(
        rydantic_codegen::generate(&schema).unwrap_err().pointer,
        "/enum"
    );
}
