use std::collections::{BTreeMap, BTreeSet};

use async_graphql::dynamic::{InputObject, InputValue, ObjectAccessor, TypeRef};
use sea_orm::{ColumnTrait, ColumnType, Condition, EntityTrait};

use crate::{
    active_enum_cascade_input::prepare_enumeration_condition, ActiveEnumFilterInputBuilder,
    BuilderContext, EntityObjectBuilder, SeaResult, TypesMapHelper,
};

type FnCascadeCondition =
    Box<dyn Fn(Condition, &ObjectAccessor) -> SeaResult<Condition> + Send + Sync>;

/// The configuration for FilterTypesMapHelper
pub struct CascadeTypesMapConfig {
    /// used to map entity_name.column_name to a custom filter type
    pub overwrites: BTreeMap<String, Option<CascadeType>>,
    /// used to map entity_name.column_name to a custom condition function
    pub condition_functions: BTreeMap<String, FnCascadeCondition>,

    // basic string filter
    pub string_filter_info: CascadeInfo,
    // basic text filter
    pub text_filter_info: CascadeInfo,
    // basic integer filter
}

impl std::default::Default for CascadeTypesMapConfig {
    fn default() -> Self {
        Self {
            overwrites: BTreeMap::default(),
            condition_functions: BTreeMap::default(),
            string_filter_info: CascadeInfo {
                type_name: "StringFilterInput".into(),
                base_type: TypeRef::STRING.into(),
                supported_operations: BTreeSet::from([CascadeOperation::IsNotNull]),
            },
            text_filter_info: CascadeInfo {
                type_name: "TextFilterInput".into(),
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
    /// used to map sea orm column type to filter type
    pub fn get_column_filter_type<T>(&self, column: &T::Column) -> Option<CascadeType>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };

        let entity_name = entity_object_builder.type_name::<T>();
        let column_name = entity_object_builder.column_name::<T>(column);

        // used to honor overwrites
        if let Some(ty) = self
            .context
            .cascade_types
            .overwrites
            .get(&format!("{entity_name}.{column_name}"))
        {
            return ty.clone();
        }

        // default mappings
        match column.def().get_column_type() {
            _ => None,
        }
    }

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

        let cascade_type: Option<CascadeType> = self.get_column_filter_type::<T>(column);

        match cascade_type {
            Some(cascade_type) => match cascade_type {
                CascadeType::String => {
                    let info = &self.context.filter_types.string_filter_info;
                    Some(InputValue::new(
                        column_name,
                        TypeRef::named(info.type_name.clone()),
                    ))
                }
                CascadeType::Enumeration(name) => {
                    let active_enum_filter_input_builder = ActiveEnumFilterInputBuilder {
                        context: self.context,
                    };

                    Some(InputValue::new(
                        column_name,
                        TypeRef::named(
                            active_enum_filter_input_builder.type_name_from_string(&name),
                        ),
                    ))
                }
                CascadeType::Custom(type_name) => {
                    Some(InputValue::new(column_name, TypeRef::named(type_name)))
                }
            },
            None => None,
        }
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
        filter: &ObjectAccessor,
        column: &T::Column,
    ) -> SeaResult<Condition>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let types_map_helper = TypesMapHelper {
            context: self.context,
        };

        let filter_info = match self.get_column_filter_type::<T>(column) {
            Some(filter_type) => match filter_type {
                CascadeType::String => &self.context.filter_types.string_filter_info,
                CascadeType::Enumeration(_) => {
                    return prepare_enumeration_condition::<T>(filter, column, condition)
                }
                CascadeType::Custom(_) => {
                    let entity_object_builder = EntityObjectBuilder {
                        context: self.context,
                    };

                    let entity_name = entity_object_builder.type_name::<T>();
                    let column_name = entity_object_builder.column_name::<T>(column);

                    if let Some(filter_condition_fn) = self
                        .context
                        .filter_types
                        .condition_functions
                        .get(&format!("{entity_name}.{column_name}"))
                    {
                        return filter_condition_fn(condition, filter);
                    } else {
                        // FIXME: add log warning to console
                        return Ok(condition);
                    }
                }
            },
            None => return Ok(condition),
        };

        for operation in filter_info.supported_operations.iter() {
            if filter.get("is_not_null").is_some() {
                condition = condition.add(column.is_not_null());
            }
        }

        Ok(condition)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CascadeType {
    String,
    Enumeration(String),
    Custom(String),
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
