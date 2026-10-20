use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::format_ident;
use syn::{
    parse_macro_input, parse_quote, spanned::Spanned, Data, DeriveInput, Expr, Fields, Ident,
    Member, Type,
};
mod attrs;
#[cfg(feature = "capnp")]
mod auto_protocol;
mod native;
mod sql_model;

// Generated locals must not shadow callback paths (or default expressions) from
// the input. Mixed-site hygiene applies only to tokens written by the macro;
// interpolated user tokens retain their original spans and name resolution.
// Keep this after the module declarations: it scopes the policy to Model's
// expansion here, rather than changing the other derives' quote invocations.
macro_rules! quote {
    ($($tokens:tt)*) => {
        ::quote::quote_spanned!(::proc_macro2::Span::mixed_site()=> $($tokens)*)
    };
}

fn field_ident(index: usize) -> Ident {
    format_ident!(
        "__rydantic_field_{index}",
        span = proc_macro2::Span::mixed_site()
    )
}

/// Derive SQL schema metadata, SQLx row decoding and validated CRUD operations.
#[proc_macro_derive(SqlModel, attributes(sql, model))]
pub fn derive_sql_model(input: TokenStream) -> TokenStream {
    sql_model::expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive a complete Cap'n Proto protocol and its bindings from an inline model module.
/// Every model must explicitly derive `Model` (an imported alias also works).
/// This attribute does not insert or resolve derive macros.
#[cfg(feature = "capnp")]
#[proc_macro_attribute]
pub fn capnp_models(args: TokenStream, input: TokenStream) -> TokenStream {
    auto_protocol::expand(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Model, attributes(model))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

struct Field {
    ident: Ident,
    member: Member,
    ty: Type,
    attr: attrs::Field,
    input: String,
    output: String,
    doc: ::core::option::Option<String>,
}
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Named,
    Tuple,
    Unit,
}
fn shape(fields: &Fields) -> Shape {
    match fields {
        Fields::Named(_) => Shape::Named,
        Fields::Unnamed(_) => Shape::Tuple,
        Fields::Unit => Shape::Unit,
    }
}
fn fields(
    source: &Fields,
    tag: ::core::option::Option<&str>,
    container: &attrs::Container,
) -> syn::Result<Vec<Field>> {
    let mut inputs = std::collections::HashSet::new();
    let mut outputs = std::collections::HashSet::new();
    let mut extras = 0;
    let result=source.iter().enumerate().map(|(i,field)|{
        let ident=field.ident.clone().unwrap_or_else(||format_ident!("__tuple_{i}"));
        let member=field.ident.clone().map(Member::Named).unwrap_or_else(||Member::Unnamed(i.into()));
        let attr=attrs::field(&field.attrs)?;
        if attr.extra{extras+=1;}
        if matches!(source,Fields::Unnamed(_)) && (attr.flatten||attr.extra||attr.rename.is_some()||attr.validation_alias.is_some()||attr.serialization_alias.is_some()||attr.exclude||attr.has_default()){
            return Err(syn::Error::new(field.span(),"tuple fields support validation and transformations; names, flatten, extra, exclude and defaults require named fields"));
        }
        if (attr.flatten||attr.extra)&&(attr.rename.is_some()||attr.validation_alias.is_some()||attr.serialization_alias.is_some()||attr.before.is_some()||attr.coerce||attr.has_rules()){
            return Err(syn::Error::new(field.span(),"flatten and extra fields use their inner model's names, transformations and constraints"));
        }
        let base=attr.rename.clone().unwrap_or_else(||if field.ident.is_some(){ident.to_string().trim_start_matches("r#").to_owned()}else{i.to_string()});
        let input=attr.validation_alias.clone().unwrap_or_else(||base.clone());let output=attr.serialization_alias.clone().unwrap_or(base);
        if !attr.flatten&&!attr.extra{
            if !inputs.insert(input.clone())||tag==Some(input.as_str()){return Err(syn::Error::new(ident.span(),"duplicate input field name or collision with enum tag"));}
            if !attr.exclude&&(!outputs.insert(output.clone())||tag==Some(output.as_str())){return Err(syn::Error::new(ident.span(),"duplicate output field name or collision with enum tag"));}
        }
        let doc=attr.description.clone().or_else(||{
            let text=field.attrs.iter().filter_map(|attr|{if !attr.path().is_ident("doc"){return None;}let syn::Meta::NameValue(v)=&attr.meta else{return None;};let Expr::Lit(v)=&v.value else{return None;};let syn::Lit::Str(v)=&v.lit else{return None;};Some(v.value().trim().to_owned())}).collect::<Vec<_>>();(!text.is_empty()).then(||text.join("\n"))
        });
        Ok(Field{ident,member,ty:field.ty.clone(),attr,input,output,doc})
    }).collect::<syn::Result<Vec<_>>>()?;
    if extras > 1 {
        return Err(syn::Error::new(
            source.span(),
            "a model may have only one extra field",
        ));
    }
    if container.allow_extra && extras != 1 {
        return Err(syn::Error::new(
            source.span(),
            "extra = \"allow\" requires one #[model(extra)] map field",
        ));
    }
    if !container.allow_extra && extras != 0 {
        return Err(syn::Error::new(
            source.span(),
            "#[model(extra)] requires #[model(extra = \"allow\")]",
        ));
    }
    Ok(result)
}
fn option<T: quote::ToTokens>(value: &::core::option::Option<T>) -> Tokens {
    match value {
        Some(v) => quote!(Some(#v)),
        None => quote!(None),
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
    let number = |v: &::core::option::Option<String>| match v {
        Some(v) => quote!(Some(#rd::__private::number(#v))),
        None => quote!(None),
    };
    let ge = number(&a.ge);
    let gt = number(&a.gt);
    let le = number(&a.le);
    let lt = number(&a.lt);
    let pattern = match &a.pattern {
        Some(v) => {
            quote!(Some(#rd::__private::regex::Regex::new(#v).expect("regex checked by Model derive")))
        }
        None => quote!(None),
    };
    quote!({static RULES: ::std::sync::OnceLock<#rd::rules::Rules>=::std::sync::OnceLock::new();RULES.get_or_init(||#rd::rules::Rules{trim:#trim,lowercase:#lower,uppercase:#upper,min_length:#min,max_length:#max,ge:#ge,gt:#gt,le:#le,lt:#lt,pattern:#pattern,email:#email})})
}
struct Mode<'a> {
    rd: &'a Tokens,
    borrow: bool,
}
impl Mode<'_> {
    fn tr(&self) -> Tokens {
        let rd = self.rd;
        if self.borrow {
            quote!(#rd::BorrowType<'__rydantic_de>)
        } else {
            quote!(#rd::JsonType)
        }
    }
    fn decode(&self, ty: &Type, raw: Tokens, coerce: bool) -> Tokens {
        let tr = self.tr();
        if self.borrow {
            quote!(<#ty as #tr>::decode(#raw,__rydantic_ctx,#coerce))
        } else {
            quote!(<#ty as #tr>::decode_input(#raw,__rydantic_ctx,#coerce))
        }
    }
}
fn default_expr(field: &Field, borrow: bool) -> ::core::option::Option<Tokens> {
    let ty = &field.ty;
    let a = &field.attr;
    if a.default_trait {
        Some(quote!(<#ty as ::core::default::Default>::default()))
    } else if let Some(expr) = &a.default {
        Some(
            if matches!(
                expr,
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(_),
                    ..
                })
            ) {
                if borrow {
                    quote!(::core::convert::From::from(#expr))
                } else {
                    quote!((#expr).to_owned())
                }
            } else {
                quote!(#expr)
            },
        )
    } else {
        a.default_factory.as_ref().map(|path| quote!(#path()))
    }
}
fn field_value(field: &Field, raw: Tokens, mode: &Mode) -> Tokens {
    let rd = mode.rd;
    let tr = mode.tr();
    let ty = &field.ty;
    let rule = rules(field, rd);
    let missing_error = quote!({__rydantic_ctx.push(#rd::ValidationError::new("missing","field is required").with_stage(#rd::Stage::Parse));None});
    let default = default_expr(field, mode.borrow);
    let missing = if let Some(value) = default {
        quote!(Some({let value:#ty=#value;value}))
    } else if field.attr.required {
        missing_error.clone()
    } else {
        quote!(match <#ty as #tr>::missing(){Some(value)=>Some(value),None=>#missing_error})
    };
    let before=field.attr.before.as_ref().map(|path|quote!{
        let value=__rydantic_raw.into_value(__rydantic_ctx)?;
        __rydantic_raw=match #path(value){Ok(value)=>#rd::Input::Owned(value),Err(error)=>{__rydantic_ctx.push(error.with_stage(#rd::Stage::Parse));return None;}};
    });
    let decoded = mode.decode(ty, quote!(__rydantic_raw), field.attr.coerce);
    let default_flow = if field.attr.before.is_some() {
        quote! {let value:#ty=#missing?;let value=<#ty as #tr>::encode_input(&value,__rydantic_ctx);let mut __rydantic_raw=#rd::Input::Owned(value);#before #decoded?}
    } else {
        quote! {let value:#ty=#missing?;<#ty as #tr>::prepare_default(value,__rydantic_ctx)?}
    };
    let custom=field.attr.validate.as_ref().map(|path|quote!{if __rydantic_ctx.error_count()==__rydantic_start{if let Err(error)=#path(&__rydantic_value){__rydantic_ctx.push(error);}}});
    let builtin=field.attr.has_rules().then(||quote!{let rules=#rule;__rydantic_value=<#ty as #tr>::normalize(__rydantic_value,rules,__rydantic_ctx)?;<#ty as #tr>::validate_rules(&__rydantic_value,rules,__rydantic_ctx);});
    quote! {
        let __rydantic_start=__rydantic_ctx.error_count();
        let mut __rydantic_value=match #raw{Some(mut __rydantic_raw)=>{#before #decoded?},None=>{#default_flow}};
        #builtin #custom Some(__rydantic_value)
    }
}
fn with_location(field: &Field, index: usize, shape: Shape, body: Tokens, _rd: &Tokens) -> Tokens {
    if field.attr.flatten || field.attr.extra || (shape == Shape::Tuple && index == usize::MAX) {
        quote! {(||{#body})()}
    } else if shape == Shape::Tuple {
        quote! {__rydantic_ctx.at(#index,|__rydantic_ctx|{#body})}
    } else {
        let input = &field.input;
        quote! {__rydantic_ctx.at(#input,|__rydantic_ctx|{#body})}
    }
}
fn object_names(fields: &[Field], rd: &Tokens, tag: ::core::option::Option<&str>) -> Tokens {
    let names=fields.iter().filter(|f|!f.attr.extra).map(|f|{let ty=&f.ty;let input=&f.input;let output=&f.output;let exclude=f.attr.exclude;
        if f.attr.flatten{quote!{if !(mode==#rd::SchemaMode::Output&&#exclude){names.extend(<#ty as #rd::ObjectType>::field_names(mode,seen));}}}
        else{quote!{if !(mode==#rd::SchemaMode::Output&&#exclude){names.push(if mode==#rd::SchemaMode::Input{#input}else{#output}.to_owned());}}}
    });
    let tag = tag.map(|tag| quote!(names.push(#tag.to_owned());));
    quote! {let mut names=::std::vec::Vec::<::std::string::String>::new();#tag #(#names)* names}
}
fn collision_checks(
    fields: &[Field],
    values: &[Tokens],
    rd: &Tokens,
    tag: ::core::option::Option<&str>,
) -> Tokens {
    let tag = tag.map(|key| quote!(names.push(#key.to_owned());));
    let entries = fields.iter().zip(values).map(|(field, value)| {
        let ty=&field.ty; let input=&field.input; let output=&field.output; let exclude=field.attr.exclude;
        if field.attr.flatten || field.attr.extra {
            quote! { if !(mode==#rd::SchemaMode::Output && #exclude) { names.extend(<#ty as #rd::ObjectType>::reserved_names(#value,mode)); } }
        } else {
            quote! { if !(mode==#rd::SchemaMode::Output && #exclude) { names.push(if mode==#rd::SchemaMode::Input { #input } else { #output }.to_owned()); } }
        }
    });
    let captures=fields.iter().filter(|f|f.attr.flatten||f.attr.extra).map(|f|{let ty=&f.ty;quote!(usize::from(<#ty as #rd::ObjectType>::captures_extra(&mut ::std::collections::HashSet::new())))});
    quote! {
        if 0usize #(+ #captures)* > 1 { __rydantic_ctx.push(#rd::ValidationError::new("flatten_ambiguous","only one flattened field may capture unknown keys")); }
        for mode in [#rd::SchemaMode::Input,#rd::SchemaMode::Output] {
            let mut names=::std::vec::Vec::new(); #tag #(#entries)*
            #rd::__private::check_names(names,__rydantic_ctx);
        }
    }
}
fn layout_check(fields: &[Field], rd: &Tokens, tag: ::core::option::Option<&str>) -> Tokens {
    let names = object_names(fields, rd, tag);
    let catches=fields.iter().filter(|f|f.attr.extra||f.attr.flatten).map(|f|{let ty=&f.ty;quote!(usize::from(<#ty as #rd::ObjectType>::captures_extra(&mut ::std::collections::HashSet::new())))});
    quote! {
        for mode in [#rd::SchemaMode::Input,#rd::SchemaMode::Output]{let seen=&mut ::std::collections::HashSet::<&'static str>::new();let names={#names};#rd::__private::check_names(names,__rydantic_ctx);}
        if 0usize #(+ #catches)*>1{__rydantic_ctx.push(#rd::ValidationError::new("flatten_ambiguous","only one flattened field may capture unknown keys"));return None;}
    }
}
fn parse_shape(
    fields: &[Field],
    shape: Shape,
    constructor: Tokens,
    container: &attrs::Container,
    mode: &Mode,
    tag: ::core::option::Option<&str>,
) -> Tokens {
    let rd = mode.rd;
    let tr = mode.tr();
    let count = fields.len();
    let newtype = shape == Shape::Tuple && count == 1;
    let mut parses = vec![];
    let locals = (0..count).map(field_ident).collect::<Vec<_>>();
    // Declared fields are consumed before flattened maps, independently of Rust field order.
    let mut order = (0..count).collect::<Vec<_>>();
    order.sort_by_key(|i| fields[*i].attr.flatten || fields[*i].attr.extra);
    for i in order {
        let field = &fields[i];
        let local = &locals[i];
        let ty = &field.ty;
        let input = &field.input;
        let has_default = field.attr.has_default();
        let raw = if shape == Shape::Named {
            if field.attr.flatten || field.attr.extra {
                quote! {{let names=<#ty as #rd::ObjectType>::field_names(#rd::SchemaMode::Input,&mut ::std::collections::HashSet::new());
                    let capture=<#ty as #rd::ObjectType>::captures_extra(&mut ::std::collections::HashSet::new());
                    let mut object=::std::collections::BTreeMap::new();
                    for name in names{if let Some(value)=__rydantic_object.remove(&name){object.insert(name,value);}}
                    if capture{object.extend(::core::mem::take(&mut __rydantic_object));}
                    if object.is_empty()&&(<#ty as #tr>::allows_missing()||#has_default){None}else{Some(#rd::Input::Object(object))}
                }}
            } else {
                quote!(__rydantic_object.remove(#input))
            }
        } else if newtype {
            quote!(Some(__rydantic_raw))
        } else {
            quote!(__rydantic_items.next())
        };
        let body = field_value(field, raw, mode);
        let body = with_location(field, if newtype { usize::MAX } else { i }, shape, body, rd);
        if field.attr.flatten || field.attr.extra {
            let capture = quote!(<#ty as #rd::ObjectType>::captures_extra(&mut ::std::collections::HashSet::new()));
            parses.push((
                i,
                quote! {if !#capture{#local= #body;}},
                quote! {if #capture{#local= #body;}},
            ));
        } else {
            parses.push((i, quote! {#local= #body;}, quote!()));
        }
    }
    let first = parses.iter().map(|(_, a, _)| a);
    let last = parses.iter().map(|(_, _, b)| b);
    let unknown=(!container.ignore_extra&&shape==Shape::Named).then(||quote!{for (key,_) in __rydantic_object{__rydantic_ctx.at(key,|ctx|ctx.push(#rd::ValidationError::new("extra_forbidden","unknown field").with_stage(#rd::Stage::Parse)));}});
    let setup = match shape {
        Shape::Named => {
            let check = layout_check(fields, rd, tag);
            quote! {let mut __rydantic_object=__rydantic_raw.object(__rydantic_ctx)?;#check}
        }
        Shape::Tuple if !newtype => {
            quote! {let items=__rydantic_raw.array(__rydantic_ctx)?;if items.len()!=#count{__rydantic_ctx.push(#rd::ValidationError::new("tuple_length",concat!("expected ",stringify!(#count)," tuple items")).with_param("expected",#count).with_param("actual",items.len()));return None;}let mut __rydantic_items=items.into_iter();}
        }
        Shape::Unit => {
            quote! {if !__rydantic_raw.is_null(){__rydantic_ctx.push(#rd::ValidationError::new("type_error","expected null").with_stage(#rd::Stage::Parse));return None;}}
        }
        _ => quote!(),
    };
    let collision = if shape == Shape::Named {
        let values = locals
            .iter()
            .map(|local| quote!(&#local))
            .collect::<Vec<_>>();
        collision_checks(fields, &values, rd, tag)
    } else {
        quote!()
    };
    let done = if count == 0 {
        quote!({#collision Some(#constructor)})
    } else {
        quote! {match (#(#locals,)*){(#(Some(#locals),)*)=>{#collision Some(#constructor)},_=>None}}
    };
    quote! {#setup #(let mut #locals=None;)* #(#first)* #(#last)* #unknown #done}
}
fn encode_shape(
    fields: &[Field],
    values: &[Tokens],
    shape: Shape,
    mode: &Mode,
    input: bool,
    tag: ::core::option::Option<(&str, &str)>,
) -> Tokens {
    let rd = mode.rd;
    let tr = mode.tr();
    let method = if input {
        format_ident!("encode_input")
    } else {
        format_ident!("encode")
    };
    let newtype = shape == Shape::Tuple && fields.len() == 1;
    if shape == Shape::Unit {
        return quote!(#rd::Value::Null);
    }
    let mut entries = vec![];
    for (i, (field, value)) in fields.iter().zip(values).enumerate() {
        if !input && field.attr.exclude {
            continue;
        }
        let ty = &field.ty;
        let key = if input { &field.input } else { &field.output };
        let encoded = quote!(<#ty as #tr>::#method(#value,__rydantic_ctx));
        if newtype {
            return encoded;
        }
        if shape == Shape::Tuple {
            entries.push(
                quote! {__rydantic_items.push(__rydantic_ctx.at(#i,|__rydantic_ctx|#encoded));},
            );
        } else if field.attr.flatten || field.attr.extra {
            entries.push(quote!{let value=#encoded;#rd::__private::merge(&mut __rydantic_object,value,__rydantic_ctx);});
        } else {
            entries.push(quote!{let value=__rydantic_ctx.at(#key,|__rydantic_ctx|#encoded);#rd::__private::merge(&mut __rydantic_object,#rd::__private::serde_json::json!({#key:value}),__rydantic_ctx);});
        }
    }
    if shape == Shape::Tuple {
        quote! {let mut __rydantic_items=::std::vec::Vec::new();#(#entries)* #rd::Value::Array(__rydantic_items)}
    } else {
        let tag=tag.map(|(tag,wire)|quote!{__rydantic_object.insert(#tag.to_owned(),#rd::__private::serde_json::json!(#wire));});
        quote! {let mut __rydantic_object=#rd::__private::serde_json::Map::new();#tag #(#entries)* #rd::Value::Object(__rydantic_object)}
    }
}
fn validate_fields(fields: &[Field], values: &[Tokens], shape: Shape, mode: &Mode) -> Tokens {
    let rd = mode.rd;
    let tr = mode.tr();
    let checks=fields.iter().zip(values).enumerate().map(|(i,(field,value))|{
        let ty=&field.ty;let rule=rules(field,rd);
        let builtin=field.attr.has_rules().then(||quote!(<#ty as #tr>::validate_rules(#value,#rule,__rydantic_ctx);));
        let custom=field.attr.validate.as_ref().map(|path|quote!(if let Err(error)=#path(#value){__rydantic_ctx.push(error);}));
        let body=quote!{let start=__rydantic_ctx.error_count();<#ty as #tr>::validate_into(#value,__rydantic_ctx);if start==__rydantic_ctx.error_count(){#builtin #custom}};
        with_location(field,if shape==Shape::Tuple&&fields.len()==1{usize::MAX}else{i},shape,body,rd)
    });
    let collision = if shape == Shape::Named {
        collision_checks(fields, values, rd, None)
    } else {
        quote!()
    };
    quote!(#(#checks;)* #collision)
}
fn async_fields(fields: &[Field], values: &[Tokens], shape: Shape, rd: &Tokens) -> Tokens {
    let checks=fields.iter().zip(values).enumerate().map(|(i,(field,value))|{
        let ty=&field.ty;let input=&field.input;
        let segment=if field.attr.flatten||field.attr.extra||(shape==Shape::Tuple&&fields.len()==1){quote!(None)}else if shape==Shape::Tuple{quote!(Some(#rd::PathSegment::from(#i)))}else{quote!(Some(#rd::PathSegment::from(#input)))};
        let custom=field.attr.async_validate.as_ref().map(|path|quote!{if child.error_count()==start{if let Err(error)=#path(#value).await{child.push(error);}}});
        quote!{let mut child=__rydantic_ctx.child(#segment);let start=child.error_count();<#ty as #rd::AsyncCheck>::check_async(#value,&mut child).await;#custom __rydantic_ctx.absorb(child);}
    });
    quote!(#(#checks)*)
}
fn property_schema(field: &Field, mode: &Mode) -> Tokens {
    let rd = mode.rd;
    let tr = mode.tr();
    let ty = &field.ty;
    let coerce = field.attr.coerce;
    let rule = rules(field, rd);
    let before=field.attr.before.as_ref().map(|path|quote!{if __rydantic_ctx.mode==#rd::SchemaMode::Input{property=#rd::__private::serde_json::json!({});#rd::schema::annotate_local(&mut property,concat!("before:",stringify!(#path)));}});
    let condition = if field.attr.before.is_some() {
        quote!(__rydantic_ctx.mode==#rd::SchemaMode::Output)
    } else {
        quote!(true)
    };
    let custom=field.attr.validate.iter().map(|path|quote!(#rd::schema::annotate_local(&mut property,concat!("validate:",stringify!(#path)));));
    let asynchronous=field.attr.async_validate.iter().map(|path|quote!(#rd::schema::annotate_local(&mut property,concat!("async_validate:",stringify!(#path)));));
    let desc = field
        .doc
        .as_ref()
        .map(|text| quote!(property["description"]=#rd::__private::serde_json::json!(#text);));
    let literal = field.attr.default.as_ref().and_then(|expr| match expr {
        Expr::Lit(v) => match &v.lit {
            syn::Lit::Str(v) => Some(serde_json::json!(v.value())),
            syn::Lit::Bool(v) => Some(serde_json::json!(v.value)),
            syn::Lit::Int(v) => v
                .base10_digits()
                .parse()
                .ok()
                .map(serde_json::Value::Number),
            syn::Lit::Float(v) => v
                .base10_digits()
                .parse()
                .ok()
                .map(serde_json::Value::Number),
            _ => None,
        },
        _ => None,
    });
    let annotation=literal.map(|v|{let text=v.to_string();quote!(if __rydantic_ctx.mode==#rd::SchemaMode::Input{property["default"]=#rd::__private::serde_json::from_str::<#rd::Value>(#text).expect("literal default");})});
    quote! {{let mut property=<#ty as #tr>::schema(__rydantic_ctx,#coerce);#before if #condition{#rule.apply_schema(&mut property,__rydantic_ctx.mode,#coerce,<#ty as #tr>::kind());}#(#custom)* #(#asynchronous)* #desc #annotation property}}
}
fn schema_shape(
    fields: &[Field],
    shape: Shape,
    container: &attrs::Container,
    mode: &Mode,
    tag: ::core::option::Option<(&str, &str)>,
) -> Tokens {
    let rd = mode.rd;
    let tr = mode.tr();
    if shape == Shape::Unit {
        return quote!(#rd::__private::serde_json::json!({"type":"null"}));
    }
    if shape == Shape::Tuple {
        let props = fields.iter().map(|f| property_schema(f, mode));
        let count = fields.len();
        if count == 0 {
            return quote!(#rd::__private::serde_json::json!({"type":"array","items":false,"maxItems":0}));
        }
        if count == 1 {
            return props.into_iter().next().expect("one field");
        }
        return quote!(#rd::__private::serde_json::json!({"type":"array","prefixItems":[#((#props)),*],"items":false,"minItems":#count,"maxItems":#count}));
    }
    let ignore = container.ignore_extra;
    let known_names = object_names(fields, rd, tag.map(|(key, _)| key));
    let captures=fields.iter().filter(|f|f.attr.flatten||f.attr.extra).map(|f|{let ty=&f.ty;quote!(usize::from(<#ty as #rd::ObjectType>::captures_extra(&mut ::std::collections::HashSet::new())))});
    let tag=tag.map(|(key,value)|quote!{schema["properties"][#key]=#rd::__private::serde_json::json!({"type":"string","const":#value});schema["required"].as_array_mut().expect("required").push(#rd::__private::serde_json::json!(#key));});
    let inserts=fields.iter().map(|f|{let ty=&f.ty;let input=&f.input;let output=&f.output;let exclude=f.attr.exclude;let required=f.attr.required;let default=f.attr.has_default();let property=property_schema(f,mode);
        let insert=if f.attr.flatten||f.attr.extra{
            let optional=quote!((__rydantic_ctx.mode==#rd::SchemaMode::Input&&#default)||((!#required||__rydantic_ctx.mode==#rd::SchemaMode::Output)&&<#ty as #tr>::allows_missing()));
            quote!{let child=#property;#rd::__private::flatten_schema(&mut schema,child,(#optional),<#ty as #rd::ObjectType>::captures_extra(&mut ::std::collections::HashSet::new()),__rydantic_ctx);}
        }else{quote!{let key=if __rydantic_ctx.mode==#rd::SchemaMode::Input{#input}else{#output};let property=#property;schema["properties"].as_object_mut().expect("properties").insert(key.to_owned(),property);if __rydantic_ctx.mode==#rd::SchemaMode::Output||#required||(!#default&&!<#ty as #tr>::allows_missing()){schema["required"].as_array_mut().expect("required").push(#rd::__private::serde_json::json!(key));}}};
        quote!{if !(__rydantic_ctx.mode==#rd::SchemaMode::Output&&#exclude){#insert}}
    });
    quote! {{let mut schema=#rd::__private::serde_json::json!({"type":"object","properties":{},"required":[],"allOf":[],"additionalProperties":(#ignore&&__rydantic_ctx.mode==#rd::SchemaMode::Input)});#tag #(#inserts)*
    let captures=0usize #(+ #captures)*;
    if captures>1 { schema["allOf"].as_array_mut().expect("allOf").push(#rd::Value::Bool(false)); }
    let mut views=::std::vec::Vec::new();
    for mode in [#rd::SchemaMode::Input,#rd::SchemaMode::Output] {
        let seen=&mut ::std::collections::HashSet::<&'static str>::new(); let names={#known_names};
        let mut unique=::std::collections::HashSet::new();
        if names.iter().any(|name|!unique.insert(name)) { schema["allOf"].as_array_mut().expect("allOf").push(#rd::Value::Bool(false)); }
        views.push(names);
    }
    let mut required=::std::collections::HashSet::new();
    schema["required"].as_array_mut().expect("required").retain(|key|required.insert(key.as_str().expect("key").to_owned()));
    if captures>0 && views[0]!=views[1] && __rydantic_ctx.mode==#rd::SchemaMode::Input {#rd::schema::annotate_local(&mut schema,"extra_alias_collision");}
    if schema["allOf"].as_array().is_some_and(|v|v.is_empty()){schema.as_object_mut().expect("schema").remove("allOf");}schema}}
}
fn constructor(fields: &[Field], shape: Shape, path: Tokens) -> Tokens {
    let locals = (0..fields.len()).map(field_ident).collect::<Vec<_>>();
    let names = fields.iter().map(|f| &f.ident);
    match shape {
        Shape::Named => quote!(#path{#(#names:#locals,)*}),
        Shape::Tuple => quote!(#path(#(#locals,)*)),
        Shape::Unit => path,
    }
}
fn pattern(fields: &[Field], shape: Shape, path: Tokens) -> (Tokens, Vec<Tokens>) {
    let locals = (0..fields.len()).map(field_ident).collect::<Vec<_>>();
    let names = fields.iter().map(|f| &f.ident);
    let values = locals.iter().map(|l| quote!(#l)).collect();
    let pat = match shape {
        Shape::Named => quote!(#path{#(#names:#locals,)*}),
        Shape::Tuple => quote!(#path(#(#locals,)*)),
        Shape::Unit => path,
    };
    (pat, values)
}
fn prepare_fields(fields: &[Field], shape: Shape, mode: &Mode, path: Tokens) -> Tokens {
    let rd = mode.rd;
    let tr = mode.tr();
    let (pat, _) = pattern(fields, shape, path.clone());
    let constructor = constructor(fields, shape, path);
    let locals = (0..fields.len()).map(field_ident).collect::<Vec<_>>();
    let prepare=fields.iter().enumerate().map(|(i,f)|{let ty=&f.ty;let local=&locals[i];let rule=rules(f,rd);
        let raw=if let Some(path)=&f.attr.before{let decoded=mode.decode(ty,quote!(#rd::Input::Owned(raw)),f.attr.coerce);quote!{let raw=<#ty as #tr>::encode_input(&#local,__rydantic_ctx);let raw=match #path(raw){Ok(v)=>v,Err(e)=>{__rydantic_ctx.push(e.with_stage(#rd::Stage::Parse));return None;}};let mut value=#decoded?;}}
        else{quote!(let mut value=<#ty as #tr>::prepare_default(#local,__rydantic_ctx)?;)};
        let builtin=f.attr.has_rules().then(||quote!{value=<#ty as #tr>::normalize(value,#rule,__rydantic_ctx)?;<#ty as #tr>::validate_rules(&value,#rule,__rydantic_ctx);});
        let custom=f.attr.validate.as_ref().map(|path|quote!{if start==__rydantic_ctx.error_count(){if let Err(error)=#path(&value){__rydantic_ctx.push(error);}}});
        let body=quote!{let start=__rydantic_ctx.error_count();#raw #builtin #custom Some(value)};
        let body=with_location(f,if shape==Shape::Tuple&&fields.len()==1{usize::MAX}else{i},shape,body,rd);quote!(let #local=#body;)
    });
    let done = if fields.is_empty() {
        quote!(Some(#constructor))
    } else {
        quote!(match (#(#locals,)*){(#(Some(#locals),)*)=>Some(#constructor),_=>None})
    };
    quote!(#pat=>{#(#prepare)* #done})
}
fn expand(input: DeriveInput) -> syn::Result<Tokens> {
    let rd = match proc_macro_crate::crate_name("rydantic")
        .map_err(|e| syn::Error::new(input.ident.span(), e))?
    {
        proc_macro_crate::FoundCrate::Itself => quote!(::rydantic),
        proc_macro_crate::FoundCrate::Name(name) => {
            let name = format_ident!("{name}");
            quote!(::#name)
        }
    };
    let container = attrs::container(&input.attrs)?;
    let native_capnp = native::expand(&input, &container, &rd)?;
    let name = &input.ident;
    let title = container.title.clone().unwrap_or_else(|| name.to_string());
    let borrowed = input.generics.lifetimes().next().is_some();
    let mode = Mode {
        rd: &rd,
        borrow: borrowed,
    };
    let tr = mode.tr();
    let mut all_fields = Vec::new();
    let mut object_impl = None;
    let (decode, encode, encode_input, validate, schema, asynchronous, kind) = match &input.data {
        Data::Struct(data) => {
            if container.tag.is_some() || container.untagged {
                return Err(syn::Error::new(
                    name.span(),
                    "tag and untagged are supported on enums",
                ));
            }
            let fields = fields(&data.fields, None, &container)?;
            let shape = shape(&data.fields);
            let ctor = constructor(&fields, shape, quote!(Self));
            let decode = parse_shape(&fields, shape, ctor, &container, &mode, None);
            let values = fields
                .iter()
                .map(|f| {
                    let m = &f.member;
                    quote!(&self.#m)
                })
                .collect::<Vec<_>>();
            let encode = encode_shape(&fields, &values, shape, &mode, false, None);
            let encode_input = encode_shape(&fields, &values, shape, &mode, true, None);
            let validate = validate_fields(&fields, &values, shape, &mode);
            let schema = schema_shape(&fields, shape, &container, &mode, None);
            let asynchronous = async_fields(&fields, &values, shape, &rd);
            let kind = match shape {
                Shape::Named => quote!(#rd::TypeKind::Object),
                Shape::Tuple if fields.len() == 1 => {
                    let ty = &fields[0].ty;
                    quote!(<#ty as #tr>::kind())
                }
                Shape::Tuple => quote!(#rd::TypeKind::Array),
                Shape::Unit => quote!(#rd::TypeKind::Any),
            };
            if shape == Shape::Named {
                let names = object_names(&fields, &rd, None);
                let present=fields.iter().map(|f|{let ty=&f.ty;let member=&f.member;let input=&f.input;let output=&f.output;let exclude=f.attr.exclude;
                    if f.attr.flatten||f.attr.extra{quote!{if !(mode==#rd::SchemaMode::Output&&#exclude){names.extend(<#ty as #rd::ObjectType>::reserved_names(&self.#member,mode));}}}
                    else{quote!{if !(mode==#rd::SchemaMode::Output&&#exclude){names.push(if mode==#rd::SchemaMode::Input{#input}else{#output}.to_owned());}}}
                });
                let extra = fields
                    .iter()
                    .filter(|f| f.attr.flatten || f.attr.extra)
                    .map(|f| {
                        let ty = &f.ty;
                        quote!(<#ty as #rd::ObjectType>::captures_extra(seen))
                    });
                object_impl = Some(quote! {
                    fn reserved_names(&self,mode:#rd::SchemaMode)->::std::vec::Vec<::std::string::String>{let mut names=::std::vec::Vec::new();#(#present)* names}
                    fn field_names(mode:#rd::SchemaMode,seen:&mut ::std::collections::HashSet<&'static str>)->::std::vec::Vec<::std::string::String>{if !seen.insert(::core::any::type_name::<Self>()){return ::std::vec::Vec::new();}let names={#names};seen.remove(::core::any::type_name::<Self>());names}
                    fn captures_extra(seen:&mut ::std::collections::HashSet<&'static str>)->bool{if !seen.insert(::core::any::type_name::<Self>()){return false;}let captures=false #(|| #extra)*;seen.remove(::core::any::type_name::<Self>());captures}
                });
            }
            all_fields.extend(fields);
            (
                decode,
                encode,
                encode_input,
                validate,
                schema,
                asynchronous,
                kind,
            )
        }
        Data::Enum(data) => {
            if data.variants.is_empty() {
                return Err(syn::Error::new(
                    name.span(),
                    "Model requires at least one enum variant",
                ));
            }
            let mut variants = vec![];
            let mut seen = std::collections::HashSet::new();
            for v in &data.variants {
                let wire = attrs::variant_name(&v.attrs, &v.ident.to_string())?;
                if !seen.insert(wire.clone()) {
                    return Err(syn::Error::new(
                        v.ident.span(),
                        "duplicate enum variant name",
                    ));
                }
                let shape = shape(&v.fields);
                if container.tag.is_some() && shape == Shape::Tuple {
                    return Err(syn::Error::new(v.ident.span(),"internally tagged enums require named fields or unit variants; use untagged for tuple variants"));
                }
                if container.tag.is_none() && !container.untagged && shape != Shape::Unit {
                    return Err(syn::Error::new(
                        v.ident.span(),
                        "enums with fields require #[model(tag = \"...\")] or #[model(untagged)]",
                    ));
                }
                variants.push((
                    &v.ident,
                    wire,
                    shape,
                    fields(&v.fields, container.tag.as_deref(), &container)?,
                ));
            }
            let mut decodes = vec![];
            let mut encodes = vec![];
            let mut input_encodes = vec![];
            let mut validates = vec![];
            let mut asyncs = vec![];
            let mut schemas = vec![];
            let wires = variants.iter().map(|(_, w, _, _)| w).collect::<Vec<_>>();
            for (variant, wire, shape, fields) in &variants {
                let (pat, values) = pattern(fields, *shape, quote!(Self::#variant));
                let ctor = constructor(fields, *shape, quote!(Self::#variant));
                let wire_shape = if container.tag.is_some() {
                    Shape::Named
                } else {
                    *shape
                };
                let tag = container.tag.as_deref().map(|t| (t, wire.as_str()));
                let (encode, encode_input, schema) =
                    if !container.untagged && container.tag.is_none() {
                        (
                            quote!(#rd::__private::serde_json::json!(#wire)),
                            quote!(#rd::__private::serde_json::json!(#wire)),
                            quote!(),
                        )
                    } else {
                        (
                            encode_shape(fields, &values, wire_shape, &mode, false, tag),
                            encode_shape(fields, &values, wire_shape, &mode, true, tag),
                            schema_shape(fields, wire_shape, &container, &mode, tag),
                        )
                    };
                encodes.push(quote!(#pat=>{#encode}));
                input_encodes.push(quote!(#pat=>{#encode_input}));
                let checks = validate_fields(fields, &values, *shape, &mode);
                validates.push(quote!(#pat=>{#checks}));
                let checks = async_fields(fields, &values, *shape, &rd);
                asyncs.push(quote!(#pat=>{#checks}));
                schemas.push(schema);
                if container.untagged {
                    let body = parse_shape(fields, *shape, ctor, &container, &mode, None);
                    decodes.push(quote!{
                        {let mut trial=#rd::Context::new();let result={let __rydantic_ctx=&mut trial;let __rydantic_raw=__rydantic_raw.clone();(||{#body})()};
                        match trial.finish(result){Ok(value)=>return Some(value),Err(errors)=>__rydantic_branches.push(#rd::__private::serde_json::json!({"variant":#wire,"errors":errors.to_json()}))}}
                    });
                } else if container.tag.is_some() {
                    let body = parse_shape(
                        fields,
                        wire_shape,
                        ctor,
                        &container,
                        &mode,
                        container.tag.as_deref(),
                    );
                    decodes.push(quote!(#wire=>{let __rydantic_raw=#rd::Input::Object(__rydantic_object);#body}));
                } else {
                    decodes.push(quote!(#wire=>Some(#ctor)));
                }
            }
            let (decode, schema, kind) = if container.untagged {
                (
                    quote! {let mut __rydantic_branches=::std::vec::Vec::<#rd::Value>::new();#(#decodes)* __rydantic_ctx.push(#rd::ValidationError::new("union_no_match","no untagged variant accepted the value").with_stage(#rd::Stage::Parse).with_param("branches",#rd::Value::Array(__rydantic_branches)));None},
                    quote!(#rd::__private::serde_json::json!({"anyOf":[#((#schemas)),*]})),
                    quote!(#rd::TypeKind::Any),
                )
            } else if let Some(tag) = &container.tag {
                (
                    quote! {let mut __rydantic_object=__rydantic_raw.object(__rydantic_ctx)?;let tag=match __rydantic_object.remove(#tag){Some(raw)=>{let value=raw.into_value(__rydantic_ctx)?;match value{#rd::Value::String(value)=>value,_=>{__rydantic_ctx.at(#tag,|ctx|ctx.push(#rd::ValidationError::new("union_tag_type","discriminator must be a string").with_stage(#rd::Stage::Parse)));return None;}}},None=>{__rydantic_ctx.at(#tag,|ctx|ctx.push(#rd::ValidationError::new("union_tag_missing","discriminator is required").with_stage(#rd::Stage::Parse)));return None;}};
                    match tag.as_str(){#(#decodes,)* _=>{__rydantic_ctx.at(#tag,|ctx|ctx.push(#rd::ValidationError::new("union_tag_invalid","unknown discriminator").with_stage(#rd::Stage::Parse).with_param("expected",#rd::__private::serde_json::json!([#(#wires),*]))));None}}},
                    quote!(#rd::__private::serde_json::json!({"oneOf":[#((#schemas)),*],"discriminator":{"propertyName":#tag}})),
                    quote!(#rd::TypeKind::Object),
                )
            } else {
                (
                    quote! {let raw=__rydantic_raw.into_value(__rydantic_ctx)?;match raw.as_str(){Some(wire)=>match wire{#(#decodes,)* _=>{__rydantic_ctx.push(#rd::ValidationError::new("enum","unknown enum value").with_stage(#rd::Stage::Parse).with_param("expected",#rd::__private::serde_json::json!([#(#wires),*])));None}},None=>{__rydantic_ctx.push(#rd::ValidationError::new("type_error","expected enum string").with_stage(#rd::Stage::Parse));None}}},
                    quote!(#rd::__private::serde_json::json!({"type":"string","enum":[#(#wires),*]})),
                    quote!(#rd::TypeKind::String),
                )
            };
            for (_, _, _, fields) in variants {
                all_fields.extend(fields);
            }
            (
                decode,
                quote!(match self{#(#encodes,)*}),
                quote!(match self{#(#input_encodes,)*}),
                quote!(match self{#(#validates,)*}),
                schema,
                quote!(match self{#(#asyncs,)*}),
                kind,
            )
        }
        Data::Union(_) => {
            return Err(syn::Error::new(
                name.span(),
                "Model cannot be derived for unions",
            ))
        }
    };
    let prepare_default = {
        let arms = match &input.data {
            Data::Struct(data) => {
                let fields = fields(&data.fields, None, &container)?;
                let arm = prepare_fields(&fields, shape(&data.fields), &mode, quote!(Self));
                quote!(#arm)
            }
            Data::Enum(data) => {
                let mut arms = vec![];
                for v in &data.variants {
                    let variant = &v.ident;
                    let fields = fields(&v.fields, container.tag.as_deref(), &container)?;
                    arms.push(prepare_fields(
                        &fields,
                        shape(&v.fields),
                        &mode,
                        quote!(Self::#variant),
                    ));
                }
                quote!(#(#arms,)*)
            }
            _ => unreachable!(),
        };
        let custom=container.validate.as_ref().map(|path|quote!{if start==ctx.error_count(){if let Some(value)=&value{if let Err(errors)=#path(value){ctx.extend(errors);}}}});
        quote! {fn prepare_default(self,ctx:&mut #rd::Context)->::core::option::Option<Self>{let start=ctx.error_count();let value={let __rydantic_ctx=&mut *ctx;match self{#arms}};#custom value}}
    };
    let mut generics = input.generics.clone();
    if borrowed {
        generics.params.insert(0, parse_quote!('__rydantic_de));
        for lt in input.generics.lifetimes() {
            let lt = &lt.lifetime;
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!('__rydantic_de:#lt));
        }
    }
    for p in generics.type_params_mut() {
        p.bounds.push(parse_quote!(#tr));
    }
    fn contains_generic(ty: &Type, ident: &Ident) -> bool {
        match ty{Type::Path(p)=>p.path.segments.iter().any(|s|s.ident==*ident||matches!(&s.arguments,syn::PathArguments::AngleBracketed(a) if a.args.iter().any(|arg|matches!(arg,syn::GenericArgument::Type(t)if contains_generic(t,ident))))),Type::Array(a)=>contains_generic(&a.elem,ident),Type::Reference(r)=>contains_generic(&r.elem,ident),Type::Tuple(t)=>t.elems.iter().any(|t|contains_generic(t,ident)),_=>false}
    }
    let mut assertions = vec![];
    for f in &all_fields {
        let ty = &f.ty;
        let a = &f.attr;
        let generic = input
            .generics
            .type_params()
            .any(|p| contains_generic(ty, &p.ident));
        let mut bounds = vec![];
        if a.trim || a.lowercase || a.uppercase || a.email || a.pattern.is_some() {
            assertions.push(quote!(__rydantic_assert_string::<#ty>();));
            bounds.push(quote!(#rd::StringType));
        }
        if a.min_length.is_some() || a.max_length.is_some() {
            assertions.push(quote!(__rydantic_assert_length::<#ty>();));
            bounds.push(quote!(#rd::LengthType));
        }
        if a.ge.is_some() || a.gt.is_some() || a.le.is_some() || a.lt.is_some() {
            assertions.push(quote!(__rydantic_assert_number::<#ty>();));
            bounds.push(quote!(#rd::NumericType));
        }
        if a.flatten {
            bounds.push(quote!(#rd::ObjectType));
        }
        if a.extra {
            bounds.push(quote!(#rd::ExtraFields));
            assertions.push(quote!(__rydantic_assert_extra::<#ty>();));
        }
        if a.default_trait {
            bounds.push(quote!(::core::default::Default));
        }
        if generic {
            for bound in bounds {
                generics
                    .make_where_clause()
                    .predicates
                    .push(parse_quote!(#ty:#bound));
            }
        }
    }
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let mut de_generics = generics.clone();
    if !borrowed {
        de_generics.params.insert(0, parse_quote!('__rydantic_de));
    }
    let (de_impl_generics, _, de_where_clause) = de_generics.split_for_impl();
    let custom_parse=container.validate.as_ref().map(|path|quote!{if __rydantic_ctx.error_count()==start{if let Some(value)=&value{if let Err(errors)=#path(value){__rydantic_ctx.extend(errors);}}}});
    let custom_validate=container.validate.as_ref().map(|path|quote!{if __rydantic_ctx.error_count()==start{if let Err(errors)=#path(self){__rydantic_ctx.extend(errors);}}});
    let schema_custom=container.validate.as_ref().map(|path|quote!(#rd::schema::annotate_local(&mut schema,concat!("validate:",stringify!(#path)));));
    let schema_async=container.async_validate.as_ref().map(|path|quote!(#rd::schema::annotate_local(&mut schema,concat!("async_validate:",stringify!(#path)));));
    let direct_async = container.async_validate.is_some()
        || all_fields.iter().any(|f| f.attr.async_validate.is_some());
    let nested_async = all_fields.iter().map(|f| {
        let ty = &f.ty;
        quote!(<#ty as #tr>::async_required(seen))
    });
    let model_impl = if borrowed {
        quote!(impl #impl_generics #rd::BorrowModel<'__rydantic_de> for #name #ty_generics #where_clause {})
    } else {
        quote!(impl #impl_generics #rd::Model for #name #ty_generics #where_clause {})
    };
    let dump = if borrowed {
        quote!(<Self as #rd::BorrowModel<'__rydantic_de>>::dump(self))
    } else {
        quote!(<Self as #rd::Model>::dump(self))
    };
    let deserialize = if borrowed {
        quote! {let raw=<&'__rydantic_de #rd::__private::serde_json::value::RawValue as #rd::__private::serde::Deserialize>::deserialize(deserializer)?;<Self as #rd::BorrowModel<'__rydantic_de>>::parse_raw(raw).map_err(#rd::__private::serde::de::Error::custom)}
    } else {
        quote! {let raw=<::std::boxed::Box<#rd::RawValue> as #rd::__private::serde::Deserialize>::deserialize(deserializer)?;<Self as #rd::Model>::parse_raw(&raw).map_err(#rd::__private::serde::de::Error::custom)}
    };
    let object_impl=object_impl.map(|body|quote!(impl #impl_generics #rd::ObjectType for #name #ty_generics #where_clause{#body}));
    let bridge=(!borrowed).then(||quote!{
        impl #de_impl_generics #rd::BorrowType<'__rydantic_de> for #name #ty_generics #de_where_clause{
            fn decode(raw:#rd::Input<'__rydantic_de>,ctx:&mut #rd::Context,coerce:bool)->::core::option::Option<Self>{<Self as #rd::JsonType>::decode_input(raw,ctx,coerce)}
            fn prepare_default(self,ctx:&mut #rd::Context)->::core::option::Option<Self>{<Self as #rd::JsonType>::prepare_default(self,ctx)}
            fn encode(&self,ctx:&mut #rd::Context)->#rd::Value{<Self as #rd::JsonType>::encode(self,ctx)}
            fn encode_input(&self,ctx:&mut #rd::Context)->#rd::Value{<Self as #rd::JsonType>::encode_input(self,ctx)}
            fn schema(ctx:&mut #rd::SchemaContext,coerce:bool)->#rd::Value{<Self as #rd::JsonType>::schema(ctx,coerce)}
            fn validate_into(&self,ctx:&mut #rd::Context){<Self as #rd::JsonType>::validate_into(self,ctx)}
            fn normalize(self,rules:&#rd::rules::Rules,ctx:&mut #rd::Context)->::core::option::Option<Self>{<Self as #rd::JsonType>::normalize(self,rules,ctx)}
            fn validate_rules(&self,rules:&#rd::rules::Rules,ctx:&mut #rd::Context){<Self as #rd::JsonType>::validate_rules(self,rules,ctx)}
            fn async_required(seen:&mut ::std::collections::HashSet<&'static str>)->bool{<Self as #rd::JsonType>::async_required(seen)}
            fn kind()->#rd::TypeKind{<Self as #rd::JsonType>::kind()}
        }
    });
    let mut async_generics = input.generics.clone();
    for p in async_generics.type_params_mut() {
        p.bounds.push(parse_quote!(#rd::AsyncCheck));
    }
    let (async_impl_generics, _, async_where_clause) = async_generics.split_for_impl();
    let custom_async=container.async_validate.as_ref().map(|path|quote!{if start==__rydantic_ctx.error_count(){if let Err(errors)=#path(self).await{__rydantic_ctx.extend(errors);}}});
    let body = quote! {
        fn __rydantic_assert_string<T:#rd::StringType>(){}fn __rydantic_assert_length<T:#rd::LengthType>(){}fn __rydantic_assert_number<T:#rd::NumericType>(){}fn __rydantic_assert_extra<T:#rd::ExtraFields>(){}
        #(#assertions)* let start=__rydantic_ctx.error_count();let value=(||{#decode})();#custom_parse value
    };
    let decode_methods = if borrowed {
        quote! {fn decode(__rydantic_raw:#rd::Input<'__rydantic_de>,__rydantic_ctx:&mut #rd::Context,_coerce:bool)->::core::option::Option<Self>{#body}}
    } else {
        quote! {
            fn decode(raw:#rd::Value,ctx:&mut #rd::Context,coerce:bool)->::core::option::Option<Self>{<Self as #rd::JsonType>::decode_input(#rd::Input::Owned(raw),ctx,coerce)}
            fn decode_input(__rydantic_raw:#rd::Input<'_>,__rydantic_ctx:&mut #rd::Context,_coerce:bool)->::core::option::Option<Self>{#body}
        }
    };
    Ok(quote! {
        impl #impl_generics #tr for #name #ty_generics #where_clause{
            #decode_methods
            #prepare_default
            fn encode(&self,__rydantic_ctx:&mut #rd::Context)->#rd::Value{#encode}
            fn encode_input(&self,__rydantic_ctx:&mut #rd::Context)->#rd::Value{#encode_input}
            fn validate_into(&self,__rydantic_ctx:&mut #rd::Context){let start=__rydantic_ctx.error_count();#validate #custom_validate}
            fn schema(__rydantic_ctx:&mut #rd::SchemaContext,_coerce:bool)->#rd::Value{__rydantic_ctx.reference(::core::any::type_name::<Self>(),|__rydantic_ctx|{let mut schema=#schema;schema["title"]=#rd::__private::serde_json::json!(#title);#schema_custom #schema_async schema})}
            fn kind()->#rd::TypeKind{#kind}
            fn async_required(seen:&mut ::std::collections::HashSet<&'static str>)->bool{if !seen.insert(::core::any::type_name::<Self>()){return false;}#direct_async #(|| #nested_async)*}
        }
        #model_impl #object_impl #bridge #native_capnp
        impl #async_impl_generics #rd::AsyncCheck for #name #ty_generics #async_where_clause{
            fn check_async<'__rydantic_future>(&'__rydantic_future self,__rydantic_ctx:&'__rydantic_future mut #rd::Context)->#rd::BoxFuture<'__rydantic_future,()>{
                ::std::boxed::Box::pin(async move{let start=__rydantic_ctx.error_count();#asynchronous #custom_async})
            }
        }
        impl #impl_generics #rd::__private::serde::Serialize for #name #ty_generics #where_clause{
            fn serialize<S:#rd::__private::serde::Serializer>(&self,serializer:S)->::core::result::Result<S::Ok,S::Error>{let value=#dump.map_err(#rd::__private::serde::ser::Error::custom)?;#rd::__private::serde::Serialize::serialize(&value,serializer)}
        }
        impl #de_impl_generics #rd::__private::serde::Deserialize<'__rydantic_de> for #name #ty_generics #de_where_clause{
            fn deserialize<D:#rd::__private::serde::Deserializer<'__rydantic_de>>(deserializer:D)->::core::result::Result<Self,D::Error>{#deserialize}
        }
    })
}
