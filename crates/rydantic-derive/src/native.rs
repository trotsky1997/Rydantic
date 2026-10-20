//! Generate calls to the official schema's typed accessors.
use super::{attrs, constructor, default_expr, fields, pattern, shape, Shape};
use heck::{ToLowerCamelCase, ToSnakeCase, ToUpperCamelCase};
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{spanned::Spanned, Data, DeriveInput, GenericArgument, PathArguments, Type};

enum Access {
    Field(Tokens, String),
    Item(Tokens, Tokens),
    Dynamic(Tokens),
}
impl Access {
    fn get(&self) -> Tokens {
        match self {
            Self::Field(parent, name) => {
                let method = format_ident!("get_{}", name.to_snake_case());
                quote!(#parent.#method())
            }
            Self::Item(parent, index) => quote!(#parent.get(#index)),
            Self::Dynamic(value) => value.clone(),
        }
    }
    fn set(&self, value: Tokens) -> Tokens {
        match self {
            Self::Field(parent, name) => {
                let method = format_ident!("set_{}", name.to_snake_case());
                quote!(#parent.#method(#value))
            }
            Self::Item(parent, index) => quote!(#parent.set(#index,#value)),
            Self::Dynamic(_) => unreachable!(),
        }
    }
    fn init(&self, length: ::core::option::Option<Tokens>) -> Tokens {
        match (self, length) {
            (Self::Field(parent, name), length) => {
                let method = format_ident!("init_{}", name.to_snake_case());
                quote!(#parent.reborrow().#method(#length))
            }
            (Self::Item(parent, index), None) => quote!(#parent.reborrow().get(#index)),
            (Self::Item(parent, index), Some(length)) => {
                quote!(#parent.reborrow().init(#index,#length))
            }
            (Self::Dynamic(_), _) => unreachable!(),
        }
    }
}
fn type_name(ty: &Type) -> String {
    if let Type::Path(path) = ty {
        if path.path.segments.first().is_some_and(|segment| {
            matches!(
                segment.ident.to_string().as_str(),
                "self" | "super" | "crate"
            )
        }) {
            return String::new();
        }
        path.path.segments.last().unwrap().ident.to_string()
    } else {
        String::new()
    }
}
fn mapped_type(field: &super::Field) -> &Type {
    field.attr.capnp_as.as_ref().unwrap_or(&field.ty)
}
fn byte_vector(ty: &Type) -> bool {
    type_name(ty) == "Vec" && args(ty).first().is_some_and(|ty| type_name(ty) == "u8")
}
fn scalar(ty: &Type) -> bool {
    !matches!(
        type_name(ty).as_str(),
        "Vec" | "Option" | "Box" | "BTreeMap" | "HashMap"
    ) && !matches!(ty, Type::Array(_))
}
fn args(ty: &Type) -> Vec<&Type> {
    let Type::Path(path) = ty else { return vec![] };
    let PathArguments::AngleBracketed(args) = &path.path.segments.last().unwrap().arguments else {
        return vec![];
    };
    args.args
        .iter()
        .filter_map(|arg| {
            if let GenericArgument::Type(ty) = arg {
                Some(ty)
            } else {
                None
            }
        })
        .collect()
}
fn primitive(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
    )
}
fn read(ty: &Type, access: Access, attr: &attrs::Field, rd: &Tokens) -> syn::Result<Tokens> {
    let name = type_name(ty);
    let arguments = args(ty);
    if let Access::Dynamic(value) = access {
        return Ok(if name == "Box" {
            let inner = read(arguments[0], Access::Dynamic(value), attr, rd)?;
            quote!(::std::boxed::Box::new({#inner}))
        } else if attr.capnp_data && byte_vector(ty) {
            quote!(#rd::capnp_support::data_value(#value,__rydantic_ctx)?.to_vec())
        } else if attr.capnp_text && scalar(ty) {
            quote!({let text=#rd::capnp_support::text_value(#value,__rydantic_ctx)?;<#ty as #rd::JsonType>::decode(#rd::Value::String(text.to_owned()),__rydantic_ctx,true)?})
        } else {
            quote!(<#ty as #rd::capnp_support::CapnpRead>::read_value(#value,__rydantic_ctx)?)
        });
    }
    let get = access.get();
    let checked = quote!(#rd::capnp_support::read_result(#get,__rydantic_ctx)?);
    Ok(if name == "Box" {
        let value = read(arguments[0], access, attr, rd)?;
        quote!(::std::boxed::Box::new({#value}))
    } else if name == "Option" {
        let value = read(
            arguments[0],
            Access::Dynamic(
                quote!(#rd::capnp_support::field_value(__wire_optional.into(),"some",__rydantic_ctx)?),
            ),
            attr,
            rd,
        )?;
        let optional = if matches!(access, Access::Item(..)) {
            get
        } else {
            checked
        };
        quote!({let __wire_optional=#optional;if #rd::capnp_support::optional_some(__wire_optional.into(),__rydantic_ctx)?{Some({#value})}else{None}})
    } else if attr.capnp_data && byte_vector(ty) {
        quote!((#checked).to_vec())
    } else if attr.capnp_text && scalar(ty) {
        quote!({let text=#checked;let text=#rd::capnp_support::read_result(text.to_str(),__rydantic_ctx)?;<#ty as #rd::JsonType>::decode(#rd::Value::String(text.to_owned()),__rydantic_ctx,true)?})
    } else if name == "Vec" || matches!(ty, Type::Array(_)) {
        let element = if let Type::Array(array) = ty {
            &*array.elem
        } else {
            arguments[0]
        };
        let value = read(
            element,
            Access::Item(quote!(__wire_list), quote!(__wire_index)),
            attr,
            rd,
        )?;
        let finish = if let Type::Array(array) = ty {
            let length = &array.len;
            quote!(__wire_values.try_into().map_err(|_|__rydantic_ctx.push(#rd::ValidationError::new("capnp_length",concat!("expected ",stringify!(#length)," array elements")))).ok()?)
        } else {
            quote!(__wire_values)
        };
        quote!({let __wire_list=#checked;let mut __wire_values=::std::vec::Vec::with_capacity(__wire_list.len() as usize);for __wire_index in 0..__wire_list.len(){let value=__rydantic_ctx.at(__wire_index as usize,|__rydantic_ctx|Some({#value}))?;__wire_values.push(value);}#finish})
    } else if matches!(name.as_str(), "BTreeMap" | "HashMap") {
        if arguments.len() < 2 || type_name(arguments[0]) != "String" {
            return Err(syn::Error::new(
                ty.span(),
                "native Cap'n Proto maps require String keys",
            ));
        }
        let value = read(
            arguments[1],
            Access::Field(quote!(__wire_entry), "value".into()),
            attr,
            rd,
        )?;
        quote!({let __wire_list=#checked;let mut __wire_values=<#ty>::new();for __wire_index in 0..__wire_list.len(){let __wire_entry=__wire_list.get(__wire_index);let key=#rd::capnp_support::read_result(__wire_entry.get_key(),__rydantic_ctx)?;let key=#rd::capnp_support::read_result(key.to_str(),__rydantic_ctx)?.to_owned();let value=__rydantic_ctx.at(key.clone(),|__rydantic_ctx|Some({#value}))?;if __wire_values.insert(key.clone(),value).is_some(){__rydantic_ctx.at(key,|ctx|ctx.push(#rd::ValidationError::new("capnp_duplicate_key","duplicate map key")));return None;}}__wire_values})
    } else if attr.capnp_enum {
        quote!(<#ty as #rd::CapnpEnum>::read_enum(#checked,__rydantic_ctx)?)
    } else if primitive(&name) {
        if matches!(name.as_str(), "bool" | "f32" | "f64") {
            get
        } else {
            quote!(#rd::capnp_support::converted::<_,#ty>(#get,__rydantic_ctx)?)
        }
    } else if name == "String" {
        quote!({let text=#checked;#rd::capnp_support::read_result(text.to_str(),__rydantic_ctx)?.to_owned()})
    } else {
        // Struct-list elements are inline, while struct fields are pointers.
        let reader = if matches!(access, Access::Item(..)) {
            get
        } else {
            checked
        };
        quote!(<#ty as #rd::CapnpModel>::read_capnp(#reader,__rydantic_ctx)?)
    })
}
fn write(
    ty: &Type,
    access: Access,
    value: Tokens,
    attr: &attrs::Field,
    rd: &Tokens,
) -> syn::Result<Tokens> {
    let name = type_name(ty);
    let arguments = args(ty);
    Ok(if name == "Box" {
        write(arguments[0], access, quote!((#value).as_ref()), attr, rd)?
    } else if name == "Option" {
        let init = access.init(None);
        let set_none = Access::Field(quote!(__wire_optional), "none".into()).set(quote!(()));
        let some = write(
            arguments[0],
            Access::Field(quote!(__wire_optional), "some".into()),
            quote!(__wire_value),
            attr,
            rd,
        )?;
        quote!({let mut __wire_optional=#init;if let Some(__wire_value)=#value{#some}else{#set_none;}})
    } else if attr.capnp_data && byte_vector(ty) {
        let set = access.set(quote!((#value).as_slice()));
        quote!(#rd::capnp_support::write_result(#set,__rydantic_ctx)?;)
    } else if attr.capnp_text && scalar(ty) {
        let set = access.set(quote!(__wire_text.as_str()));
        quote!({let __wire_text=match <#ty as #rd::JsonType>::encode(#value,__rydantic_ctx){#rd::Value::String(text)=>text,#rd::Value::Number(number)=>number.to_string(),_=>{__rydantic_ctx.push(#rd::ValidationError::new("capnp_text","capnp_text requires a scalar text or number representation"));return None;}};#rd::capnp_support::write_result(#set,__rydantic_ctx)?;})
    } else if name == "Vec" || matches!(ty, Type::Array(_)) {
        let element = if let Type::Array(array) = ty {
            &*array.elem
        } else {
            arguments[0]
        };
        let init = access.init(Some(
            quote!(#rd::capnp_support::list_len((#value).len(),__rydantic_ctx)?),
        ));
        let body = write(
            element,
            Access::Item(quote!(__wire_list), quote!(__wire_index as u32)),
            quote!(__wire_value),
            attr,
            rd,
        )?;
        quote!({let mut __wire_list=#init;for (__wire_index,__wire_value) in (#value).iter().enumerate(){__rydantic_ctx.at(__wire_index,|__rydantic_ctx|{#body Some(())})?;}})
    } else if matches!(name.as_str(), "BTreeMap" | "HashMap") {
        if arguments.len() < 2 || type_name(arguments[0]) != "String" {
            return Err(syn::Error::new(
                ty.span(),
                "native Cap'n Proto maps require String keys",
            ));
        }
        let init = access.init(Some(
            quote!(#rd::capnp_support::list_len((#value).len(),__rydantic_ctx)?),
        ));
        let body = write(
            arguments[1],
            Access::Field(quote!(__wire_entry), "value".into()),
            quote!(__wire_value),
            attr,
            rd,
        )?;
        quote!({let mut __wire_list=#init;for (__wire_index,(__wire_key,__wire_value)) in (#value).iter().enumerate(){let mut __wire_entry=__wire_list.reborrow().get(__wire_index as u32);__wire_entry.set_key(__wire_key.as_str());__rydantic_ctx.at(__wire_key.clone(),|__rydantic_ctx|{#body Some(())})?;}})
    } else if attr.capnp_enum {
        let set = access.set(quote!(<#ty as #rd::CapnpEnum>::write_enum(#value)));
        quote!(#rd::capnp_support::write_result(#set,__rydantic_ctx)?;)
    } else if primitive(&name) {
        let scalar = if matches!(name.as_str(), "bool" | "f32" | "f64") {
            quote!(*#value)
        } else {
            quote!(#rd::capnp_support::converted(*#value,__rydantic_ctx)?)
        };
        let set = access.set(scalar);
        quote!(#rd::capnp_support::write_result(#set,__rydantic_ctx)?;)
    } else if name == "String" {
        let set = access.set(quote!((#value).as_str()));
        quote!(#rd::capnp_support::write_result(#set,__rydantic_ctx)?;)
    } else {
        let init = access.init(None);
        quote!(<#ty as #rd::CapnpModel>::write_capnp(#value,#init,__rydantic_ctx);)
    })
}
fn wire_name(field: &super::Field, index: usize, shape: Shape, count: usize) -> String {
    field
        .attr
        .capnp_name
        .clone()
        .unwrap_or_else(|| match shape {
            Shape::Named => field.ident.to_string().trim_start_matches("r#").to_owned(),
            Shape::Tuple if count == 1 => "value".into(),
            _ => format!("item{index}"),
        })
}
fn read_fields(
    fields: &[super::Field],
    shape: Shape,
    path: Tokens,
    rd: &Tokens,
    dynamic: bool,
) -> syn::Result<Tokens> {
    let mut parses = vec![];
    let mut native_names = std::collections::HashSet::new();
    let locals = (0..fields.len())
        .map(super::field_ident)
        .collect::<Vec<_>>();
    for (i, field) in fields.iter().enumerate() {
        let local = &locals[i];
        let name = wire_name(field, i, shape, fields.len());
        if !field.attr.capnp_skip && !native_names.insert(name.to_snake_case()) {
            return Err(syn::Error::new(
                field.ident.span(),
                "duplicate native Cap'n Proto field mapping",
            ));
        }
        let location = &field.input;
        let ty = mapped_type(field);
        let value = if field.attr.capnp_skip {
            if let Some(value) = default_expr(field, false) {
                quote!(#value)
            } else {
                quote!(<#ty as #rd::JsonType>::missing().or_else(||{__rydantic_ctx.push(#rd::ValidationError::new("missing","skipped native field requires a default"));None})?)
            }
        } else {
            let access = if dynamic {
                Access::Dynamic(
                    quote!(#rd::capnp_support::field_value(__wire_reader,#name,__rydantic_ctx)?),
                )
            } else {
                Access::Field(quote!(__wire_reader), name)
            };
            read(ty, access, &field.attr, rd)?
        };
        parses
            .push(quote!(let #local=__rydantic_ctx.at(#location,|__rydantic_ctx|Some({#value}));));
    }
    let constructed = constructor(fields, shape, path);
    Ok(quote!(#(#parses)* #(let #locals=#locals?;)* Some(#constructed)))
}

fn write_fields(
    fields: &[super::Field],
    shape: Shape,
    values: &[Tokens],
    rd: &Tokens,
) -> syn::Result<Tokens> {
    let mut writes = vec![];
    for (i, (field, value)) in fields.iter().zip(values).enumerate() {
        if field.attr.capnp_skip {
            continue;
        }
        let name = wire_name(field, i, shape, fields.len());
        let location = &field.input;
        let body = write(
            mapped_type(field),
            Access::Field(quote!(__wire_builder), name),
            value.clone(),
            &field.attr,
            rd,
        )?;
        writes.push(quote! {let _=__rydantic_ctx.at(#location,|__rydantic_ctx|{#body Some(())});});
    }
    Ok(quote!(#(#writes)*))
}
pub(super) fn expand(
    input: &DeriveInput,
    container: &attrs::Container,
    rd: &Tokens,
) -> syn::Result<Tokens> {
    if container.capnp.is_none() && container.capnp_enum.is_none() {
        return Ok(quote!());
    }
    if container.capnp.is_some() && container.capnp_enum.is_some() {
        return Err(syn::Error::new(
            input.span(),
            "choose capnp or capnp_enum for a model",
        ));
    }
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "native Cap'n Proto models require a concrete owned type",
        ));
    }
    let name = &input.ident;
    if let Some(wire) = &container.capnp_enum {
        let Data::Enum(data) = &input.data else {
            return Err(syn::Error::new(
                input.span(),
                "capnp_enum requires an enum with unit variants",
            ));
        };
        let mut reads = vec![];
        let mut writes = vec![];
        for variant in &data.variants {
            if !matches!(variant.fields, syn::Fields::Unit) {
                return Err(syn::Error::new(
                    variant.span(),
                    "capnp_enum requires unit variants",
                ));
            }
            let id = &variant.ident;
            let native = format_ident!(
                "{}",
                attrs::native_variant_name(&variant.attrs, &id.to_string())?.to_upper_camel_case()
            );
            reads.push(quote!(#wire::#native=>Some(Self::#id)));
            writes.push(quote!(Self::#id=>#wire::#native));
        }
        return Ok(
            quote! {impl #rd::CapnpEnum for #name{type Wire=#wire;#[allow(unreachable_patterns)]fn read_enum(value:Self::Wire,ctx:&mut #rd::Context)->::core::option::Option<Self>{match value{#(#reads,)* _=>{ctx.push(#rd::ValidationError::new("capnp_enum","native enum variant has no model mapping"));None}}}fn write_enum(&self)->Self::Wire{match self{#(#writes,)*}}}
            impl #rd::capnp_support::CapnpRead for #name {fn read_value(value:#rd::capnp::dynamic_value::Reader<'_>,ctx:&mut #rd::Context)->::core::option::Option<Self>{let #rd::capnp::dynamic_value::Reader::Enum(value)=value else{#rd::capnp_support::mismatch(ctx);return None;};let value=#rd::capnp_support::read_result(<#wire as ::core::convert::TryFrom<u16>>::try_from(value.get_value()),ctx)?;<Self as #rd::CapnpEnum>::read_enum(value,ctx)}}},
        );
    }
    let wire = container.capnp.as_ref().unwrap();
    let (read_body, write_body) = match &input.data {
        Data::Struct(data) => {
            let fs = fields(&data.fields, None, container)?;
            let form = shape(&data.fields);
            let values = fs
                .iter()
                .map(|f| {
                    let member = &f.member;
                    quote!(&self.#member)
                })
                .collect::<Vec<_>>();
            (
                read_fields(&fs, form, quote!(Self), rd, false)?,
                write_fields(&fs, form, &values, rd)?,
            )
        }
        Data::Enum(data) => {
            let mut reads = vec![];
            let mut writes = vec![];
            for variant in &data.variants {
                let id = &variant.ident;
                let native = attrs::native_variant_name(&variant.attrs, &id.to_string())?
                    .to_lower_camel_case();
                let fs = fields(&variant.fields, None, container)?;
                let form = shape(&variant.fields);
                let body = if form == Shape::Unit || fs.is_empty() {
                    let value = constructor(&fs, form, quote!(Self::#id));
                    quote!(Some(#value))
                } else if form == Shape::Tuple && fs.len() == 1 {
                    let f = &fs[0];
                    let value = read(
                        mapped_type(f),
                        Access::Dynamic(
                            quote!(#rd::capnp_support::field_value(__wire_reader.into(),#native,__rydantic_ctx)?),
                        ),
                        &f.attr,
                        rd,
                    )?;
                    quote!(Some(Self::#id({#value})))
                } else {
                    let body = read_fields(&fs, form, quote!(Self::#id), rd, true)?;
                    quote!({let __wire_reader=#rd::capnp_support::field_value(__wire_reader.into(),#native,__rydantic_ctx)?;#body})
                };
                reads.push(quote!(#native=>{#body}));
                let (pattern, values) = pattern(&fs, form, quote!(Self::#id));
                let body = if form == Shape::Unit || fs.iter().all(|f| f.attr.capnp_skip) {
                    let set = Access::Field(quote!(__wire_builder), native.clone()).set(quote!(()));
                    quote!(#set;)
                } else if form == Shape::Tuple && fs.len() == 1 {
                    let body = write(
                        mapped_type(&fs[0]),
                        Access::Field(quote!(__wire_builder), native.clone()),
                        values[0].clone(),
                        &fs[0].attr,
                        rd,
                    )?;
                    quote!(let _=(||{#body Some(())})();)
                } else {
                    let init = Access::Field(quote!(__wire_builder), native.clone()).init(None);
                    let body = write_fields(&fs, form, &values, rd)?;
                    quote!(let mut __wire_builder=#init;#body)
                };
                writes.push(quote!(#pattern=>{#body}));
            }
            (
                quote! {match #rd::capnp_support::which_name(__wire_reader.into(),__rydantic_ctx)?.as_str(){#(#reads,)* _=>{__rydantic_ctx.push(#rd::ValidationError::new("capnp_union","native union variant has no model mapping"));None}}},
                quote!(match self{#(#writes,)*}),
            )
        }
        Data::Union(_) => return Err(syn::Error::new(input.span(), "Rust unions are unsupported")),
    };
    Ok(
        quote! {impl #rd::CapnpModel for #name{type Wire=#wire::Owned;fn read_capnp(__wire_reader:<Self::Wire as #rd::capnp::traits::Owned>::Reader<'_>,__rydantic_ctx:&mut #rd::Context)->::core::option::Option<Self>{#read_body}fn write_capnp(&self,mut __wire_builder:<Self::Wire as #rd::capnp::traits::Owned>::Builder<'_>,__rydantic_ctx:&mut #rd::Context){#write_body}}
        impl #rd::capnp_support::CapnpRead for #name{fn read_value(value:#rd::capnp::dynamic_value::Reader<'_>,ctx:&mut #rd::Context)->::core::option::Option<Self>{let reader=#rd::capnp_support::struct_value::<#wire::Owned>(value,ctx)?;<Self as #rd::CapnpModel>::read_capnp(reader,ctx)}}},
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_native_names_are_rejected_before_codegen() {
        let input: DeriveInput = syn::parse_quote! {
            #[model(capnp="wire::record")]
            struct Duplicate {
                #[model(capnp_name="fullName")]
                first:String,
                full_name:String,
            }
        };
        let container = attrs::container(&input.attrs).unwrap();
        let error = expand(&input, &container, &quote!(::rydantic)).unwrap_err();
        assert!(error
            .to_string()
            .contains("duplicate native Cap'n Proto field mapping"));
    }
}
