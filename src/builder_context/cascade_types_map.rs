use std::collections::{BTreeMap, BTreeSet};

use async_graphql::dynamic::{InputObject, InputValue, TypeRef};
use sea_orm::{ColumnTrait, Condition, EntityTrait};

use crate::{BuilderContext, EntityObjectBuilder, SeaResult};

/// The configuration for FilterTypesMapHelper
pub struct CascadeTypesMapConfig {
    /// used to map entity_name.column_name to a custom filter type
    pub overwrites: BTreeMap<String, Option<CascadeType>>,

    // basic string filter
    pub string_filter_info: CascadeInfo,
}

impl std::default::Default for CascadeTypesMapConfig {
    fn default() -> Self {
        Self {
            overwrites: BTreeMap::default(),
            string_filter_info: CascadeInfo {
                type_name: "StringFilterInput".into(),
                base_type: TypeRef::STRING.into(),
                supported_operations: BTreeSet::from([CascadeOperation::IsNotNull]),
            },
        }
    }
}

/// The FilterTypesMapHelper
/// * provides basic input filter types
/// * provides entity filter object type mappings
/// * helper functions that assist filtering on queries
/// * helper function that generate input filter types
pub struct CascadeTypesMapHelper {
    pub context: &'static BuilderContext,
}

impl CascadeTypesMapHelper {
    /// used to get the GraphQL input value field for a SeaORM entity column
    pub fn get_column_filter_input_value<T>(&self, column: &T::Column) -> Option<InputValue>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let column_name = entity_object_builder.column_name::<T>(column);

        let info = &self.context.cascade_types.string_filter_info;
        Some(InputValue::new(
            column_name,
            TypeRef::named(info.type_name.clone()),
        ))
    }

    /// used to get all basic input filter objects
    pub fn get_input_filters(&self) -> Vec<InputObject> {
        vec![self.generate_cascade_filter(&self.context.cascade_types.string_filter_info)]
    }

    /// used to convert a filter input info struct into input object
    pub fn generate_cascade_filter(&self, filter_info: &CascadeInfo) -> InputObject {
        filter_info.supported_operations.iter().fold(
            InputObject::new(filter_info.type_name.to_string()),
            |object, cur| {
                let field = match cur {
                    CascadeOperation::IsNotNull => InputValue::new(
                        "is_not_null",
                        TypeRef::named(filter_info.base_type.clone()),
                    ),
                };
                object.field(field)
            },
        )
    }

    /// used to parse a filter input object and update the query condition
    pub fn prepare_column_condition<T>(
        &self,
        mut condition: Condition,
        column: &T::Column,
    ) -> SeaResult<Condition>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let filter_info = &self.context.cascade_types.string_filter_info;
        for _operation in filter_info.supported_operations.iter() {
            condition = condition.add(column.is_not_null());
        }

        Ok(condition)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CascadeType {
    String,
}

#[derive(Clone, Debug)]
pub struct CascadeInfo {
    pub type_name: String,
    pub base_type: String,
    pub supported_operations: BTreeSet<CascadeOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CascadeOperation {
    IsNotNull,
}
