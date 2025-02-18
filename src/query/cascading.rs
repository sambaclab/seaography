use async_graphql::dynamic::{ObjectAccessor, ValueAccessor};
use async_graphql::Value;
use sea_orm::{Condition, EntityTrait, Iterable};

use crate::BuilderContext;
pub fn get_cascade_conditions(
    context: &'static BuilderContext,
    cascades: Option<ValueAccessor>,
) -> Condition {
    let v = extract_column_names(cascades);
    dbg!(&v);
    Condition::all()
}

fn extract_column_names(cascades: Option<ValueAccessor>) -> Vec<String> {
    let obj = cascades.map(|cascades| cascades.object().unwrap());
    if let Some(obj) = obj {
        if let Some(x) = obj.values().next() {
            if let Value::List(data) = x.as_value() {
                return data
                    .iter()
                    .map(|x| x.clone().into_value().to_string())
                    .collect::<Vec<_>>();
            }
        }
    }
    Vec::new()
}

