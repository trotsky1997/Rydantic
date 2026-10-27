use crate::{Context, Input, Stage, ValidationError, Value};
/// Scalar compatibility adapter; compound raw values are rejected without building a DOM.
pub(crate) fn scalar(input: Input<'_>, ctx: &mut Context, expected: &str) -> Option<Value> {
    match input {
        Input::Owned(value) => Some(value),
        Input::Borrowed(raw) => {
            let actual = match raw.get().as_bytes().first() {
                Some(b'{') => Some("object"),
                Some(b'[') => Some("array"),
                _ => None,
            };
            if let Some(actual) = actual {
                mismatch(ctx, expected, actual);
                None
            } else {
                serde_json::from_str(raw.get())
                    .map_err(|error| ctx.push(crate::borrowed::json_error(error)))
                    .ok()
            }
        }
        Input::BorrowedJson(raw) => {
            let value: Value = serde_json::from_str(raw)
                .map_err(|error| ctx.push(crate::borrowed::json_error(error)))
                .ok()?;
            let actual = match &value {
                Value::Object(_) => Some("object"),
                Value::Array(_) => Some("array"),
                _ => None,
            };
            if let Some(actual) = actual {
                mismatch(ctx, expected, actual);
                None
            } else {
                Some(value)
            }
        }
        Input::Object(_) => {
            mismatch(ctx, expected, "object");
            None
        }
    }
}
fn mismatch(ctx: &mut Context, expected: &str, actual: &str) {
    ctx.push(
        ValidationError::new(
            "type_error",
            format!("expected {expected}, received {actual}"),
        )
        .with_stage(Stage::Parse)
        .with_param("expected", expected)
        .with_param("actual", actual),
    );
}
