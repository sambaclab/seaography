use async_graphql::dynamic::{ListAccessor, ObjectAccessor, ValueAccessor};
use sea_orm::{Condition, EntityTrait, Iden, Iterable};

use crate::BuilderContext;
use crate::CascadeTypesMapHelper;
pub fn get_cascade_conditions<T>(
    context: &'static BuilderContext,
    cascades: Option<ValueAccessor>,
    selected_items: Vec<&str>,
) -> Condition
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    if let Some(cascades) = cascades {
        let cascades = cascades.object().unwrap();

        recursive_prepare_condition::<T>(context, cascades, selected_items)
    } else {
        Condition::all()
    }
}

// used to prepare recursively the query cascading condition
// Todo: refactor this function to use the new helper functions
fn recursive_prepare_condition<T>(
    context: &'static BuilderContext,
    filters: ObjectAccessor,
    selected_items: Vec<&str>,
) -> Condition
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    let cascade_types_map_helper = CascadeTypesMapHelper { context };
    T::Column::iter().fold(Condition::all(), |condition, column: T::Column| {
        let filter = filters.get("fields");
        match filter {
            Some(ref value) => {
                let value = value.list().unwrap();
                if value.is_empty() {
                    let is_part = filter_query_fields_conditions::<T>(&selected_items, &column);
                    if is_part {
                        return cascade_types_map_helper
                            .prepare_column_condition::<T>(condition, &column)
                            .unwrap();
                    } else {
                        return condition;
                    }
                }
                if filters_cascada_conditions::<T>(value, &column) {
                    cascade_types_map_helper
                        .prepare_column_condition::<T>(condition, &column)
                        .unwrap()
                } else {
                    condition
                }
            }
            _ => cascade_types_map_helper
                .prepare_column_condition::<T>(condition, &column)
                .unwrap(),
        }
    })
}

fn filters_cascada_conditions<T>(filters: ListAccessor<'_>, column: &T::Column) -> bool
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    filters.as_values_slice().iter().any(|v| {
        let value_str = v
            .clone()
            .into_value()
            .to_string()
            .trim_matches('"')
            .to_string();
        column.to_string() == value_str
    })
}

fn filter_query_fields_conditions<T>(selected_items: &Vec<&str>, column: &T::Column) -> bool
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    selected_items.iter().any(|v| {
        let value_str = v.to_string();
        column.to_string() == value_str
    })
}
