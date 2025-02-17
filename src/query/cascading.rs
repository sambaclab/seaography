use async_graphql::dynamic::{ObjectAccessor, ValueAccessor};

pub fn get_cascade_conditions(cascades: Option<ValueAccessor>) -> Option<ObjectAccessor> {
    cascades.map(|cascades| cascades.object().unwrap())
}