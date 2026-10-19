use rd::{CapnpModel, Model};

#[rd::capnp_models(namespace = "rydantic.examples.edgeTypes")]
#[allow(non_camel_case_types)]
mod models {
    use rd::chrono::{DateTime, FixedOffset, NaiveDateTime, NaiveTime, Utc};
    use rd::rust_decimal::Decimal;
    pub type Bytes = Vec<u8>;
    pub type Digest = [u8; 2];

    #[derive(Debug, PartialEq)]
    pub struct String {
        pub value: std::string::String,
    }
    #[derive(Debug, PartialEq)]
    pub struct Option {
        pub value: u8,
    }
    #[derive(Debug, PartialEq)]
    pub struct r#type {
        pub r#union: bool,
    }
    #[derive(Debug, PartialEq)]
    pub struct Text {
        pub data: std::string::String,
    }
    #[derive(Debug, PartialEq)]
    pub enum Status {
        #[model(rename = "完成")]
        Done,
        Pending,
    }

    #[derive(Debug, PartialEq)]
    pub struct Scalars {
        pub signed: i128,
        pub floats: [f64; 2],
        #[model(capnp_text)]
        pub float_text: std::option::Option<Vec<f64>>,
        pub words: usize,
        pub offset: isize,
        pub flag: bool,
        pub local_string: String,
        pub local_option: Option,
        pub keyword: r#type,
        pub text_model: Text,
        pub status: Status,
        pub blobs: std::option::Option<Bytes>,
        pub pairs: Vec<Digest>,
        pub maps: std::option::Option<std::collections::BTreeMap<std::string::String, Bytes>>,
        pub nested_option: std::option::Option<std::option::Option<u32>>,
        pub moment: DateTime<Utc>,
        pub fixed: std::option::Option<DateTime<FixedOffset>>,
        pub clock: NaiveTime,
        pub naive: [NaiveDateTime; 1],
        pub decimals: std::option::Option<Vec<Decimal>>,
    }
}

#[test]
fn names_aliases_all_text_scalars_and_nested_optionals_roundtrip() {
    use models::*;
    let scalar = Scalars {
        signed: i128::MIN,
        floats: [1.25, -0.5],
        float_text: Some(vec![1.25, -0.5]),
        words: usize::MAX,
        offset: isize::MIN,
        flag: true,
        local_string: String {
            value: "custom model".into(),
        },
        local_option: Option { value: 42 },
        keyword: r#type { r#union: true },
        text_model: Text {
            data: "native built-in name".into(),
        },
        status: Status::Done,
        blobs: Some(vec![0, 1, 255]),
        pairs: vec![[0, 255]],
        maps: Some([("data".into(), vec![0, 255])].into()),
        nested_option: Some(Some(42)),
        moment: "2026-10-19T00:00:00Z".parse().unwrap(),
        fixed: Some("2026-10-19T12:00:00+08:00".parse().unwrap()),
        clock: "12:34:56".parse().unwrap(),
        naive: ["2026-10-19T12:34:56".parse().unwrap()],
        decimals: Some(vec![rd::rust_decimal::Decimal::MIN]),
    };
    assert_eq!(
        Scalars::parse_capnp(&scalar.dump_capnp().unwrap()).unwrap(),
        scalar
    );
    assert_eq!(
        Scalars::parse_capnp_packed(&scalar.dump_capnp_packed().unwrap()).unwrap(),
        scalar
    );
    assert_eq!(
        Scalars::parse_json(&scalar.dump_json().unwrap()).unwrap(),
        scalar
    );
    for value in [None, Some(None), Some(Some(42))] {
        let scalar = Scalars {
            nested_option: value,
            ..Scalars::parse_capnp(&scalar.dump_capnp().unwrap()).unwrap()
        };
        assert_eq!(
            Scalars::parse_capnp(&scalar.dump_capnp().unwrap()).unwrap(),
            scalar
        );
    }
}
