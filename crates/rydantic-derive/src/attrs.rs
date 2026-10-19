use std::collections::HashSet;
use syn::{spanned::Spanned, Attribute, Expr, Lit, LitInt, LitStr, Path};

#[derive(Default)]
pub struct Container {
    pub ignore_extra: bool,
    pub allow_extra: bool,
    pub untagged: bool,
    pub async_validate: Option<Path>,
    pub title: Option<String>,
    pub validate: Option<Path>,
    pub tag: Option<String>,
    pub capnp: Option<Path>,
    pub capnp_enum: Option<Path>,
}

#[derive(Default)]
pub struct Field {
    pub rename: Option<String>,
    pub validation_alias: Option<String>,
    pub serialization_alias: Option<String>,
    pub description: Option<String>,
    pub trim: bool,
    pub lowercase: bool,
    pub uppercase: bool,
    pub coerce: bool,
    pub email: bool,
    pub required: bool,
    pub exclude: bool,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub ge: Option<String>,
    pub gt: Option<String>,
    pub le: Option<String>,
    pub lt: Option<String>,
    pub pattern: Option<String>,
    pub default_trait: bool,
    pub default: Option<Expr>,
    pub default_factory: Option<Path>,
    pub validate: Option<Path>,
    pub before: Option<Path>,
    pub async_validate: Option<Path>,
    pub flatten: bool,
    pub extra: bool,
    pub capnp_name: Option<String>,
    pub capnp_data: bool,
    pub capnp_text: bool,
    pub capnp_enum: bool,
    pub capnp_skip: bool,
}

fn key(meta: &syn::meta::ParseNestedMeta<'_>, seen: &mut HashSet<String>) -> syn::Result<String> {
    let key = meta
        .path
        .get_ident()
        .ok_or_else(|| meta.error("expected a Rydantic attribute name"))?
        .to_string();
    if !seen.insert(key.clone()) {
        return Err(meta.error(format!("duplicate Rydantic attribute `{key}`")));
    }
    Ok(key)
}

pub fn container(attrs: &[Attribute]) -> syn::Result<Container> {
    let mut result = Container::default();
    let mut seen = HashSet::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("model")) {
        attr.parse_nested_meta(|meta| {
            match key(&meta, &mut seen)?.as_str() {
                "extra" => {
                    let value: LitStr = meta.value()?.parse()?;
                    result.ignore_extra = match value.value().as_str() {
                        "ignore" => true,
                        "forbid" => false,
                        "allow" => {
                            result.allow_extra = true;
                            false
                        }
                        _ => {
                            return Err(syn::Error::new(
                                value.span(),
                                "extra must be \"forbid\", \"ignore\", or \"allow\"",
                            ))
                        }
                    };
                }
                "title" => result.title = Some(meta.value()?.parse::<LitStr>()?.value()),
                "validate" => result.validate = Some(meta.value()?.parse()?),
                "tag" => result.tag = Some(meta.value()?.parse::<LitStr>()?.value()),
                "capnp" => result.capnp = Some(meta.value()?.parse::<LitStr>()?.parse()?),
                "capnp_enum" => result.capnp_enum = Some(meta.value()?.parse::<LitStr>()?.parse()?),
                "untagged" => result.untagged = true,
                "async_validate" => result.async_validate = Some(meta.value()?.parse()?),
                _ => {
                    return Err(meta.error(
                        "unsupported model attribute; expected extra, title, validate, or tag",
                    ))
                }
            }
            Ok(())
        })?;
    }
    if result.untagged && result.tag.is_some() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "untagged and tag cannot be combined",
        ));
    }
    Ok(result)
}

fn number(expr: Expr) -> syn::Result<String> {
    fn literal(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Lit(value) => match &value.lit {
                Lit::Int(value) => Some(value.base10_digits().to_owned()),
                Lit::Float(value) => Some(value.base10_digits().to_owned()),
                Lit::Str(value) => Some(value.value()),
                _ => None,
            },
            Expr::Unary(value) if matches!(value.op, syn::UnOp::Neg(_)) => {
                literal(&value.expr).map(|value| format!("-{value}"))
            }
            _ => None,
        }
    }
    let value = literal(&expr).ok_or_else(|| {
        syn::Error::new(
            expr.span(),
            "numeric constraint must be a finite numeric literal",
        )
    })?;
    value.parse::<serde_json::Number>().map_err(|_| {
        syn::Error::new(
            expr.span(),
            "numeric constraint must be representable in JSON",
        )
    })?;
    Ok(value)
}

