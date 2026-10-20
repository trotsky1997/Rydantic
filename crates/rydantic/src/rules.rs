use crate::{schema::annotate_local, Context, SchemaMode, ValidationError};
use regex::Regex;
use serde_json::{json, Number, Value};
use std::cmp::Ordering;

/// Shared runtime and schema rule descriptor generated from each field declaration.
#[derive(Debug, Default)]
pub struct Rules {
    pub trim: bool,
    pub lowercase: bool,
    pub uppercase: bool,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub ge: Option<Number>,
    pub gt: Option<Number>,
    pub le: Option<Number>,
    pub lt: Option<Number>,
    pub pattern: Option<Regex>,
    pub email: bool,
}

/// Compare JSON decimal representations without casting 64-bit integers to floats.
pub fn compare_numbers(left: &Number, right: &Number) -> Ordering {
    fn parts(number: &Number) -> (bool, Vec<u8>, i64) {
        let text = number.to_string();
        let negative = text.starts_with('-');
        let text = text.trim_start_matches('-');
        let (mantissa, exponent) = text.split_once(['e', 'E']).map_or((text, 0), |(m, e)| {
            (
                m,
                e.parse::<i64>().unwrap_or(if e.starts_with('-') {
                    i64::MIN / 2
                } else {
                    i64::MAX / 2
                }),
            )
        });
        let decimals = mantissa
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len() as i64);
        let digits: Vec<u8> = mantissa
            .bytes()
            .filter(u8::is_ascii_digit)
            .skip_while(|digit| *digit == b'0')
            .collect();
        let magnitude = (digits.len() as i64)
            .saturating_add(exponent)
            .saturating_sub(decimals);
        (negative && !digits.is_empty(), digits, magnitude)
    }
    let (ln, ld, lm) = parts(left);
    let (rn, rd, rm) = parts(right);
    if ld.is_empty() && rd.is_empty() {
        return Ordering::Equal;
    }
    if ln != rn {
        return if ln {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    let order = if ld.is_empty() {
        Ordering::Less
    } else if rd.is_empty() {
        Ordering::Greater
    } else {
        lm.cmp(&rm).then_with(|| {
            (0..ld.len().max(rd.len()))
                .map(|i| {
                    ld.get(i)
                        .copied()
                        .unwrap_or(b'0')
                        .cmp(&rd.get(i).copied().unwrap_or(b'0'))
                })
                .find(|order| *order != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
        })
    };
    if ln {
        order.reverse()
    } else {
        order
    }
}

/// Normalize a mathematical integer without floating point or exponent-sized allocation.
pub fn integer_text(number: &Number) -> Option<String> {
    let text = number.to_string();
    let negative = text.starts_with('-');
    let text = text.trim_start_matches('-');
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text, None), |(m, e)| (m, Some(e)));
    let decimals = mantissa.split_once('.').map_or(0, |(_, f)| f.len() as i64);
    let mut digits: String = mantissa
        .chars()
        .filter(|c| c.is_ascii_digit())
        .skip_while(|c| *c == '0')
        .collect();
    if digits.is_empty() {
        return Some("0".into());
    }
    // Number already guarantees valid JSON syntax. Zero is independent of its
    // exponent, which may exceed i64 even though the result fits every integer.
    let exponent = exponent.map_or(Some(0i64), |e| e.parse::<i64>().ok())?;
    let scale = exponent.checked_sub(decimals)?;
    if scale < 0 {
        let remove = usize::try_from(scale.checked_neg()?).ok()?;
        if remove > digits.len() || !digits[digits.len() - remove..].bytes().all(|b| b == b'0') {
            return None;
        }
        digits.truncate(digits.len() - remove);
    } else {
        let append = usize::try_from(scale).ok()?;
        if digits.len().checked_add(append)? > 39 {
            return None;
        }
        digits.extend(std::iter::repeat_n('0', append));
    }
    if digits.len() > 39 {
        return None;
    }
    if negative {
        digits.insert(0, '-');
    }
    Some(digits)
}

