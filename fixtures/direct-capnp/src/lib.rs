//! Direct, renamed proc macros with the runtime's derive re-exports disabled.
//! Run this package separately: workspace builds can unify the runtime's features.
use rd_macros::{capnp_models as protocol, Model as Record};

#[protocol(namespace = "rydantic.tests.direct_alias")]
pub mod models {
    use super::Record;

    #[cfg(any())]
    use nonexistent::Missing;
    #[cfg(any())]
    pub struct Unsupported<T>(T);

    #[cfg(feature = "cfg-fixture")]
    type Value = u32;
    #[cfg(not(feature = "cfg-fixture"))]
    type Value = u64;

    #[cfg_attr(all(), derive(Record, Debug, PartialEq))]
    pub struct Item {
        pub value: Value,
        #[cfg_attr(all(), cfg(any()))]
        pub omitted: Unknown,
    }
}

#[rd_macros::capnp_models(namespace = "rydantic.tests.direct_qualified")]
pub mod qualified {
    #[derive(rd_macros::Model, Debug, PartialEq)]
    pub struct Item {
        pub name: String,
    }
}

#[protocol]
#[cfg(any())]
mod omitted {
    struct Unsupported<T>(T);
}

#[cfg(test)]
mod tests {
    use super::{models, qualified};
    use rd::CapnpModel;

    #[test]
    fn direct_imported_aliases_preserve_consumer_configuration() {
        let value = models::Item { value: 42 };
        assert_eq!(
            models::Item::parse_capnp(&value.dump_capnp().unwrap()).unwrap(),
            value
        );
        #[cfg(feature = "cfg-fixture")]
        assert!(models::CAPNP_SCHEMA.contains("value @0 :UInt32"));
        #[cfg(not(feature = "cfg-fixture"))]
        assert!(models::CAPNP_SCHEMA.contains("value @0 :UInt64"));
        assert!(!models::CAPNP_SCHEMA.contains("Unsupported"));
        assert!(!models::CAPNP_SCHEMA.contains("omitted"));
    }

    #[test]
    fn direct_qualified_macros_round_trip_without_runtime_derive() {
        let value = qualified::Item {
            name: "direct".into(),
        };
        assert_eq!(
            qualified::Item::parse_capnp(&value.dump_capnp().unwrap()).unwrap(),
            value
        );
    }
}