pub fn field(attrs: &[Attribute]) -> syn::Result<Field> {
    let mut result = Field::default();
    let mut seen = HashSet::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("model")) {
        attr.parse_nested_meta(|meta| {
            match key(&meta, &mut seen)?.as_str() {
                "rename" => result.rename = Some(meta.value()?.parse::<LitStr>()?.value()),
                "validation_alias" => {
                    result.validation_alias = Some(meta.value()?.parse::<LitStr>()?.value())
                }
                "serialization_alias" => {
                    result.serialization_alias = Some(meta.value()?.parse::<LitStr>()?.value())
                }
                "description" => {
                    result.description = Some(meta.value()?.parse::<LitStr>()?.value())
                }
                "trim" => result.trim = true,
                "lowercase" => result.lowercase = true,
                "uppercase" => result.uppercase = true,
                "coerce" => result.coerce = true,
                "email" => result.email = true,
                "required" => result.required = true,
                "exclude" => result.exclude = true,
                "min_length" => {
                    result.min_length = Some(meta.value()?.parse::<LitInt>()?.base10_parse()?)
                }
                "max_length" => {
                    result.max_length = Some(meta.value()?.parse::<LitInt>()?.base10_parse()?)
                }
                "ge" => result.ge = Some(number(meta.value()?.parse()?)?),
                "gt" => result.gt = Some(number(meta.value()?.parse()?)?),
                "le" => result.le = Some(number(meta.value()?.parse()?)?),
                "lt" => result.lt = Some(number(meta.value()?.parse()?)?),
                "pattern" => {
                    let pattern: LitStr = meta.value()?.parse()?;
                    regex::Regex::new(&pattern.value()).map_err(|error| {
                        syn::Error::new(
                            pattern.span(),
                            format!("invalid regular expression: {error}"),
                        )
                    })?;
                    result.pattern = Some(pattern.value());
                }
                "default" => {
                    if meta.input.peek(syn::Token![=]) {
                        result.default = Some(meta.value()?.parse()?);
                    } else {
                        result.default_trait = true;
                    }
                }
                "default_factory" => result.default_factory = Some(meta.value()?.parse()?),
                "validate" => result.validate = Some(meta.value()?.parse()?),
                "before" => result.before = Some(meta.value()?.parse()?),
                "async_validate" => result.async_validate = Some(meta.value()?.parse()?),
                "capnp_name" => result.capnp_name = Some(meta.value()?.parse::<LitStr>()?.value()),
                "capnp_data" => result.capnp_data = true,
                "capnp_text" => result.capnp_text = true,
                "capnp_enum" => result.capnp_enum = true,
                "capnp_skip" => result.capnp_skip = true,
                "flatten" => result.flatten = true,
                "extra" => result.extra = true,
                _ => return Err(meta.error("unsupported Rydantic field attribute")),
            }
            Ok(())
        })?;
    }
    if result.flatten && result.extra {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "flatten and extra cannot be combined",
        ));
    }
    if result.lowercase && result.uppercase {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "lowercase and uppercase cannot be combined",
        ));
    }
    if result
        .min_length
        .zip(result.max_length)
        .is_some_and(|(min, max)| min > max)
    {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "min_length must be <= max_length",
        ));
    }
    if result.has_default() && result.required {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "required cannot be combined with a default",
        ));
    }
    if (result.default_trait || result.default.is_some()) && result.default_factory.is_some() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "default and default_factory cannot be combined",
        ));
    }
    Ok(result)
}

impl Field {
    pub fn has_rules(&self) -> bool {
        self.trim
            || self.lowercase
            || self.uppercase
            || self.email
            || self.pattern.is_some()
            || self.min_length.is_some()
            || self.max_length.is_some()
            || self.ge.is_some()
            || self.gt.is_some()
            || self.le.is_some()
            || self.lt.is_some()
    }
    pub fn has_default(&self) -> bool {
        self.default_trait || self.default.is_some() || self.default_factory.is_some()
    }
}

pub fn variant_name(attrs: &[Attribute], fallback: &str) -> syn::Result<String> {
    let mut name = fallback.to_owned();
    let mut seen = HashSet::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("model")) {
        attr.parse_nested_meta(|meta| {
            if key(&meta, &mut seen)? != "rename" {
                return Err(meta.error("enum variants support only rename"));
            }
            name = meta.value()?.parse::<LitStr>()?.value();
            Ok(())
        })?;
    }
    Ok(name)
}