impl Rules {
    pub fn normalizes(&self) -> bool {
        self.trim || self.lowercase || self.uppercase
    }
    pub fn prepare(&self, value: &mut Value) {
        if let Value::String(text) = value {
            if self.trim {
                *text = text.trim().to_owned();
            }
            if self.lowercase {
                *text = text.to_lowercase();
            }
            if self.uppercase {
                *text = text.to_uppercase();
            }
        }
    }
    pub fn validate_length(&self, length: usize, ctx: &mut Context) {
        for (bound, is_min, code) in [
            (self.min_length, true, "too_short"),
            (self.max_length, false, "too_long"),
        ] {
            if let Some(bound) = bound {
                if (is_min && length < bound) || (!is_min && length > bound) {
                    ctx.push(
                        ValidationError::new(
                            code,
                            format!(
                                "length must be {} {bound}",
                                if is_min { ">=" } else { "<=" }
                            ),
                        )
                        .with_param("limit", bound)
                        .with_param("actual", length),
                    );
                }
            }
        }
    }
    pub fn validate(&self, value: &Value, ctx: &mut Context) {
        // Optional fields retain all constraints on Some and accept explicit null.
        if value.is_null() {
            return;
        }
        let length = match value {
            Value::String(text) => Some(text.chars().count()),
            Value::Array(items) => Some(items.len()),
            Value::Object(map) => Some(map.len()),
            _ => None,
        };
        if let Some(length) = length {
            self.validate_length(length, ctx);
        }
        if let Value::Number(number) = value {
            for (bound, operator, code) in [
                (&self.ge, ">=", "greater_than_equal"),
                (&self.gt, ">", "greater_than"),
                (&self.le, "<=", "less_than_equal"),
                (&self.lt, "<", "less_than"),
            ] {
                if let Some(bound) = bound {
                    let order = compare_numbers(number, bound);
                    let valid = match operator {
                        ">=" => order != Ordering::Less,
                        ">" => order == Ordering::Greater,
                        "<=" => order != Ordering::Greater,
                        _ => order == Ordering::Less,
                    };
                    if !valid {
                        ctx.push(
                            ValidationError::new(code, format!("value must be {operator} {bound}"))
                                .with_param("limit", bound.clone()),
                        );
                    }
                }
            }
        }
        if let Value::String(text) = value {
            if let Some(pattern) = &self.pattern {
                if !pattern.is_match(text) {
                    ctx.push(
                        ValidationError::new(
                            "pattern_mismatch",
                            "string does not match the pattern",
                        )
                        .with_param("pattern", pattern.as_str()),
                    );
                }
            }
            if self.email && !email_address::EmailAddress::is_valid(text) {
                ctx.push(ValidationError::new(
                    "email",
                    "string must be a valid email address",
                ));
            }
        }
    }
    /// Input transformations have a structural schema plus explicit local-rule metadata.
    pub fn apply_schema(
        &self,
        schema: &mut Value,
        mode: SchemaMode,
        coerce: bool,
        kind: crate::TypeKind,
    ) {
        if mode == SchemaMode::Input && (self.normalizes() || coerce) {
            if self.trim {
                annotate_local(schema, "trim");
            }
            if self.lowercase {
                annotate_local(schema, "lowercase");
            }
            if self.uppercase {
                annotate_local(schema, "uppercase");
            }
            if coerce {
                annotate_local(schema, "coerce");
            }
            annotate_local(schema, "constraints_after_transform");
            return;
        }
        // Constraints on Option<T> belong to the non-null branch, including optional $refs.
        if let Some(branches) = schema.get_mut("anyOf").and_then(Value::as_array_mut) {
            for branch in branches
                .iter_mut()
                .filter(|branch| branch.get("type") != Some(&json!("null")))
            {
                self.apply_schema(branch, mode, false, kind);
            }
            return;
        }
        let object = schema
            .as_object_mut()
            .expect("JsonType schemas must be objects");
        let (min_key, max_key) = match kind {
            crate::TypeKind::String => ("minLength", "maxLength"),
            crate::TypeKind::Array => ("minItems", "maxItems"),
            crate::TypeKind::Object => ("minProperties", "maxProperties"),
            _ => ("minLength", "maxLength"),
        };
        if let Some(min) = self.min_length {
            if object
                .get(min_key)
                .and_then(Value::as_u64)
                .is_none_or(|old| min as u64 > old)
            {
                object.insert(min_key.into(), json!(min));
            }
        }
        if let Some(max) = self.max_length {
            if object
                .get(max_key)
                .and_then(Value::as_u64)
                .is_none_or(|old| (max as u64) < old)
            {
                object.insert(max_key.into(), json!(max));
            }
        }
        if kind == crate::TypeKind::Decimal {
            if self.ge.is_some() || self.gt.is_some() || self.le.is_some() || self.lt.is_some() {
                annotate_local(schema, "decimal_bounds");
            }
            return;
        }
        for (key, bound) in [
            ("minimum", &self.ge),
            ("exclusiveMinimum", &self.gt),
            ("maximum", &self.le),
            ("exclusiveMaximum", &self.lt),
        ] {
            if let Some(bound) = bound {
                // Keep intrinsic integer bounds when a user supplies a weaker constraint.
                let replace = object
                    .get(key)
                    .and_then(Value::as_number)
                    .is_none_or(|old| {
                        let order = compare_numbers(bound, old);
                        if key.contains("Minimum") || key == "minimum" {
                            order == Ordering::Greater
                        } else {
                            order == Ordering::Less
                        }
                    });
                if replace {
                    object.insert(key.into(), Value::Number(bound.clone()));
                }
            }
        }
        if let Some(pattern) = &self.pattern {
            object.insert("pattern".into(), json!(pattern.as_str()));
        }
        if self.email {
            object.insert("format".into(), json!("email"));
        }
        // Rust regex and email policies have engine-specific edge cases.
        if self.pattern.is_some() {
            annotate_local(schema, "rust_regex");
        }
        if self.email {
            annotate_local(schema, "email_address");
        }
    }
}
