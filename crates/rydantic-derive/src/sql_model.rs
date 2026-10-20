use crate::{attrs, field_value, fields, Mode};
use heck::ToSnakeCase;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote, quote_spanned};
use std::collections::HashSet;
use syn::{spanned::Spanned, Data, DeriveInput, Fields, LitInt, LitStr};

#[derive(Default)]
struct Options {
    name: Option<String>,
    primary_key: bool,
    auto_increment: bool,
    unique: bool,
    index: bool,
    skip: bool,
    references: Option<(String, String)>,
    on_delete: Option<String>,
    length: Option<usize>,
    rename_from: Option<String>,
    default: Option<syn::Expr>,
}
fn identifier(value: &str, span: Span) -> syn::Result<()> {
    let mut chars = value.chars();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(syn::Error::new(span,"SQL identifiers must contain ASCII letters, digits or underscores and start with a letter or underscore"));
    }
    Ok(())
}
fn options(attributes: &[syn::Attribute], container: bool) -> syn::Result<Options> {
    let mut result = Options::default();
    let mut seen = HashSet::new();
    for attr in attributes.iter().filter(|a| a.path().is_ident("sql")) {
        attr.parse_nested_meta(|meta| {
            let name = meta
                .path
                .get_ident()
                .ok_or_else(|| meta.error("expected a SQL attribute name"))?
                .to_string();
            if !seen.insert(name.clone()) {
                return Err(meta.error("duplicate SQL attribute"));
            }
            if container {
                if name != "table" {
                    return Err(meta.error("SQL model attributes support table = \"...\""));
                }
                let value = meta.value()?.parse::<LitStr>()?;
                identifier(&value.value(), value.span())?;
                result.name = Some(value.value());
                return Ok(());
            }
            match name.as_str() {
                "rename" => {
                    let value = meta.value()?.parse::<LitStr>()?;
                    identifier(&value.value(), value.span())?;
                    result.name = Some(value.value());
                }
                "rename_from" => {
                    let value = meta.value()?.parse::<LitStr>()?;
                    identifier(&value.value(), value.span())?;
                    result.rename_from = Some(value.value());
                }
                "default" => {
                    let value = meta.value()?.parse::<syn::Expr>()?;
                    sql_default(&value)?;
                    result.default = Some(value);
                }
                "primary_key" => result.primary_key = true,
                "auto_increment" => result.auto_increment = true,
                "unique" => result.unique = true,
                "index" => result.index = true,
                "skip" => result.skip = true,
                "length" => {
                    let value = meta.value()?.parse::<LitInt>()?.base10_parse::<usize>()?;
                    if value == 0 {
                        return Err(meta.error("SQL length must be greater than zero"));
                    }
                    result.length = Some(value);
                }
                "references" => {
                    let value = meta.value()?.parse::<LitStr>()?;
                    let text = value.value();
                    let Some((table, column)) = text.split_once('.') else {
                        return Err(meta.error("references requires table.column"));
                    };
                    identifier(table, value.span())?;
                    identifier(column, value.span())?;
                    result.references = Some((table.into(), column.into()));
                }
                "on_delete" => {
                    let value = meta.value()?.parse::<LitStr>()?;
                    if !matches!(
                        value.value().as_str(),
                        "no_action" | "restrict" | "cascade" | "set_null"
                    ) {
                        return Err(meta
                            .error("on_delete supports no_action, restrict, cascade or set_null"));
                    }
                    result.on_delete = Some(value.value());
                }
                _ => return Err(meta.error("unsupported SQL field attribute")),
            }
            Ok(())
        })?;
    }
    if result.auto_increment && !result.primary_key {
        return Err(syn::Error::new(
            Span::call_site(),
            "auto_increment requires primary_key",
        ));
    }
    if result.on_delete.is_some() && result.references.is_none() {
        return Err(syn::Error::new(
            Span::call_site(),
            "on_delete requires references",
        ));
    }
    if result.skip
        && (result.primary_key
            || result.auto_increment
            || result.unique
            || result.index
            || result.references.is_some()
            || result.length.is_some()
            || result.rename_from.is_some()
            || result.default.is_some()
            || result.name.is_some())
    {
        return Err(syn::Error::new(
            Span::call_site(),
            "skipped SQL fields cannot declare database column attributes",
        ));
    }
    Ok(result)
}

