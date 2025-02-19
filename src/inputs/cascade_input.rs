use async_graphql::dynamic::{InputObject, InputValue, TypeRef, ValueAccessor};
use sea_orm::EntityTrait;

use crate::BuilderContext;

/// The configuration structure for OrderInputBuilder
pub struct CascadeInputConfig {
    /// used to format OrderInput object name
    pub type_name: crate::SimpleNamingFn,
}

impl std::default::Default for CascadeInputConfig {
    fn default() -> Self {
        CascadeInputConfig {
            type_name: Box::new(|object_name: &str| -> String {
                format!("{object_name}CascadeInput")
            }),
        }
    }
}

/// This builder produces the OrderInput object of a SeaORM entity
pub struct CascadeInputBuilder {
    pub context: &'static BuilderContext,
}

impl CascadeInputBuilder {
    /// used to get type name
    pub fn type_name(&self, object_name: &str) -> String {
        self.context.cascade_by_enum.type_name.as_ref()(object_name)
    }

    /// used to get the OrderInput object of a SeaORM entity
    pub fn to_object<T>(&self) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        InputObject::new("cascade").field(InputValue::new(
            "fields",
            TypeRef::named_nn_list(TypeRef::STRING),
        ))
    }

    pub fn parse_object<T>(
        &self,
        _value: Option<ValueAccessor<'_>>,
    ) -> Vec<(T::Column, sea_orm::sea_query::Order)>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        Vec::new()
    }
}
