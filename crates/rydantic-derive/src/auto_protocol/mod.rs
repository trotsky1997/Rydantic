//! Rust models are the source of truth; the official compiler determines wire layout.
mod protocol;
mod types;
use crate::attrs;
use heck::{ToSnakeCase, ToUpperCamelCase};
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{quote, ToTokens};
use sha2::{Digest, Sha256};
use std::{io::Cursor, process::Command};
use syn::{
    parse_quote, spanned::Spanned, visit_mut::VisitMut, Data, DeriveInput, Item, ItemMod, LitInt,
    LitStr, Path, Type,
};
use types::Registry;

#[derive(Default)]
struct Options {
    id: Option<u64>,
    namespace: Option<String>,
}
fn options(tokens: Tokens) -> syn::Result<Options> {
    let mut result = Options::default();
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("id") && result.id.is_none() {
            result.id = Some(meta.value()?.parse::<LitInt>()?.base10_parse()?);
        } else if meta.path.is_ident("namespace") && result.namespace.is_none() {
            result.namespace = Some(meta.value()?.parse::<LitStr>()?.value());
        } else {
            return Err(meta.error("expected a unique id = 0x... or namespace = \"...\""));
        }
        Ok(())
    });
    syn::parse::Parser::parse2(parser, tokens)?;
    if result.id.is_some_and(|id| id >> 63 == 0) {
        return Err(syn::Error::new(
            Span::call_site(),
            "Cap'n Proto file IDs must have the high bit set",
        ));
    }
    if result.id.is_some() && result.namespace.is_some() {
        return Err(syn::Error::new(Span::call_site(), "choose id or namespace"));
    }
    if result.namespace.as_ref().is_some_and(|ns| ns.is_empty()) {
        return Err(syn::Error::new(
            Span::call_site(),
            "namespace must not be empty",
        ));
    }
    Ok(result)
}
fn stable_id(value: &str) -> u64 {
    u64::from_be_bytes(Sha256::digest(value.as_bytes())[..8].try_into().unwrap()) | (1 << 63)
}
fn root_path() -> syn::Result<Tokens> {
    Ok(
        match proc_macro_crate::crate_name("rydantic")
            .map_err(|e| syn::Error::new(Span::call_site(), e))?
        {
            proc_macro_crate::FoundCrate::Itself => quote!(::rydantic),
            proc_macro_crate::FoundCrate::Name(name) => {
                let name = syn::Ident::new(&name, Span::call_site());
                quote!(::#name)
            }
        },
    )
}
fn relative_root(scope: &[String]) -> Tokens {
    if scope.is_empty() {
        quote!(self)
    } else {
        let supers = scope.iter().map(|_| quote!(super));
        quote!(#(#supers)::*)
    }
}
fn relative_model(path: &[String], scope: &[String]) -> Type {
    let root = relative_root(scope);
    let ids = path
        .iter()
        .map(|id| syn::Ident::new_raw(id, Span::call_site()));
    syn::parse2(quote!(#root::#(#ids)::* )).expect("validated Rust model path")
}
fn valid_name(name: &str, upper: bool) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| {
        if upper {
            c.is_ascii_uppercase()
        } else {
            c.is_ascii_lowercase()
        }
    }) && chars.all(|c| c.is_ascii_alphanumeric())
}
fn protocol_name(text: &str, upper: bool) -> String {
    use heck::ToLowerCamelCase;
    let name = if upper {
        text.to_upper_camel_case()
    } else {
        text.to_lower_camel_case()
    };
    if valid_name(&name, upper)
        && !matches!(
            name.as_str(),
            "union"
                | "group"
                | "struct"
                | "enum"
                | "interface"
                | "using"
                | "import"
                | "const"
                | "annotation"
        )
    {
        name
    } else {
        format!(
            "{}{:x}",
            if upper { "Model" } else { "field" },
            stable_id(text)
        )
    }
}
fn wire_name(field: &syn::Field, index: usize, count: usize) -> syn::Result<String> {
    let attr = attrs::field(&field.attrs)?;
    let original = attr.capnp_name.unwrap_or_else(|| {
        field
            .ident
            .as_ref()
            .map(|id| id.to_string().trim_start_matches("r#").to_owned())
            .unwrap_or_else(|| {
                if count == 1 {
                    "value".into()
                } else {
                    format!("item{index}")
                }
            })
    });
    Ok(protocol_name(&original, false))
}
fn ordinal(attributes: &[syn::Attribute]) -> syn::Result<Option<u16>> {
    let mut result = None;
    for attr in attributes.iter().filter(|a| a.path().is_ident("model")) {
        let metas = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        )?;
        for meta in metas {
            if let syn::Meta::NameValue(pair) = meta {
                if pair.path.is_ident("capnp_ordinal") {
                    let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Int(value),
                        ..
                    }) = pair.value
                    else {
                        return Err(syn::Error::new(
                            pair.span(),
                            "capnp_ordinal requires an integer",
                        ));
                    };
                    if result.replace(value.base10_parse()?).is_some() {
                        return Err(syn::Error::new(value.span(), "duplicate capnp_ordinal"));
                    }
                }
            }
        }
    }
    Ok(result)
}
fn ordinals(values: &[Option<u16>], span: Span) -> syn::Result<Vec<u16>> {
    if values.len() > u16::MAX as usize {
        return Err(syn::Error::new(span, "too many native protocol fields"));
    }
    let result = if values.iter().all(Option::is_none) {
        (0..values.len() as u16).collect::<Vec<_>>()
    } else {
        if values.iter().any(Option::is_none) {
            return Err(syn::Error::new(
                span,
                "when using capnp_ordinal, number every wire field or enum member",
            ));
        }
        values.iter().map(|n| n.unwrap()).collect()
    };
    let mut sorted = result.clone();
    sorted.sort_unstable();
    if sorted.iter().enumerate().any(|(i, n)| i != *n as usize) {
        return Err(syn::Error::new(
            span,
            "capnp_ordinal values must be unique and contiguous from zero",
        ));
    }
    Ok(result)
}

