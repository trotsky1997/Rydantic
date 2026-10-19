use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, parse_quote, spanned::Spanned, Data, DeriveInput, Expr, Fields, Ident, Type,
};

mod attrs;

#[proc_macro_derive(Model, attributes(model))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

struct Field {
    ident: Ident,
    ty: Type,
    attr: attrs::Field,
    input: String,
    output: String,
    doc: Option<String>,
}

fn fields(fields: &Fields, tag: Option<&str>) -> syn::Result<Vec<Field>> {
    if matches!(fields, Fields::Unnamed(_)) {
        return Err(syn::Error::new(
            fields.span(),
            "Model supports named fields and unit enum variants; tuple fields are unsupported",
        ));
    }
    let mut inputs = std::collections::HashSet::new();
    let mut outputs = std::collections::HashSet::new();
    fields
        .iter()
        .map(|field| {
            let ident = field.ident.clone().expect("named field");
            let attr = attrs::field(&field.attrs)?;
            let base = attr
                .rename
                .clone()
                .unwrap_or_else(|| ident.to_string().trim_start_matches("r#").to_owned());
            let input = attr
                .validation_alias
                .clone()
                .unwrap_or_else(|| base.clone());
            let output = attr.serialization_alias.clone().unwrap_or(base);
            if !inputs.insert(input.clone()) || tag == Some(input.as_str()) {
                return Err(syn::Error::new(
                    ident.span(),
                    "duplicate input field name or collision with enum tag",
                ));
            }
            if !attr.exclude && (!outputs.insert(output.clone()) || tag == Some(output.as_str())) {
                return Err(syn::Error::new(
                    ident.span(),
                    "duplicate output field name or collision with enum tag",
                ));
            }
            let doc = attr.description.clone().or_else(|| {
                let text: Vec<_> = field
                    .attrs
                    .iter()
                    .filter_map(|attr| {
                        if !attr.path().is_ident("doc") {
                            return None;
                        }
                        let syn::Meta::NameValue(value) = &attr.meta else {
                            return None;
                        };
                        let Expr::Lit(value) = &value.value else {
                            return None;
                        };
                        let syn::Lit::Str(value) = &value.lit else {
                            return None;
                        };
                        Some(value.value().trim().to_owned())
                    })
                    .collect();
                (!text.is_empty()).then(|| text.join("\n"))
            });
            Ok(Field {
                ident,
                ty: field.ty.clone(),
                attr,
                input,
                output,
                doc,
            })
        })
        .collect()
}

fn option<T: quote::ToTokens>(value: &Option<T>) -> Tokens {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}

