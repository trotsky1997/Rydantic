//! Compiler-driven conditional model selection in a renamed consumer.
#[rd::capnp_models(namespace = "rydantic.tests.configured")]
pub mod models {
    #![allow(dead_code)]
    #[cfg(any())]
    use nonexistent::Missing;
    #[cfg(any())]
    mod omitted {
        struct Unsupported<T>(T);
    }
    #[cfg(any())]
    pub struct Generic<T> {
        value: T,
    }
    #[cfg_attr(all(), cfg(any()))]
    pub union NotAModel {
        a: u64,
        b: u32,
    }
    #[cfg(any())]
    pub mod wire {}

    #[cfg(feature = "cfg-fixture")]
    pub type Selected = u32;
    #[cfg(not(feature = "cfg-fixture"))]
    pub type Selected = u64;

    #[cfg(target_pointer_width = "64")]
    pub type TargetValue = u64;
    #[cfg(not(target_pointer_width = "64"))]
    pub type TargetValue = u32;

    #[cfg_attr(all(), derive(rd::Model, Debug, PartialEq))]
    pub struct Active {
        #[cfg_attr(all(), model(capnp_name = "selected"))]
        pub first: Selected,
        pub target_value: TargetValue,
        #[cfg(any())]
        pub unsupported: std::net::TcpStream,
        #[cfg_attr(all(), cfg_attr(all(), cfg(any())))]
        pub hidden: Unknown,
        pub nested: nested::Child,
        pub choice: Choice,
    }
    #[derive(rd::Model, Debug, PartialEq)]
    pub struct Tuple(
        #[cfg(any())] Unknown,
        pub u64,
        #[cfg_attr(all(), cfg(any()))] Unknown,
        pub String,
    );

    #[derive(rd::Model, Debug, PartialEq)]
    pub enum Choice {
        First,
        #[cfg(any())]
        Inactive(Unknown),
        Last,
    }
    pub mod nested {
        #[cfg(any())]
        pub struct Generic<T>(T);
        #[derive(rd::Model, Debug, PartialEq)]
        pub struct Child {
            pub value: u32,
        }
    }
}

#[test]
fn inactive_items_fields_and_variants_do_not_enter_schema() {
    use rd::CapnpModel;
    let value = models::Active {
        first: 7,
        target_value: 9,
        nested: models::nested::Child { value: 8 },
        choice: models::Choice::Last,
    };
    let bytes = value.dump_capnp().unwrap();
    assert_eq!(models::Active::parse_capnp(&bytes).unwrap(), value);
    let schema = models::CAPNP_SCHEMA;
    #[cfg(feature = "cfg-fixture")]
    assert!(schema.contains("selected @0 :UInt32"));
    #[cfg(not(feature = "cfg-fixture"))]
    assert!(schema.contains("selected @0 :UInt64"));
    assert!(!schema.contains("Generic"));
    assert!(!schema.contains("unsupported"));
    assert!(!schema.contains("hidden"));
    assert!(!schema.contains("inactive"));
}

#[rd::capnp_models]
#[cfg(any())]
mod entirely_disabled {
    struct Generic<T>(T);
}

#[test]
fn configured_tuple_fields_keep_their_active_positions() {
    use rd::CapnpModel;
    let value = models::Tuple(42, "kept".to_owned());
    assert_eq!(
        models::Tuple::parse_capnp(&value.dump_capnp().unwrap()).unwrap(),
        value
    );
}
