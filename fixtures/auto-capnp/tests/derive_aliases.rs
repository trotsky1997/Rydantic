use rd::{CapnpModel, Model as OuterModel};

#[rd::capnp_models(namespace = "tests.explicitDeriveAliases")]
mod models {
    use rd::{Model as LocalModel, Model as RenamedModel};

    #[derive(LocalModel, Debug, PartialEq)]
    pub struct Local {
        pub value: u64,
    }

    #[derive(super::OuterModel, Debug, PartialEq)]
    pub struct Outer {
        pub value: String,
    }

    #[derive(RenamedModel, Debug, PartialEq)]
    pub enum State {
        Off,
        On,
    }

    pub mod nested {
        use super::LocalModel as NestedModel;

        #[derive(NestedModel, Debug, PartialEq)]
        pub struct Nested {
            pub value: super::Local,
        }
    }
}

#[test]
fn rust_resolves_local_outer_and_nested_derive_aliases() {
    fn roundtrip<T: CapnpModel + std::fmt::Debug + PartialEq>(value: T) {
        assert_eq!(T::parse_capnp(&value.dump_capnp().unwrap()).unwrap(), value);
        assert_eq!(T::parse_json(&value.dump_json().unwrap()).unwrap(), value);
    }
    roundtrip(models::Local { value: 42 });
    roundtrip(models::Outer {
        value: "outer".into(),
    });
    roundtrip(models::State::On);
    roundtrip(models::nested::Nested {
        value: models::Local { value: 7 },
    });
}