fn rules(field: &Field, rd: &Tokens) -> Tokens {
    let a = &field.attr;
    let trim = a.trim;
    let lower = a.lowercase;
    let upper = a.uppercase;
    let email = a.email;
    let min = option(&a.min_length);
    let max = option(&a.max_length);
    let number = |value: &Option<String>| match value {
        Some(value) => quote!(Some(#rd::__private::number(#value))),
        None => quote!(None),
    };
    let ge = number(&a.ge);
    let gt = number(&a.gt);
    let le = number(&a.le);
    let lt = number(&a.lt);
    let pattern = match &a.pattern {
        Some(value) => {
            quote!(Some(#rd::__private::regex::Regex::new(#value).expect("regex checked by Model derive")))
        }
        None => quote!(None),
    };
    quote!({
        static __RYDANTIC_RULES: ::std::sync::OnceLock<#rd::rules::Rules> = ::std::sync::OnceLock::new();
        __RYDANTIC_RULES.get_or_init(|| #rd::rules::Rules {
            trim: #trim, lowercase: #lower, uppercase: #upper, min_length: #min, max_length: #max,
            ge: #ge, gt: #gt, le: #le, lt: #lt, pattern: #pattern, email: #email,
        })
    })
}

fn parse_field(field: &Field, index: usize, rd: &Tokens) -> Tokens {
    let ty = &field.ty;
    let input = &field.input;
    let rule = rules(field, rd);
    let coerce = field.attr.coerce;
    let local = format_ident!("__rydantic_field_{index}");
    let missing_error = quote!({
        __rydantic_ctx.push(#rd::ValidationError::new("missing", "field is required").with_stage(#rd::Stage::Parse));
        None
    });
    let default = if field.attr.default_trait {
        Some(quote!(<#ty as ::core::default::Default>::default()))
    } else if let Some(expr) = &field.attr.default {
        Some(
            if matches!(
                expr,
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(_),
                    ..
                })
            ) {
                quote!((#expr).to_owned())
            } else {
                quote!(#expr)
            },
        )
    } else {
        field
            .attr
            .default_factory
            .as_ref()
            .map(|path| quote!(#path()))
    };
    let missing = if let Some(default) = default {
        quote!({
            let __rydantic_default: #ty = #default;
            Some(<#ty as #rd::JsonType>::encode_input(&__rydantic_default, __rydantic_ctx))
        })
    } else if field.attr.required {
        missing_error.clone()
    } else {
        quote!({
            match <#ty as #rd::JsonType>::missing() {
                Some(__rydantic_default) => Some(<#ty as #rd::JsonType>::encode_input(&__rydantic_default, __rydantic_ctx)),
                None => #missing_error,
            }
        })
    };
    let before = field.attr.before.as_ref().map(|path| quote! {
        __rydantic_raw = match #path(__rydantic_raw) {
            Ok(value) => value,
            Err(error) => { __rydantic_ctx.push(error.with_stage(#rd::Stage::Parse)); return None; }
        };
    });
    let validate = field.attr.validate.as_ref().map(|path| {
        quote! {
            if __rydantic_ctx.error_count() == __rydantic_start {
                if let Err(error) = #path(&__rydantic_value) { __rydantic_ctx.push(error); }
            }
        }
    });
    let builtin = field.attr.has_rules().then(|| quote! {
        let __rydantic_rules = #rule;
        let mut __rydantic_encoded = <#ty as #rd::JsonType>::encode(&__rydantic_value, __rydantic_ctx);
        if __rydantic_rules.normalizes() {
            __rydantic_rules.prepare(&mut __rydantic_encoded);
            __rydantic_value = <#ty as #rd::JsonType>::decode(__rydantic_encoded.clone(), __rydantic_ctx, false)?;
        }
        __rydantic_rules.validate(&__rydantic_encoded, __rydantic_ctx);
    });
    quote! {
        let #local = __rydantic_ctx.at(#input, |__rydantic_ctx| -> Option<#ty> {
            let __rydantic_start = __rydantic_ctx.error_count();
            let mut __rydantic_raw = match __rydantic_object.remove(#input) { Some(value) => Some(value), None => #missing }?;
            #before
            let mut __rydantic_value = <#ty as #rd::JsonType>::decode(__rydantic_raw, __rydantic_ctx, #coerce)?;
            #builtin
            #validate
            Some(__rydantic_value)
        });
    }
}

fn parse_body(
    fields: &[Field],
    constructor: Tokens,
    container: &attrs::Container,
    rd: &Tokens,
) -> Tokens {
    let parse = fields
        .iter()
        .enumerate()
        .map(|(i, f)| parse_field(f, i, rd));
    let locals: Vec<_> = (0..fields.len())
        .map(|i| format_ident!("__rydantic_field_{i}"))
        .collect();
    let extra = (!container.ignore_extra).then(|| quote! {
        for (__rydantic_key, _) in __rydantic_object {
            __rydantic_ctx.at(__rydantic_key, |ctx| ctx.push(#rd::ValidationError::new("extra_forbidden", "unknown field").with_stage(#rd::Stage::Parse)));
        }
    });
    let validate = container.validate.as_ref().map(|path| {
        quote! {
            if __rydantic_ctx.error_count() == __rydantic_model_start {
                if let Err(errors) = #path(&__rydantic_model) { __rydantic_ctx.extend(errors); }
            }
        }
    });
    let complete = if fields.is_empty() {
        quote! {
            let __rydantic_model = #constructor;
            #validate
            Some(__rydantic_model)
        }
    } else {
        quote! {
            match (#(#locals,)*) {
                (#(Some(#locals),)*) => {
                    let __rydantic_model = #constructor;
                    #validate
                    Some(__rydantic_model)
                },
                _ => None,
            }
        }
    };
    quote! {
        let __rydantic_model_start = __rydantic_ctx.error_count();
        #(#parse)*
        #extra
        #complete
    }
}

fn validate_fields(fields: &[Field], values: &[Tokens], rd: &Tokens) -> Tokens {
    let checks = fields.iter().zip(values).map(|(field, value)| {
        let ty = &field.ty;
        let input = &field.input;
        let rule = rules(field, rd);
        let custom = field.attr.validate.as_ref().map(|path| {
            quote! {
                if __rydantic_ctx.error_count() == __rydantic_start {
                    if let Err(error) = #path(#value) { __rydantic_ctx.push(error); }
                }
            }
        });
        let builtin = field.attr.has_rules().then(|| {
            quote! {
                let __rydantic_raw = <#ty as #rd::JsonType>::encode(#value, __rydantic_ctx);
                #rule.validate(&__rydantic_raw, __rydantic_ctx);
            }
        });
        quote! {
            __rydantic_ctx.at(#input, |__rydantic_ctx| {
                let __rydantic_start = __rydantic_ctx.error_count();
                <#ty as #rd::JsonType>::validate_into(#value, __rydantic_ctx);
                if __rydantic_ctx.error_count() == __rydantic_start {
                    #builtin
                    #custom
                }
            });
        }
    });
    quote!(#(#checks)*)
}

fn encode_fields(fields: &[Field], values: &[Tokens], rd: &Tokens) -> Tokens {
    let inserts = fields.iter().zip(values).filter(|(field, _)| !field.attr.exclude).map(|(field, value)| {
        let ty = &field.ty; let output = &field.output;
        quote! {
            __rydantic_object.insert(#output.to_owned(), __rydantic_ctx.at(#output, |ctx| <#ty as #rd::JsonType>::encode(#value, ctx)));
        }
    });
    quote!(#(#inserts)*)
}

fn encode_input_fields(fields: &[Field], values: &[Tokens], rd: &Tokens) -> Tokens {
    let inserts = fields.iter().zip(values).map(|(field, value)| {
        let ty = &field.ty;
        let input = &field.input;
        quote! {
            __rydantic_object.insert(#input.to_owned(), __rydantic_ctx.at(#input, |ctx| <#ty as #rd::JsonType>::encode_input(#value, ctx)));
        }
    });
    quote!(#(#inserts)*)
}

fn input_encoder(data: &Data, container: &attrs::Container, rd: &Tokens) -> syn::Result<Tokens> {
    match data {
        Data::Struct(data) => {
            let fields = fields(&data.fields, None)?;
            let values = fields
                .iter()
                .map(|field| {
                    let ident = &field.ident;
                    quote!(&self.#ident)
                })
                .collect::<Vec<_>>();
            let body = encode_input_fields(&fields, &values, rd);
            Ok(
                quote! { let mut __rydantic_object = #rd::__private::serde_json::Map::new(); #body #rd::Value::Object(__rydantic_object) },
            )
        }
        Data::Enum(data) => {
            let mut arms = vec![];
            for variant in &data.variants {
                let variant_ident = &variant.ident;
                let wire = attrs::variant_name(&variant.attrs, &variant_ident.to_string())?;
                if let Some(tag) = &container.tag {
                    let fields = fields(&variant.fields, Some(tag))?;
                    let names = fields.iter().map(|field| &field.ident);
                    let locals = (0..fields.len())
                        .map(|i| format_ident!("__rydantic_field_{i}"))
                        .collect::<Vec<_>>();
                    let pattern = if matches!(variant.fields, Fields::Unit) {
                        quote!(Self::#variant_ident)
                    } else {
                        quote!(Self::#variant_ident { #(#names: #locals,)* })
                    };
                    let values = locals
                        .iter()
                        .map(|local| quote!(#local))
                        .collect::<Vec<_>>();
                    let body = encode_input_fields(&fields, &values, rd);
                    arms.push(quote! { #pattern => {
                        let mut __rydantic_object = #rd::__private::serde_json::Map::new();
                        __rydantic_object.insert(#tag.to_owned(), #rd::__private::serde_json::json!(#wire));
                        #body
                        #rd::Value::Object(__rydantic_object)
                    }});
                } else {
                    arms.push(
                        quote! { Self::#variant_ident => #rd::__private::serde_json::json!(#wire) },
                    );
                }
            }
            Ok(quote!(match self { #(#arms,)* }))
        }
        Data::Union(_) => unreachable!("union rejected before encoding"),
    }
}

fn schema_fields(
    fields: &[Field],
    container: &attrs::Container,
    rd: &Tokens,
    tag: Option<(&str, &str)>,
) -> Tokens {
    let inserts = fields.iter().map(|field| {
        let ty = &field.ty; let input = &field.input; let output = &field.output;
        let exclude = field.attr.exclude; let required = field.attr.required; let default = field.attr.has_default();
        let coerce = field.attr.coerce; let rule = rules(field, rd);
        let before = field.attr.before.as_ref().map(|path| quote! {
            if __rydantic_ctx.mode == #rd::SchemaMode::Input {
                __rydantic_property = #rd::__private::serde_json::json!({});
                #rd::schema::annotate_local(&mut __rydantic_property, concat!("before:", stringify!(#path)));
            }
        });
        let apply_rules = if field.attr.before.is_some() { quote! {
            if __rydantic_ctx.mode == #rd::SchemaMode::Output {
                #rule.apply_schema(&mut __rydantic_property, __rydantic_ctx.mode, #coerce, <#ty as #rd::JsonType>::kind());
            }
        }} else { quote! { #rule.apply_schema(&mut __rydantic_property, __rydantic_ctx.mode, #coerce, <#ty as #rd::JsonType>::kind()); } };
        let validate = field.attr.validate.as_ref().map(|path| quote! {
            #rd::schema::annotate_local(&mut __rydantic_property, concat!("validate:", stringify!(#path)));
        });
        let description = field.doc.as_ref().map(|text| quote! { __rydantic_property["description"] = #rd::__private::serde_json::json!(#text); });
        let literal_default = field.attr.default.as_ref().and_then(|expr| match expr {
            Expr::Lit(expr) => match &expr.lit {
                syn::Lit::Str(value) => Some(serde_json::json!(value.value())),
                syn::Lit::Bool(value) => Some(serde_json::json!(value.value)),
                syn::Lit::Int(value) => value.base10_digits().parse::<serde_json::Number>().ok().map(serde_json::Value::Number),
                syn::Lit::Float(value) => value.base10_digits().parse::<serde_json::Number>().ok().map(serde_json::Value::Number),
                _ => None,
            }, _ => None,
        });
        let default_annotation = literal_default.map(|value| { let text = value.to_string(); quote! {
            if __rydantic_ctx.mode == #rd::SchemaMode::Input {
                __rydantic_property["default"] = #rd::__private::serde_json::from_str::<#rd::Value>(#text).expect("literal default");
            }
        }});
        quote! {
            if !(__rydantic_ctx.mode == #rd::SchemaMode::Output && #exclude) {
                let __rydantic_key = if __rydantic_ctx.mode == #rd::SchemaMode::Input { #input } else { #output };
                let mut __rydantic_property = <#ty as #rd::JsonType>::schema(__rydantic_ctx, #coerce);
                #before
                #apply_rules
                #validate
                #description
                #default_annotation
                __rydantic_properties.insert(__rydantic_key.to_owned(), __rydantic_property);
                if __rydantic_ctx.mode == #rd::SchemaMode::Output || #required || (!#default && !<#ty as #rd::JsonType>::allows_missing()) {
                    __rydantic_required.push(#rd::__private::serde_json::json!(__rydantic_key));
                }
            }
        }
    });
    let tag = tag.map(|(key, value)| quote! {
        __rydantic_properties.insert(#key.to_owned(), #rd::__private::serde_json::json!({"const":#value, "type":"string"}));
        __rydantic_required.push(#rd::__private::serde_json::json!(#key));
    });
    let ignore = container.ignore_extra;
    quote! {{
        let mut __rydantic_properties = #rd::__private::serde_json::Map::new();
        let mut __rydantic_required = ::std::vec::Vec::new();
        #tag
        #(#inserts)*
        #rd::__private::serde_json::json!({
            "type":"object", "properties":__rydantic_properties, "required":__rydantic_required,
            "additionalProperties": (#ignore && __rydantic_ctx.mode == #rd::SchemaMode::Input),
        })
    }}
}

fn expand(input: DeriveInput) -> syn::Result<Tokens> {
    let rd = match proc_macro_crate::crate_name("rydantic")
        .map_err(|error| syn::Error::new(input.ident.span(), error))?
    {
        proc_macro_crate::FoundCrate::Itself => quote!(::rydantic),
        proc_macro_crate::FoundCrate::Name(name) => {
            let name = format_ident!("{name}");
            quote!(::#name)
        }
    };
    if input.generics.lifetimes().next().is_some() {
        return Err(syn::Error::new(
            input.generics.span(),
            "Model currently supports owned types; lifetime parameters are unsupported",
        ));
    }
    let container = attrs::container(&input.attrs)?;
    let name = &input.ident;
    let title = container.title.clone().unwrap_or_else(|| name.to_string());
    let mut all_fields = vec![];
    let (decode, encode, validate, schema, kind) = match &input.data {
        Data::Struct(data) => {
            if container.tag.is_some() {
                return Err(syn::Error::new(name.span(), "tag is supported on enums"));
            }
            if !matches!(data.fields, Fields::Named(_)) {
                return Err(syn::Error::new(
                    name.span(),
                    "Model structs require named fields",
                ));
            }
            let fields = fields(&data.fields, None)?;
            let names = fields.iter().map(|f| &f.ident);
            let locals: Vec<_> = (0..fields.len())
                .map(|i| format_ident!("__rydantic_field_{i}"))
                .collect();
            let body = parse_body(
                &fields,
                quote!(Self { #(#names: #locals,)* }),
                &container,
                &rd,
            );
            let decode = quote! {
                let #rd::Value::Object(mut __rydantic_object) = __rydantic_raw else {
                    __rydantic_ctx.push(#rd::ValidationError::new("type_error", "expected object").with_stage(#rd::Stage::Parse));
                    return None;
                };
                #body
            };
            let values: Vec<_> = fields
                .iter()
                .map(|f| {
                    let name = &f.ident;
                    quote!(&self.#name)
                })
                .collect();
            let encode = encode_fields(&fields, &values, &rd);
            let encode = quote! { let mut __rydantic_object = #rd::__private::serde_json::Map::new(); #encode #rd::Value::Object(__rydantic_object) };
            let validate = validate_fields(&fields, &values, &rd);
            let schema = schema_fields(&fields, &container, &rd, None);
            all_fields.extend(fields);
            (
                decode,
                encode,
                validate,
                schema,
                quote!(#rd::TypeKind::Object),
            )
        }
        Data::Enum(data) => {
            let mut seen = std::collections::HashSet::new();
            let mut variants = vec![];
            for variant in &data.variants {
                let wire = attrs::variant_name(&variant.attrs, &variant.ident.to_string())?;
                if !seen.insert(wire.clone()) {
                    return Err(syn::Error::new(
                        variant.ident.span(),
                        "duplicate enum variant name",
                    ));
                }
                if container.tag.is_none() && !matches!(variant.fields, Fields::Unit) {
                    return Err(syn::Error::new(
                        variant.ident.span(),
                        "enums with fields require #[model(tag = \"...\")]",
                    ));
                }
                variants.push((
                    &variant.ident,
                    wire,
                    fields(&variant.fields, container.tag.as_deref())?,
                    matches!(variant.fields, Fields::Unit),
                ));
            }
            if variants.is_empty() {
                return Err(syn::Error::new(
                    name.span(),
                    "Model requires at least one enum variant",
                ));
            }
            let wires: Vec<_> = variants
                .iter()
                .map(|(_, wire, _, _)| wire.as_str())
                .collect();
            if let Some(tag) = &container.tag {
                let decodes = variants.iter().map(|(variant, wire, fields, unit)| {
                    let names = fields.iter().map(|field| &field.ident);
                    let locals: Vec<_> = (0..fields.len())
                        .map(|i| format_ident!("__rydantic_field_{i}"))
                        .collect();
                    let ctor = if *unit {
                        quote!(Self::#variant)
                    } else {
                        quote!(Self::#variant { #(#names: #locals,)* })
                    };
                    let body = parse_body(fields, ctor, &container, &rd);
                    quote!(#wire => { #body })
                });
                let decode = quote! {
                    let #rd::Value::Object(mut __rydantic_object) = __rydantic_raw else {
                        __rydantic_ctx.push(#rd::ValidationError::new("type_error", "expected tagged object").with_stage(#rd::Stage::Parse)); return None;
                    };
                    let __rydantic_tag = match __rydantic_object.remove(#tag) {
                        Some(#rd::Value::String(tag)) => tag,
                        Some(_) => { __rydantic_ctx.at(#tag, |ctx| ctx.push(#rd::ValidationError::new("union_tag_type", "discriminator must be a string").with_stage(#rd::Stage::Parse))); return None; },
                        None => { __rydantic_ctx.at(#tag, |ctx| ctx.push(#rd::ValidationError::new("union_tag_missing", "discriminator is required").with_stage(#rd::Stage::Parse))); return None; },
                    };
                    match __rydantic_tag.as_str() { #(#decodes,)* _ => {
                        __rydantic_ctx.at(#tag, |ctx| ctx.push(#rd::ValidationError::new("union_tag_invalid", "unknown discriminator").with_stage(#rd::Stage::Parse).with_param("expected", #rd::__private::serde_json::json!([#(#wires),*])))); None
                    }}
                };
                let encodes = variants.iter().map(|(variant, wire, fields, unit)| {
                    let names = fields.iter().map(|f| &f.ident);
                    let locals: Vec<_> = (0..fields.len()).map(|i| format_ident!("__rydantic_field_{i}")).collect();
                    let pattern = if *unit { quote!(Self::#variant) } else { quote!(Self::#variant { #(#names: #locals,)* }) };
                    let values: Vec<_> = locals.iter().map(|local| quote!(#local)).collect();
                    let body = encode_fields(fields, &values, &rd);
                    quote!(#pattern => {
                        let mut __rydantic_object = #rd::__private::serde_json::Map::new();
                        __rydantic_object.insert(#tag.to_owned(), #rd::__private::serde_json::json!(#wire));
                        #body
                        #rd::Value::Object(__rydantic_object)
                    })
                });
                let checks = variants.iter().map(|(variant, _, fields, unit)| {
                    let names = fields.iter().map(|f| &f.ident);
                    let locals: Vec<_> = (0..fields.len())
                        .map(|i| format_ident!("__rydantic_field_{i}"))
                        .collect();
                    let pattern = if *unit {
                        quote!(Self::#variant)
                    } else {
                        quote!(Self::#variant { #(#names: #locals,)* })
                    };
                    let values: Vec<_> = locals.iter().map(|local| quote!(#local)).collect();
                    let body = validate_fields(fields, &values, &rd);
                    quote!(#pattern => { #body })
                });
                let schemas = variants.iter().map(|(_, wire, fields, _)| {
                    schema_fields(fields, &container, &rd, Some((tag, wire)))
                });
                let schema = quote!(#rd::__private::serde_json::json!({"oneOf":[#((#schemas)),*], "discriminator":{"propertyName":#tag}}));
                let result = (
                    decode,
                    quote!(match self { #(#encodes,)* }),
                    quote!(match self { #(#checks,)* }),
                    schema,
                    quote!(#rd::TypeKind::Object),
                );
                for (_, _, fields, _) in variants {
                    all_fields.extend(fields);
                }
                result
            } else {
                let decodes = variants
                    .iter()
                    .map(|(variant, wire, _, _)| quote!(#wire => Some(Self::#variant)));
                let encodes = variants.iter().map(|(variant, wire, _, _)| quote!(Self::#variant => #rd::__private::serde_json::json!(#wire)));
                let decode = quote! {
                    match __rydantic_raw.as_str() {
                        Some(wire) => match wire { #(#decodes,)* _ => { __rydantic_ctx.push(#rd::ValidationError::new("enum", "unknown enum value").with_stage(#rd::Stage::Parse).with_param("expected", #rd::__private::serde_json::json!([#(#wires),*]))); None } },
                        None => { __rydantic_ctx.push(#rd::ValidationError::new("type_error", "expected enum string").with_stage(#rd::Stage::Parse)); None },
                    }
                };
                (
                    decode,
                    quote!(match self { #(#encodes,)* }),
                    quote!(),
                    quote!(#rd::__private::serde_json::json!({"type":"string","enum":[#(#wires),*]})),
                    quote!(#rd::TypeKind::String),
                )
            }
        }
        Data::Union(_) => {
            return Err(syn::Error::new(
                name.span(),
                "Model cannot be derived for unions",
            ))
        }
    };

    let encode_input = input_encoder(&input.data, &container, &rd)?;
    let mut generics = input.generics.clone();
    for parameter in generics.type_params_mut() {
        parameter.bounds.push(parse_quote!(#rd::JsonType));
    }
    for field in &all_fields {
        let ty = &field.ty;
        let a = &field.attr;
        // Only generic field predicates are needed; concrete predicates would
        // introduce recursive trait obligations on recursive model definitions.
        let generic_field = input.generics.type_params().any(|parameter| {
            fn has(ty: &Type, name: &Ident) -> bool {
                match ty {
                    Type::Path(path) => path.path.segments.iter().any(|segment| segment.ident == *name || match &segment.arguments {
                        syn::PathArguments::AngleBracketed(args) => args.args.iter().any(|arg| matches!(arg, syn::GenericArgument::Type(ty) if has(ty, name))), _ => false,
                    }),
                    Type::Array(array) => has(&array.elem, name), _ => false,
                }
            }
            has(ty, &parameter.ident)
        });
        if generic_field {
            if a.trim || a.lowercase || a.uppercase || a.email || a.pattern.is_some() {
                generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#ty: #rd::StringType));
            }
            if a.min_length.is_some() || a.max_length.is_some() {
                generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#ty: #rd::LengthType));
            }
            if a.ge.is_some() || a.gt.is_some() || a.le.is_some() || a.lt.is_some() {
                generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#ty: #rd::NumericType));
            }
            if a.default_trait {
                generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#ty: ::core::default::Default));
            }
        }
    }
    let assertions = all_fields.iter().flat_map(|field| {
        let ty = &field.ty;
        let a = &field.attr;
        let mut result = vec![];
        if a.trim || a.lowercase || a.uppercase || a.email || a.pattern.is_some() {
            result.push(quote! { __rydantic_assert_string::<#ty>(); });
        }
        if a.min_length.is_some() || a.max_length.is_some() {
            result.push(quote! { __rydantic_assert_length::<#ty>(); });
        }
        if a.ge.is_some() || a.gt.is_some() || a.le.is_some() || a.lt.is_some() {
            result.push(quote! { __rydantic_assert_number::<#ty>(); });
        }
        result
    });
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let mut de_generics = generics.clone();
    de_generics.params.insert(0, parse_quote!('__rydantic_de));
    let (de_impl_generics, _, de_where_clause) = de_generics.split_for_impl();
    let custom_validate = container.validate.as_ref().map(|path| {
        quote! {
            if __rydantic_ctx.error_count() == __rydantic_model_start {
                if let Err(errors) = #path(self) { __rydantic_ctx.extend(errors); }
            }
        }
    });
    let unit_parse_validate = if matches!(&input.data, Data::Enum(data) if data.variants.iter().all(|v| matches!(v.fields, Fields::Unit)))
        && container.tag.is_none()
    {
        container.validate.as_ref().map(|path| quote! {
            let __rydantic_value = (|| { #decode })();
            if let Some(value) = &__rydantic_value { if let Err(errors) = #path(value) { __rydantic_ctx.extend(errors); } }
            __rydantic_value
        })
    } else {
        None
    };
    let decode = unit_parse_validate.unwrap_or(decode);
    let schema_custom = container.validate.as_ref().map(|path| quote! { #rd::schema::annotate_local(&mut __rydantic_schema, concat!("validate:", stringify!(#path))); });

    Ok(quote! {
        impl #impl_generics #rd::JsonType for #name #ty_generics #where_clause {
            fn decode(__rydantic_raw: #rd::Value, __rydantic_ctx: &mut #rd::Context, _coerce: bool) -> Option<Self> {
                fn __rydantic_assert_string<T: #rd::StringType>() {}
                fn __rydantic_assert_length<T: #rd::LengthType>() {}
                fn __rydantic_assert_number<T: #rd::NumericType>() {}
                #(#assertions)*
                #decode
            }
            fn encode(&self, __rydantic_ctx: &mut #rd::Context) -> #rd::Value { #encode }
            fn encode_input(&self, __rydantic_ctx: &mut #rd::Context) -> #rd::Value { #encode_input }
            fn validate_into(&self, __rydantic_ctx: &mut #rd::Context) {
                let __rydantic_model_start = __rydantic_ctx.error_count();
                #validate
                #custom_validate
            }
            fn schema(__rydantic_ctx: &mut #rd::SchemaContext, _coerce: bool) -> #rd::Value {
                __rydantic_ctx.reference(::core::any::type_name::<Self>(), |__rydantic_ctx| {
                    let mut __rydantic_schema = #schema;
                    __rydantic_schema["title"] = #rd::__private::serde_json::json!(#title);
                    #schema_custom
                    __rydantic_schema
                })
            }
            fn kind() -> #rd::TypeKind { #kind }
        }
        impl #impl_generics #rd::Model for #name #ty_generics #where_clause {}
        impl #impl_generics #rd::__private::serde::Serialize for #name #ty_generics #where_clause {
            fn serialize<S: #rd::__private::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let value = <Self as #rd::Model>::dump(self).map_err(#rd::__private::serde::ser::Error::custom)?;
                #rd::__private::serde::Serialize::serialize(&value, serializer)
            }
        }
        impl #de_impl_generics #rd::__private::serde::Deserialize<'__rydantic_de> for #name #ty_generics #de_where_clause {
            fn deserialize<D: #rd::__private::serde::Deserializer<'__rydantic_de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = <#rd::Value as #rd::__private::serde::Deserialize>::deserialize(deserializer)?;
                <Self as #rd::Model>::parse_value(value).map_err(#rd::__private::serde::de::Error::custom)
            }
        }
    })
}
