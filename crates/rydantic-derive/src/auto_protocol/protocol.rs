use super::types::{Model, Registry, Spec};
use super::{attrs, ordinal, ordinals, protocol_name, stable_id, wire_name};
use proc_macro2::Span;
use std::collections::{BTreeMap, BTreeSet};
use syn::{spanned::Spanned, Data, Fields};

struct Generator<'a> {
    registry: &'a Registry,
    helpers: BTreeMap<String, String>,
    reserved: BTreeSet<String>,
}
impl Generator<'_> {
    fn helper(
        &mut self,
        kind: &str,
        value: &str,
        body: impl FnOnce(&str) -> String,
    ) -> syn::Result<String> {
        let name = format!("Rydantic{kind}{:016x}", stable_id(value));
        if self.reserved.contains(&name) {
            return Err(syn::Error::new(
                Span::call_site(),
                format!("model name {name} is reserved for an inferred container"),
            ));
        }
        self.helpers
            .entry(name.clone())
            .or_insert_with(|| body(&name));
        Ok(name)
    }
    fn wire(&mut self, spec: &Spec, text: bool) -> syn::Result<String> {
        Ok(match spec {
            Spec::Scalar(name) => {
                if text && name == "bool" {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        "capnp_text supports strings and numeric scalars",
                    ));
                }
                if text && name != "String" {
                    "Text".into()
                } else {
                    match name.as_str() {
                        "bool" => "Bool",
                        "i8" => "Int8",
                        "i16" => "Int16",
                        "i32" => "Int32",
                        "i64" | "isize" => "Int64",
                        "u8" => "UInt8",
                        "u16" => "UInt16",
                        "u32" => "UInt32",
                        "u64" | "usize" => "UInt64",
                        "f32" => "Float32",
                        "f64" => "Float64",
                        "String" | "u128" | "i128" => "Text",
                        _ => unreachable!(),
                    }
                    .into()
                }
            }
            Spec::Text(..) => "Text".into(),
            Spec::Model { name, .. } => name.clone(),
            Spec::Boxed(inner) => self.wire(inner, text)?,
            Spec::Optional(inner) => {
                let value = self.wire(inner, text)?;
                self.helper("Optional", &value, |name| {
                    format!("struct {name} {{ union {{ none @0 :Void; some @1 :{value}; }} }}\n")
                })?
            }
            Spec::List(_) if spec.bytes() => "Data".into(),
            Spec::List(inner) | Spec::Array(inner, _) => {
                format!("List({})", self.wire(inner, text)?)
            }
            Spec::Map { value, .. } => {
                let value = self.wire(value, text)?;
                let entry = self.helper("Entry", &value, |name| {
                    format!("struct {name} {{ key @0 :Text; value @1 :{value}; }}\n")
                })?;
                format!("List({entry})")
            }
        })
    }
    fn field(
        &mut self,
        field: &syn::Field,
        index: usize,
        count: usize,
        scope: &[String],
    ) -> syn::Result<Option<(String, String, Option<u16>)>> {
        let attr = attrs::field(&field.attrs)?;
        if attr.capnp_skip {
            return Ok(None);
        }
        let spec = self.registry.resolve(&field.ty, scope, &mut Vec::new())?;
        let wire = self.wire(&spec, attr.capnp_text || spec.hints().text)?;
        Ok(Some((
            wire_name(field, index, count)?,
            wire,
            attr.capnp_ordinal,
        )))
    }
    fn fields(
        &mut self,
        fields: &Fields,
        scope: &[String],
    ) -> syn::Result<Vec<(String, String, Option<u16>)>> {
        let mut result = vec![];
        let mut names = BTreeSet::new();
        for (index, field) in fields.iter().enumerate() {
            if let Some(entry) = self.field(field, index, fields.len(), scope)? {
                if !names.insert(entry.0.clone()) {
                    return Err(syn::Error::new(
                        field.span(),
                        "duplicate native Cap'n Proto field mapping",
                    ));
                }
                result.push(entry);
            }
        }
        Ok(result)
    }
    fn model(&mut self, model: &Model) -> syn::Result<String> {
        let name = &model.name;
        match &model.input.data {
            Data::Struct(data) => {
                let fields = self.fields(&data.fields, &model.scope)?;
                let ids = ordinals(
                    &fields.iter().map(|f| f.2).collect::<Vec<_>>(),
                    data.fields.span(),
                )?;
                let body = fields
                    .iter()
                    .zip(ids)
                    .map(|((name, ty, _), id)| format!("  {name} @{id} :{ty};\n"))
                    .collect::<String>();
                Ok(format!("struct {name} {{\n{body}}}\n"))
            }
            Data::Enum(data) if model.unit_enum => {
                if data.variants.is_empty() {
                    return Err(syn::Error::new(
                        model.input.span(),
                        "a model enum requires at least one variant",
                    ));
                }
                let ids = ordinals(
                    &data
                        .variants
                        .iter()
                        .map(|v| ordinal(&v.attrs))
                        .collect::<syn::Result<Vec<_>>>()?,
                    data.enum_token.span,
                )?;
                let mut names = BTreeSet::new();
                let mut body = String::new();
                for (variant, id) in data.variants.iter().zip(ids) {
                    let value = protocol_name(
                        &attrs::native_variant_name(&variant.attrs, &variant.ident.to_string())?,
                        false,
                    );
                    if !names.insert(value.clone()) {
                        return Err(syn::Error::new(
                            variant.span(),
                            "duplicate native enum name",
                        ));
                    }
                    body.push_str(&format!("  {value} @{id};\n"));
                }
                Ok(format!(
                    "enum {name} {{\n{body}}}\nstruct Rydantic{name}Root {{ value @0 :{name}; }}\n"
                ))
            }
            Data::Enum(data) => {
                struct Variant {
                    name: String,
                    group: bool,
                    fields: Vec<(String, String, Option<u16>)>,
                    start: usize,
                }
                let mut variants = vec![];
                let mut desired = vec![];
                let mut names = BTreeSet::new();
                for variant in &data.variants {
                    let name = protocol_name(
                        &attrs::native_variant_name(&variant.attrs, &variant.ident.to_string())?,
                        false,
                    );
                    if !names.insert(name.clone()) {
                        return Err(syn::Error::new(
                            variant.span(),
                            "duplicate native union name",
                        ));
                    }
                    let mut fields = self.fields(&variant.fields, &model.scope)?;
                    let group = !fields.is_empty()
                        && !(matches!(variant.fields, Fields::Unnamed(_)) && fields.len() == 1);
                    let start = desired.len();
                    let base = ordinal(&variant.attrs)?;
                    if fields.is_empty() {
                        desired.push(base);
                    } else {
                        for (index, field) in fields.iter_mut().enumerate() {
                            let inherited = base
                                .map(|base| {
                                    base.checked_add(index as u16).ok_or_else(|| {
                                        syn::Error::new(variant.span(), "ordinal exceeds u16")
                                    })
                                })
                                .transpose()?;
                            if field.2.is_some() && inherited.is_some() && field.2 != inherited {
                                return Err(syn::Error::new(
                                    variant.span(),
                                    "variant and field ordinals disagree",
                                ));
                            }
                            field.2 = field.2.or(inherited);
                            desired.push(field.2);
                        }
                    }
                    variants.push(Variant {
                        name,
                        group,
                        fields,
                        start,
                    });
                }
                if variants.is_empty() {
                    return Err(syn::Error::new(
                        model.input.span(),
                        "a model enum requires at least one variant",
                    ));
                }
                // The native format requires two union members. The hidden member is rejected by the model.
                let single = variants.len() == 1;
                if single {
                    desired.push(if desired.iter().any(Option::is_some) {
                        Some(desired.len() as u16)
                    } else {
                        None
                    });
                }
                let ids = ordinals(&desired, model.input.span())?;
                let mut body = String::new();
                for variant in variants {
                    let id = ids[variant.start];
                    let name = variant.name;
                    if variant.fields.is_empty() {
                        body.push_str(&format!("    {name} @{id} :Void;\n"));
                    } else if !variant.group {
                        body.push_str(&format!("    {name} @{id} :{};\n", variant.fields[0].1));
                    } else {
                        body.push_str(&format!("    {name} :group {{\n"));
                        for (index, (field, ty, _)) in variant.fields.iter().enumerate() {
                            body.push_str(&format!(
                                "      {field} @{} :{ty};\n",
                                ids[variant.start + index]
                            ));
                        }
                        body.push_str("    }\n");
                    }
                }
                if single {
                    let reserved = if names.contains("rydanticReserved") {
                        "rydanticReservedEmpty"
                    } else {
                        "rydanticReserved"
                    };
                    body.push_str(&format!("    {reserved} @{} :Void;\n", ids.last().unwrap()));
                }
                Ok(format!("struct {name} {{\n  union {{\n{body}  }}\n}}\n"))
            }
            Data::Union(_) => unreachable!(),
        }
    }
}
pub(super) fn generate(registry: &Registry, id: u64) -> syn::Result<String> {
    let mut generator = Generator {
        registry,
        helpers: BTreeMap::new(),
        reserved: registry
            .models
            .values()
            .map(|m| m.name.clone())
            .chain(
                registry
                    .models
                    .values()
                    .filter(|m| m.unit_enum)
                    .map(|m| format!("Rydantic{}Root", m.name)),
            )
            .collect(),
    };
    let mut declarations = String::new();
    for model in registry.models.values() {
        declarations.push_str(&generator.model(model)?);
    }
    let helpers = generator.helpers.values().cloned().collect::<String>();
    Ok(format!(
        "# Generated from Rust models by Rydantic.\n@0x{id:016x};\n\n{declarations}{helpers}"
    ))
}