fn sql_default(expr: &syn::Expr) -> syn::Result<(&'static str, Tokens)> {
    use syn::{Expr, Lit, UnOp};
    let (literal, negative) = match expr {
        Expr::Unary(value) if matches!(value.op, UnOp::Neg(_)) => (&*value.expr, true),
        other => (other, false),
    };
    let Expr::Lit(value) = literal else {
        return Err(syn::Error::new_spanned(
            expr,
            "SQL default requires a string, boolean, integer or finite float literal",
        ));
    };
    let kind = match &value.lit {
        Lit::Str(value) if !negative && !value.value().contains('\0') => "Text",
        Lit::Bool(_) if !negative => "Boolean",
        Lit::Int(value) => {
            let text = format!(
                "{}{}",
                if negative { "-" } else { "" },
                value.base10_digits()
            );
            text.parse::<i64>()
                .map_err(|_| syn::Error::new_spanned(expr, "SQL integer default must fit i64"))?;
            "Integer"
        }
        Lit::Float(value) if value.base10_parse::<f64>()?.is_finite() => "Real",
        _ => return Err(syn::Error::new_spanned(
            expr,
            "SQL default requires a string without NUL, boolean, integer or finite float literal",
        )),
    };
    Ok((kind, quote!(#expr)))
}

pub(super) fn expand(input: DeriveInput) -> syn::Result<Tokens> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "SqlModel requires a concrete owned struct",
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.span(),
            "SqlModel requires a struct with named fields",
        ));
    };
    if !matches!(data.fields, Fields::Named(_)) {
        return Err(syn::Error::new(
            data.fields.span(),
            "SqlModel requires named fields",
        ));
    }
    let model_options = options(&input.attrs, true)?;
    let name = &input.ident;
    let table = model_options
        .name
        .unwrap_or_else(|| name.to_string().trim_start_matches("r#").to_snake_case());
    identifier(&table, name.span())?;
    let rd = match proc_macro_crate::crate_name("rydantic")
        .map_err(|e| syn::Error::new(input.span(), e))?
    {
        proc_macro_crate::FoundCrate::Itself => quote!(::rydantic),
        proc_macro_crate::FoundCrate::Name(name) => {
            let id = format_ident!("{name}");
            quote!(::#id)
        }
    };
    let container = attrs::container(&input.attrs)?;
    let fields = fields(&data.fields, None, &container)?;
    let mut columns = Vec::new();
    let mut reads = Vec::new();
    let mut bindings = Vec::new();
    let mut included = Vec::new();
    let mut checks = Vec::new();
    let mut names = HashSet::new();
    let mut primary = None;
    for (field, original) in fields.iter().zip(data.fields.iter()) {
        let options = options(&original.attrs, false)?;
        let ident = &field.ident;
        let ty = &field.ty;
        if options.skip {
            let location = &field.input;
            let value = field_value(
                field,
                quote!(::core::option::Option::<#rd::Input<'_>>::None),
                &Mode {
                    rd: &rd,
                    borrow: false,
                },
            );
            // field_value emits mixed-site context references. Its enclosing
            // bindings must share that hygiene and must not shadow callbacks.
            reads.push(quote_spanned!(Span::mixed_site()=> #ident:{
                let mut ctx=#rd::Context::new();
                let value=ctx.at(#location,|__rydantic_ctx|{#value});
                ctx.finish(value).map_err(|error|#rd::sqlx::Error::Decode(::std::boxed::Box::new(error)))?
            }));
            continue;
        }
        let column = options
            .name
            .unwrap_or_else(|| ident.to_string().trim_start_matches("r#").to_owned());
        identifier(&column, ident.span())?;
        if !names.insert(column.to_ascii_lowercase()) {
            return Err(syn::Error::new(ident.span(), "duplicate SQL column name"));
        }
        let index = columns.len();
        let pk = options.primary_key;
        let auto = options.auto_increment;
        let unique = options.unique;
        let indexed = options.index;
        let rename_from = crate::option(&options.rename_from);
        let default = if let Some(expr) = &options.default {
            if auto {
                return Err(syn::Error::new_spanned(
                    expr,
                    "auto_increment cannot have a SQL default",
                ));
            }
            let (kind, value) = sql_default(expr)?;
            let kind = format_ident!("{kind}");
            checks.push(quote_spanned!(ty.span()=>
                const _: () = assert!(matches!(<#ty as #rd::sql::SqlValue>::SQL_TYPE, #rd::sql::SqlType::#kind),
                    "SQL default must match the column storage type");
            ));
            quote!(::core::option::Option::Some(#rd::sql::SqlDefault::#kind(#value)))
        } else {
            quote!(::core::option::Option::None)
        };
        let length = crate::option(&options.length.or(field.attr.max_length));
        if options.length.is_some() {
            checks.push(quote_spanned!(ty.span()=>
                const _: () = assert!(matches!(<#ty as #rd::sql::SqlValue>::SQL_TYPE, #rd::sql::SqlType::Text | #rd::sql::SqlType::Blob),
                    "SQL length requires a text or binary column");
            ));
        }
        if options.on_delete.as_deref() == Some("set_null") {
            checks.push(quote_spanned!(ty.span()=>
                const _: () = assert!(<#ty as #rd::sql::SqlValue>::NULLABLE && !#pk,
                    "on_delete = set_null requires a nullable non-primary-key column");
            ));
        }
        if pk {
            if primary.is_some() {
                return Err(syn::Error::new(
                    ident.span(),
                    "SqlModel supports one primary key",
                ));
            }
            primary = Some((ident, ty));
        }
        let references = if let Some((table, column)) = options.references {
            let delete = format_ident!(
                "{}",
                match options.on_delete.as_deref().unwrap_or("no_action") {
                    "cascade" => "Cascade",
                    "restrict" => "Restrict",
                    "set_null" => "SetNull",
                    _ => "NoAction",
                }
            );
            quote!(::core::option::Option::Some(#rd::sql::Reference{table:#table,column:#column,on_delete:#rd::sql::OnDelete::#delete}))
        } else {
            quote!(::core::option::Option::None)
        };
        columns.push(quote!(#rd::sql::Column{name:#column,sql_type:<#ty as #rd::sql::SqlValue>::SQL_TYPE,nullable:<#ty as #rd::sql::SqlValue>::NULLABLE && !#pk,primary_key:#pk,auto_increment:#auto,unique:#unique,index:#indexed,references:#references,length:#length,rename_from:#rename_from,default:#default}));
        reads.push(quote!(#ident:<__RydanticDb as #rd::sql::SqlBackend>::read::<#ty>(row,#column).map_err(|error|#rd::sqlx::Error::ColumnDecode{index:#column.to_owned(),source: ::std::boxed::Box::new(error)})?));
        let location = &field.input;
        bindings.push(quote!(#index=>#rd::sql::bind_value::<__RydanticDb,#ty>(&self.#ident,&<Self as #rd::SqlModel>::COLUMNS[#index],query).map_err(|error|error.at(#location))));
        let include = if auto {
            quote!(!<#ty as #rd::sql::AutoKey>::needs_generation(&self.#ident))
        } else {
            quote!(true)
        };
        included.push(quote!(#index=>#include));
    }
    let Some((key, key_ty)) = primary else {
        return Err(syn::Error::new(
            name.span(),
            "SqlModel requires one #[sql(primary_key)] field",
        ));
    };
    let from_row = |db: Tokens, row: Tokens| {
        quote! {
            impl<'__rydantic_row> #rd::sqlx::FromRow<'__rydantic_row,#row> for #name {
                fn from_row(row:&'__rydantic_row #row)->::core::result::Result<Self,#rd::sqlx::Error>{
                    let value=<Self as #rd::sql::SqlRow<#db>>::decode_sql_row(row)?;
                    <Self as #rd::Model>::validate(&value).map_err(|error|#rd::sqlx::Error::Decode(::std::boxed::Box::new(error)))?;
                    Ok(value)
                }
            }
        }
    };
    let sqlite_row = from_row(
        quote!(#rd::sqlx::Sqlite),
        quote!(#rd::sqlx::sqlite::SqliteRow),
    );
    let postgres_row = from_row(
        quote!(#rd::sqlx::Postgres),
        quote!(#rd::sqlx::postgres::PgRow),
    );
    let mysql_row = from_row(quote!(#rd::sqlx::MySql), quote!(#rd::sqlx::mysql::MySqlRow));
    Ok(quote! {
        #(#checks)*
        impl #rd::SqlModel for #name {
            type Key=<#key_ty as #rd::sql::Key>::Value;
            const TABLE:&'static str=#table;
            const COLUMNS:&'static [#rd::sql::Column]=&[#(#columns),*];
            fn key(&self)->::core::option::Option<&Self::Key>{<#key_ty as #rd::sql::Key>::value(&self.#key)}
            fn include_insert_column(&self,index:usize)->bool{match index{#(#included,)* _=>false}}
        }
        impl<__RydanticDb: #rd::sql::SqlBackend> #rd::sql::SqlRow<__RydanticDb> for #name {
            fn decode_sql_row(row:&__RydanticDb::Row)->::core::result::Result<Self,#rd::sqlx::Error>{Ok(Self{#(#reads),*})}
            fn bind_sql_column(&self,index:usize,query:&mut #rd::sqlx::QueryBuilder<__RydanticDb>)->::core::result::Result<(),#rd::SqlError>{match index{#(#bindings,)* _=>unreachable!("generated SQL column index")}}
        }
        #rd::__rydantic_sqlite!{#sqlite_row}
        #rd::__rydantic_postgres!{#postgres_row}
        #rd::__rydantic_mysql!{#mysql_row}
    })
}
