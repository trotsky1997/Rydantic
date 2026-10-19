use super::{model_input, protocol_name, relative_model};
use proc_macro2::TokenStream as Tokens;
use quote::{quote, ToTokens};
use std::collections::BTreeMap;
use syn::{
    parse_quote, spanned::Spanned, visit_mut::VisitMut, Data, DeriveInput, Expr, GenericArgument,
    Item, PathArguments, Type,
};

pub(super) struct Model {
    pub input: DeriveInput,
    pub scope: Vec<String>,
    pub name: String,
    pub unit_enum: bool,
}
pub(super) struct Registry {
    pub models: BTreeMap<String, Model>,
    aliases: BTreeMap<String, (Vec<String>, syn::ItemType)>,
    imports: BTreeMap<String, Vec<String>>,
    root: String,
}
fn key(scope: &[String], name: &str) -> String {
    scope
        .iter()
        .cloned()
        .chain([name.trim_start_matches("r#").to_owned()])
        .collect::<Vec<_>>()
        .join("::")
}
fn imports(
    tree: &syn::UseTree,
    prefix: &[String],
    scope: &[String],
    map: &mut BTreeMap<String, Vec<String>>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            let mut prefix = prefix.to_vec();
            prefix.push(path.ident.to_string().trim_start_matches("r#").to_owned());
            imports(&path.tree, &prefix, scope, map);
        }
        syn::UseTree::Name(name) => {
            let mut target = prefix.to_vec();
            target.push(name.ident.to_string().trim_start_matches("r#").to_owned());
            map.insert(key(scope, &name.ident.to_string()), target);
        }
        syn::UseTree::Rename(name) => {
            let mut target = prefix.to_vec();
            target.push(name.ident.to_string().trim_start_matches("r#").to_owned());
            map.insert(key(scope, &name.rename.to_string()), target);
        }
        syn::UseTree::Group(group) => {
            for child in &group.items {
                imports(child, prefix, scope, map);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}
impl Registry {
    pub fn collect(items: &[Item], root: &str) -> syn::Result<Self> {
        let mut registry = Self {
            models: BTreeMap::new(),
            aliases: BTreeMap::new(),
            imports: BTreeMap::new(),
            root: root.to_owned(),
        };
        registry.walk(items, &[])?;
        let mut names = BTreeMap::new();
        for (key, model) in &registry.models {
            if let Some(previous) = names.insert(model.name.clone(), key) {
                return Err(syn::Error::new(model.input.span(),format!("native type name collision between {previous} and {key}; use distinct model/module names")));
            }
            if model.unit_enum {
                let wrapper = format!("Rydantic{}Root", model.name);
                if registry.models.values().any(|m| m.name == wrapper) {
                    return Err(syn::Error::new(
                        model.input.span(),
                        "model name collides with an automatically generated enum root",
                    ));
                }
            }
        }
        Ok(registry)
    }
    fn walk(&mut self, items: &[Item], scope: &[String]) -> syn::Result<()> {
        for item in items {
            if let Some(input) = model_input(item) {
                if !input.generics.params.is_empty() {
                    return Err(syn::Error::new(input.generics.span(),"automatic Cap'n Proto models require concrete owned structs/enums; instantiate generic fields through concrete aliases"));
                }
                let unit_enum = matches!(&input.data,Data::Enum(data)if data.variants.iter().all(|v|matches!(v.fields,syn::Fields::Unit)));
                let raw = input.ident.to_string().trim_start_matches("r#").to_owned();
                let name = protocol_name(
                    &scope
                        .iter()
                        .cloned()
                        .chain([raw.clone()])
                        .collect::<Vec<_>>()
                        .join("_"),
                    true,
                );
                self.models.insert(
                    key(scope, &raw),
                    Model {
                        input,
                        scope: scope.to_vec(),
                        name,
                        unit_enum,
                    },
                );
            } else {
                match item {
                    Item::Mod(module) => {
                        if let Some((_, children)) = &module.content {
                            let mut scope = scope.to_vec();
                            scope
                                .push(module.ident.to_string().trim_start_matches("r#").to_owned());
                            self.walk(children, &scope)?;
                        } else {
                            return Err(syn::Error::new(module.span(),"use inline submodules so capnp_models can inspect their model definitions"));
                        }
                    }
                    Item::Type(alias) => {
                        self.aliases.insert(
                            key(scope, &alias.ident.to_string()),
                            (scope.to_vec(), alias.clone()),
                        );
                    }
                    Item::Use(import) => imports(&import.tree, &[], scope, &mut self.imports),
                    Item::Union(union) => {
                        return Err(syn::Error::new(
                            union.span(),
                            "Rust unions cannot be used as automatic data models",
                        ))
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
    fn normalize(&self, names: &[String], scope: &[String]) -> Option<String> {
        if names.is_empty() {
            return None;
        }
        let mut base = scope.to_vec();
        let mut index = 0;
        if names[0] == "crate" {
            index = names.iter().position(|name| name == &self.root)? + 1;
            base.clear();
        } else if names[0] == "self" {
            index = 1;
        } else {
            while names.get(index).is_some_and(|n| n == "super") {
                base.pop()?;
                index += 1;
            }
        }
        base.extend(names[index..].iter().cloned());
        Some(base.join("::"))
    }
    pub fn resolve(
        &self,
        ty: &Type,
        scope: &[String],
        stack: &mut Vec<String>,
    ) -> syn::Result<Spec> {
        if stack.len() > 64 {
            return Err(syn::Error::new(
                ty.span(),
                "type alias expansion exceeds 64 levels",
            ));
        }
        if let Type::Array(array) = ty {
            return Ok(Spec::Array(
                Box::new(self.resolve(&array.elem, scope, stack)?),
                array.len.clone(),
            ));
        }
        let Type::Path(path) = ty else {
            return Err(syn::Error::new(ty.span(),"use an owned scalar, model, Option, Box, Vec, array or string-keyed map for Cap'n Proto"));
        };
        if path.qself.is_some() {
            return Err(syn::Error::new(ty.span(),"associated types require a concrete type alias for automatic Cap'n Proto generation"));
        }
        let names = path
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string().trim_start_matches("r#").to_owned())
            .collect::<Vec<_>>();
        let name = names.last().unwrap();
        let arguments = &path.path.segments.last().unwrap().arguments;
        if !matches!(names[0].as_str(), "self" | "super" | "crate") {
            if let Some(target) = self.imports.get(&key(scope, &names[0])) {
                if target != &names[..1] {
                    let mut full = target.clone();
                    full.extend(names.iter().skip(1).cloned());
                    let mut replacement: syn::TypePath = syn::parse_str(&full.join("::"))?;
                    replacement.path.segments.last_mut().unwrap().arguments = arguments.clone();
                    let marker = format!("use:{}", key(scope, &names[0]));
                    if !stack.contains(&marker) {
                        stack.push(marker);
                        let result = self.resolve(&Type::Path(replacement), scope, stack);
                        stack.pop();
                        return result;
                    }
                }
            }
        }
        let local = self.normalize(&names, scope);
        if let Some((alias_scope, alias)) = local.as_ref().and_then(|k| self.aliases.get(k)) {
            let alias_key = local.as_ref().unwrap();
            if stack.contains(alias_key) {
                return Err(syn::Error::new(ty.span(), "cyclic type alias"));
            }
            let parameters=alias.generics.params.iter().map(|p|match p{syn::GenericParam::Type(p)=>Ok(p.ident.to_string()),_=>Err(syn::Error::new(p.span(),"automatic type aliases support type parameters; use concrete lifetime/const values"))}).collect::<syn::Result<Vec<_>>>()?;
            let supplied = type_args(arguments)?;
            if parameters.len() != supplied.len() {
                return Err(syn::Error::new(
                    ty.span(),
                    "type alias requires all type arguments explicitly",
                ));
            }
            let canonical = supplied
                .iter()
                .map(|ty| {
                    self.resolve(ty, scope, stack)
                        .map(|spec| spec.rust_type(alias_scope, &quote!(::rydantic)))
                })
                .collect::<syn::Result<Vec<_>>>()?;
            let substitutions = parameters
                .into_iter()
                .zip(canonical)
                .collect::<BTreeMap<_, _>>();
            struct Substitute<'a>(&'a BTreeMap<String, Type>);
            impl VisitMut for Substitute<'_> {
                fn visit_type_mut(&mut self, ty: &mut Type) {
                    if let Type::Path(path) = ty {
                        if path.path.segments.len() == 1 {
                            if let Some(value) =
                                self.0.get(&path.path.segments[0].ident.to_string())
                            {
                                *ty = value.clone();
                                return;
                            }
                        }
                    }
                    syn::visit_mut::visit_type_mut(self, ty);
                }
            }
            let mut expanded = (*alias.ty).clone();
            Substitute(&substitutions).visit_type_mut(&mut expanded);
            stack.push(alias_key.clone());
            let result = self.resolve(&expanded, alias_scope, stack);
            stack.pop();
            return result;
        }
        if let Some(model) = local.as_ref().and_then(|k| self.models.get(k)) {
            if !matches!(arguments, PathArguments::None) {
                return Err(syn::Error::new(
                    ty.span(),
                    "generated models have concrete types",
                ));
            }
            let mut path = model.scope.clone();
            path.push(
                model
                    .input
                    .ident
                    .to_string()
                    .trim_start_matches("r#")
                    .to_owned(),
            );
            return Ok(Spec::Model {
                path,
                name: model.name.clone(),
                enumeration: model.unit_enum,
            });
        }
        let args = type_args(arguments)?;
        let arg = |index: usize| {
            args.get(index)
                .copied()
                .ok_or_else(|| syn::Error::new(ty.span(), "missing container type argument"))
        };
        Ok(match name.as_str(){
            "bool"|"i8"|"i16"|"i32"|"i64"|"i128"|"isize"|"u8"|"u16"|"u32"|"u64"|"u128"|"usize"|"f32"|"f64"|"String" if args.is_empty()=>Spec::Scalar(name.clone()),
            "NaiveDate"|"NaiveTime"|"NaiveDateTime"|"Decimal" if args.is_empty()=>Spec::Text(name.clone(),None),
            "DateTime" if args.len()==1=>{let Type::Path(zone)=args[0] else{return Err(syn::Error::new(ty.span(),"DateTime requires Utc or FixedOffset"));};let zone=zone.path.segments.last().unwrap().ident.to_string();if !matches!(zone.as_str(),"Utc"|"FixedOffset"){return Err(syn::Error::new(ty.span(),"DateTime requires Utc or FixedOffset"));}Spec::Text(name.clone(),Some(zone))},
            "Option"|"Box"|"Vec" if args.len()==1=>{
                let value=Box::new(self.resolve(arg(0)?,scope,stack)?);
                match name.as_str(){"Option"=>Spec::Optional(value),"Box"=>Spec::Boxed(value),_=>Spec::List(value)}
            },
            "BTreeMap"|"HashMap" if args.len()==2=>{
                if !matches!(self.resolve(arg(0)?,scope,stack)?,Spec::Scalar(ref n)if n=="String"){return Err(syn::Error::new(ty.span(),"automatic native maps require String keys"));}
                Spec::Map{hash:name=="HashMap",value:Box::new(self.resolve(arg(1)?,scope,stack)?)}
            },
            _=>return Err(syn::Error::new(ty.span(),format!("cannot infer native wire type `{}`; put the model/type alias in this capnp_models module or use capnp_skip with a default",ty.to_token_stream()))),
        })
    }
}
fn type_args(arguments: &PathArguments) -> syn::Result<Vec<&Type>> {
    match arguments {
        PathArguments::None => Ok(vec![]),
        PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .map(|a| match a {
                GenericArgument::Type(ty) => Ok(ty),
                _ => Err(syn::Error::new(
                    a.span(),
                    "use concrete owned type arguments for Cap'n Proto",
                )),
            })
            .collect(),
        _ => Err(syn::Error::new(
            arguments.span(),
            "function-style type parameters are unsupported",
        )),
    }
}
#[derive(Clone)]
pub(super) enum Spec {
    Scalar(String),
    Text(String, Option<String>),
    Model {
        path: Vec<String>,
        name: String,
        enumeration: bool,
    },
    Optional(Box<Spec>),
    Boxed(Box<Spec>),
    List(Box<Spec>),
    Array(Box<Spec>, Expr),
    Map {
        hash: bool,
        value: Box<Spec>,
    },
}
#[derive(Default)]
pub(super) struct Hints {
    pub text: bool,
    pub data: bool,
    pub enumeration: bool,
}
impl Spec {
    pub fn bytes(&self) -> bool {
        matches!(self,Self::List(inner)if matches!(&**inner,Self::Scalar(n)if n=="u8"))
    }
    pub fn hints(&self) -> Hints {
        if self.bytes() {
            return Hints {
                data: true,
                ..Hints::default()
            };
        }
        match self {
            Self::Scalar(n) if matches!(n.as_str(), "i128" | "u128") => Hints {
                text: true,
                ..Hints::default()
            },
            Self::Text(..) => Hints {
                text: true,
                ..Hints::default()
            },
            Self::Model {
                enumeration: true, ..
            } => Hints {
                enumeration: true,
                ..Hints::default()
            },
            Self::Optional(inner)
            | Self::Boxed(inner)
            | Self::List(inner)
            | Self::Array(inner, _) => inner.hints(),
            Self::Map { value, .. } => value.hints(),
            _ => Hints::default(),
        }
    }
    pub fn rust_type(&self, scope: &[String], rd: &Tokens) -> Type {
        match self {
            Self::Scalar(name) => {
                let id = syn::Ident::new(name, proc_macro2::Span::call_site());
                if name == "String" {
                    parse_quote!(::std::string::String)
                } else {
                    parse_quote!(::core::primitive::#id)
                }
            }
            Self::Text(name, zone) => {
                let name = syn::Ident::new(name, proc_macro2::Span::call_site());
                if name == "Decimal" {
                    parse_quote!(#rd::rust_decimal::Decimal)
                } else if let Some(zone) = zone {
                    let zone = syn::Ident::new(zone, proc_macro2::Span::call_site());
                    parse_quote!(#rd::chrono::#name<#rd::chrono::#zone>)
                } else {
                    parse_quote!(#rd::chrono::#name)
                }
            }
            Self::Model { path, .. } => relative_model(path, scope),
            Self::Optional(inner) => {
                let inner = inner.rust_type(scope, rd);
                parse_quote!(::std::option::Option<#inner>)
            }
            Self::Boxed(inner) => {
                let inner = inner.rust_type(scope, rd);
                parse_quote!(::std::boxed::Box<#inner>)
            }
            Self::List(inner) => {
                let inner = inner.rust_type(scope, rd);
                parse_quote!(::std::vec::Vec<#inner>)
            }
            Self::Array(inner, len) => {
                let inner = inner.rust_type(scope, rd);
                parse_quote!([#inner;#len])
            }
            Self::Map { value, hash } => {
                let value = value.rust_type(scope, rd);
                if *hash {
                    parse_quote!(::std::collections::HashMap<::std::string::String,#value>)
                } else {
                    parse_quote!(::std::collections::BTreeMap<::std::string::String,#value>)
                }
            }
        }
    }
}
