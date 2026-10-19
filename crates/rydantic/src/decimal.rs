use crate::{
    rules::Rules, Context, JsonType, NumericType, SchemaContext, Stage, TypeKind, ValidationError,
    Value,
};
use rust_decimal::Decimal;
use serde_json::json;
impl JsonType for Decimal {
    fn decode(value: Value, ctx: &mut Context, coerce: bool) -> Option<Self> {
        let text = match value {
            Value::String(text) => text,
            Value::Number(number) if coerce => number.to_string(),
            _ => {
                ctx.push(
                    ValidationError::new(
                        "decimal_type",
                        "Decimal requires a decimal string; numeric input requires coerce",
                    )
                    .with_stage(Stage::Parse),
                );
                return None;
            }
        };
        // Exact parser rejects precision loss; JSON number grammar excludes NaN and infinity.
        if text.parse::<serde_json::Number>().is_err() {
            ctx.push(
                ValidationError::new("decimal_parsing", "invalid decimal string")
                    .with_stage(Stage::Parse),
            );
            return None;
        }
        let parsed = if text.contains(['e', 'E']) {
            Decimal::from_scientific(&text)
        } else {
            Decimal::from_str_exact(&text)
        };
        match parsed {
            Ok(value) => {
                // Detect any rounding performed by the scientific parser.
                let original = text.parse::<serde_json::Number>().expect("checked number");
                let encoded = value
                    .to_string()
                    .parse::<serde_json::Number>()
                    .expect("Decimal number");
                if crate::rules::compare_numbers(&original, &encoded) != std::cmp::Ordering::Equal {
                    ctx.push(ValidationError::new(
                        "decimal_precision",
                        "decimal exceeds 28-digit scale or 96-bit coefficient",
                    ));
                    None
                } else {
                    Some(value)
                }
            }
            Err(_) => {
                ctx.push(
                    ValidationError::new(
                        "decimal_parsing",
                        "decimal exceeds supported precision or range",
                    )
                    .with_stage(Stage::Parse),
                );
                None
            }
        }
    }
    fn encode(&self, _: &mut Context) -> Value {
        Value::String(self.to_string())
    }
    fn schema(ctx: &mut SchemaContext, coerce: bool) -> Value {
        let string =
            json!({"type":"string","pattern":"^-?(0|[1-9][0-9]*)(\\.[0-9]+)?([eE][+-]?[0-9]+)?$"});
        let mut schema = if coerce && ctx.mode == crate::SchemaMode::Input {
            json!({"anyOf":[string,{"type":"number"}]})
        } else {
            string
        };
        crate::schema::annotate_local(&mut schema, "decimal_precision");
        schema
    }
    fn validate_rules(&self, rules: &Rules, ctx: &mut Context) {
        let number = self.to_string().parse().expect("Decimal number");
        rules.validate(&Value::Number(number), ctx);
    }
    fn kind() -> TypeKind {
        TypeKind::Decimal
    }
}
impl NumericType for Decimal {}
