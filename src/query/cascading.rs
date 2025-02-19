use async_graphql::dynamic::{ObjectAccessor, ValueAccessor};
use async_graphql::Value;
use sea_orm::{Condition, EntityTrait, Iterable};

use crate::BuilderContext;
use crate::CascadeTypesMapHelper;
use crate::EntityObjectBuilder;
pub fn get_cascade_conditions<T>(
    context: &'static BuilderContext,
    cascades: Option<ValueAccessor>,
) -> Condition
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    // let v = extract_column_names(cascades);
    // dbg!(&v);
    // recursive_prepare_condition::<T>(context, cascades.unwrap().object().unwrap());
    // Condition::all()
    if let Some(cascades) = cascades {
        let cascades = cascades.object().unwrap();

        recursive_prepare_condition::<T>(context, cascades)
    } else {
        Condition::all()
    }
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

// used to prepare recursively the query cascading condition

fn recursive_prepare_condition<T>(
    context: &'static BuilderContext,
    filters: ObjectAccessor,
) -> Condition
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    let entity_object_builder = EntityObjectBuilder { context };
    let cascade_types_map_helper = CascadeTypesMapHelper { context };
    let condition = T::Column::iter().fold(Condition::all(), |condition, column: T::Column| {
        dbg!(&column);
        let column_name = entity_object_builder.column_name::<T>(&column);
        let filter = filters.get(&column_name);
        let filter = filters.get("fields");

        dbg!(&filter.is_some());
        if let Some(filter) = filter {
            cascade_types_map_helper
                .prepare_column_condition::<T>(condition, &column)
                .unwrap()
        } else {
            condition
        }
    });
    dbg!(&condition);

    condition
}