fn model_input(item: &Item) -> Option<DeriveInput> {
    match item {
        Item::Struct(_) | Item::Enum(_) => syn::parse2(item.to_token_stream()).ok(),
        _ => None,
    }
}
fn ensure_derive(input: &mut DeriveInput, rd: &Tokens) {
    let exists = input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("derive"))
        .any(|a| {
            a.parse_args_with(syn::punctuated::Punctuated::<Path, syn::Token![,]>::parse_terminated)
                .is_ok_and(|paths| {
                    paths
                        .iter()
                        .any(|p| p.segments.last().is_some_and(|s| s.ident == "Model"))
                })
        });
    if !exists {
        input.attrs.insert(0, parse_quote!(#[derive(#rd::Model)]));
    }
}
fn set_native_name(attributes: &mut Vec<syn::Attribute>, name: &str) -> syn::Result<()> {
    let mut retained = Vec::new();
    for attr in attributes.drain(..) {
        if attr.path().is_ident("model") {
            let metas = attr.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )?;
            let metas = metas
                .into_iter()
                .filter(|meta| !meta.path().is_ident("capnp_name"))
                .collect::<Vec<_>>();
            if !metas.is_empty() {
                retained.push(parse_quote!(#[model(#(#metas),*)]));
            }
        } else {
            retained.push(attr);
        }
    }
    retained.push(parse_quote!(#[model(capnp_name=#name)]));
    *attributes = retained;
    Ok(())
}
fn add_field_metadata(
    field: &mut syn::Field,
    index: usize,
    count: usize,
    scope: &[String],
    registry: &Registry,
    rd: &Tokens,
) -> syn::Result<()> {
    let existing = attrs::field(&field.attrs)?;
    if existing.capnp_skip {
        return Ok(());
    }
    let spec = registry.resolve(&field.ty, scope, &mut Vec::new())?;
    let hints = spec.hints();
    if existing.capnp_data && !hints.data {
        return Err(syn::Error::new(
            field.span(),
            "capnp_data requires a byte vector, optionally nested in containers",
        ));
    }
    if existing.capnp_enum && !hints.enumeration {
        return Err(syn::Error::new(
            field.span(),
            "capnp_enum requires a unit enum model",
        ));
    }
    if existing.capnp_text && hints.data {
        return Err(syn::Error::new(
            field.span(),
            "a byte vector maps to Data; capnp_text requires a text or numeric scalar",
        ));
    }
    let name = wire_name(field, index, count)?;
    set_native_name(&mut field.attrs, &name)?;
    let resolved = spec.rust_type(scope, rd);
    let resolved = resolved.to_token_stream().to_string();
    if existing.capnp_as.is_some() {
        return Err(syn::Error::new(
            field.span(),
            "capnp_as is generated automatically inside capnp_models",
        ));
    }
    field.attrs.push(parse_quote!(#[model(capnp_as=#resolved)]));
    if hints.data && !existing.capnp_data {
        field.attrs.push(parse_quote!(#[model(capnp_data)]));
    }
    if hints.text && !existing.capnp_text {
        field.attrs.push(parse_quote!(#[model(capnp_text)]));
    }
    if hints.enumeration && !existing.capnp_enum {
        field.attrs.push(parse_quote!(#[model(capnp_enum)]));
    }
    Ok(())
}
fn configure(
    input: &mut DeriveInput,
    scope: &[String],
    registry: &Registry,
    rd: &Tokens,
) -> syn::Result<()> {
    ensure_derive(input, rd);
    let container = attrs::container(&input.attrs)?;
    if container.capnp.is_some() || container.capnp_enum.is_some() {
        return Err(syn::Error::new(input.span(),"capnp_models generates bindings automatically; remove the manual capnp/capnp_enum path"));
    }
    let mut key = scope.to_vec();
    key.push(input.ident.to_string().trim_start_matches("r#").to_owned());
    let model = &registry.models[&key.join("::")];
    let root = relative_root(scope);
    let module = syn::Ident::new(&model.name.to_snake_case(), Span::call_site());
    if model.unit_enum {
        let native = syn::Ident::new(&model.name, Span::call_site());
        let path = quote!(#root::wire::#native).to_string();
        input.attrs.push(parse_quote!(#[model(capnp_enum=#path)]));
    } else {
        let path = quote!(#root::wire::#module).to_string();
        input.attrs.push(parse_quote!(#[model(capnp=#path)]));
    }
    let configure_fields = |fields: &mut syn::Fields| -> syn::Result<()> {
        let count = fields.len();
        let tuple = matches!(fields, syn::Fields::Unnamed(_));
        for (index, field) in fields.iter_mut().enumerate() {
            if tuple && attrs::field(&field.attrs)?.capnp_skip {
                return Err(syn::Error::new(
                    field.span(),
                    "capnp_skip requires named fields",
                ));
            }
            add_field_metadata(field, index, count, scope, registry, rd)?;
        }
        Ok(())
    };
    match &mut input.data {
        Data::Struct(data) => configure_fields(&mut data.fields)?,
        Data::Enum(data) => {
            for variant in &mut data.variants {
                configure_fields(&mut variant.fields)?;
                let native =
                    attrs::native_variant_name(&variant.attrs, &variant.ident.to_string())?;
                let fixed = protocol_name(&native, false);
                set_native_name(&mut variant.attrs, &fixed)?;
            }
        }
        Data::Union(_) => unreachable!(),
    }
    Ok(())
}
fn rewrite_models(
    items: &mut Vec<Item>,
    scope: &[String],
    registry: &Registry,
    rd: &Tokens,
) -> syn::Result<()> {
    let mut additions = vec![];
    for item in items.iter_mut() {
        if let Some(mut input) = model_input(item) {
            configure(&mut input, scope, registry, rd)?;
            let name = &input.ident;
            let root = relative_root(scope);
            let key = if scope.is_empty() {
                name.to_string()
            } else {
                format!("{}::{name}", scope.join("::"))
            };
            let model = &registry.models[&key];
            let root_type = if model.unit_enum {
                format!("Rydantic{}Root", model.name)
            } else {
                model.name.clone()
            };
            let cfgs = input
                .attrs
                .iter()
                .filter(|a| a.path().is_ident("cfg") || a.path().is_ident("cfg_attr"))
                .collect::<Vec<_>>();
            additions.push(syn::parse2(quote!{
                #(#cfgs)* impl #rd::CapnpSchema for #name{const CAPNP_SCHEMA:&'static str=#root::CAPNP_SCHEMA;const CAPNP_TYPE:&'static str=#root_type;}
            })?);
            if model.unit_enum {
                let wire = syn::Ident::new(&root_type.to_snake_case(), Span::call_site());
                additions.push(syn::parse2(quote!{
                    #(#cfgs)* impl #rd::CapnpModel for #name {
                        type Wire=#root::wire::#wire::Owned;
                        fn read_capnp(reader:<Self::Wire as #rd::capnp::traits::Owned>::Reader<'_>,ctx:&mut #rd::Context)->Option<Self>{
                            let value=#rd::capnp_support::read_result(reader.get_value(),ctx)?;<Self as #rd::CapnpEnum>::read_enum(value,ctx)
                        }
                        fn write_capnp(&self,mut builder:<Self::Wire as #rd::capnp::traits::Owned>::Builder<'_>,_:&mut #rd::Context){builder.set_value(<Self as #rd::CapnpEnum>::write_enum(self));}
                    }
                })?);
            }
            *item = syn::parse2(input.to_token_stream())?;
        } else if let Item::Mod(module) = item {
            if let Some((_, children)) = &mut module.content {
                let mut scope = scope.to_vec();
                scope.push(module.ident.to_string());
                rewrite_models(children, &scope, registry, rd)?;
            }
        }
    }
    items.extend(additions);
    Ok(())
}

/// Rebase references in official bindings so expansion works at any Rust module depth.
struct Rebase {
    depth: usize,
}
impl VisitMut for Rebase {
    fn visit_item_mod_mut(&mut self, module: &mut ItemMod) {
        self.depth += 1;
        syn::visit_mut::visit_item_mod_mut(self, module);
        self.depth -= 1;
    }
    fn visit_path_mut(&mut self, path: &mut Path) {
        if path.segments.len() >= 2
            && path.segments[0].ident == "crate"
            && path.segments[1].ident == "rydantic_capnp"
        {
            let mut segments = syn::punctuated::Punctuated::new();
            if self.depth == 0 {
                segments.push(parse_quote!(self));
            } else {
                for _ in 0..self.depth {
                    segments.push(parse_quote!(super));
                }
            }
            for segment in path.segments.iter().skip(2) {
                segments.push(segment.clone());
            }
            path.leading_colon = None;
            path.segments = segments;
        }
        syn::visit_mut::visit_path_mut(self, path);
    }
}
fn compile(schema: &str, rd: &Tokens, span: Span) -> syn::Result<Vec<Item>> {
    let temp = tempfile::Builder::new()
        .prefix("rydantic-capnp-")
        .tempdir()
        .map_err(|e| syn::Error::new(span, e))?;
    std::fs::write(temp.path().join("rydantic.capnp"), schema)
        .map_err(|e| syn::Error::new(span, e))?;
    let compiler = std::env::var_os("RYDANTIC_CAPNP_COMPILER").unwrap_or_else(|| "capnp".into());
    let result=Command::new(compiler).current_dir(temp.path()).args(["compile","-o-","rydantic.capnp"]).output().map_err(|e|syn::Error::new(span,format!("cannot execute the official capnp compiler: {e}; install capnproto or set RYDANTIC_CAPNP_COMPILER")))?;
    if !result.status.success() {
        return Err(syn::Error::new(
            span,
            format!(
                "generated Cap'n Proto schema did not compile: {}",
                String::from_utf8_lossy(&result.stderr)
            ),
        ));
    }
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(temp.path())
        .capnp_root(&format!("{rd}::capnp"))
        .run(Cursor::new(result.stdout))
        .map_err(|e| {
            syn::Error::new(span, format!("native Rust binding generation failed: {e}"))
        })?;
    let generated = std::fs::read_to_string(temp.path().join("rydantic_capnp.rs"))
        .map_err(|e| syn::Error::new(span, e))?;
    let mut file: syn::File = syn::parse_str(&generated)?;
    Rebase { depth: 0 }.visit_file_mut(&mut file);
    Ok(file.items)
}
pub(super) fn expand(args: Tokens, input: Tokens) -> syn::Result<Tokens> {
    let options = options(args)?;
    let mut module: ItemMod = syn::parse2(input)?;
    let span = module.span();
    let ident = module.ident.clone();
    let Some((_, items)) = &mut module.content else {
        return Err(syn::Error::new(
            span,
            "capnp_models requires an inline module containing the model definitions",
        ));
    };
    for item in items.iter() {
        let reserved = match item {
            Item::Mod(i) => i.ident == "wire",
            Item::Const(i) => matches!(
                i.ident.to_string().as_str(),
                "CAPNP_SCHEMA" | "CAPNP_SCHEMA_ID"
            ),
            _ => false,
        };
        if reserved {
            return Err(syn::Error::new(
                item.span(),
                "wire, CAPNP_SCHEMA and CAPNP_SCHEMA_ID are generated names",
            ));
        }
    }
    let registry = Registry::collect(items, &ident.to_string())?;
    if registry.models.is_empty() {
        return Err(syn::Error::new(
            span,
            "capnp_models requires at least one struct or enum",
        ));
    }
    let namespace = options.namespace.unwrap_or_else(|| {
        format!(
            "{}::{ident}",
            std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "rydantic".into())
        )
    });
    let id = options.id.unwrap_or_else(|| stable_id(&namespace));
    let schema = protocol::generate(&registry, id)?;
    let rd = root_path()?;
    let bindings = compile(&schema, &rd, span)?;
    rewrite_models(items, &[], &registry, &rd)?;
    items.push(parse_quote!(pub const CAPNP_SCHEMA:&str=#schema;));
    items.push(parse_quote!(pub const CAPNP_SCHEMA_ID:u64=#id;));
    items.push(syn::parse2(
        quote! {#[allow(dead_code,unused_imports)] pub mod wire{#(#bindings)*}},
    )?);
    Ok(quote!(#module))
}
