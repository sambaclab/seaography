use async_graphql::dynamic::{Enum, EnumItem};
use sea_orm::{EntityTrait, Iterable};

use crate::{BuilderContext, EntityObjectBuilder};

/// The configuration structure for OrderByEnumBuilder
pub struct CascadeByEnumConfig {
    /// the enumeration name
    pub type_name: crate::SimpleNamingFn,
}

impl std::default::Default for CascadeByEnumConfig {
    fn default() -> Self {
        CascadeByEnumConfig {
            type_name: Box::new(|object_name: &str| -> String {
                format!("{object_name}CascadeEnumInput")
            }),
        }
    }
}

/// The OrderByEnumeration is used for Entities Fields sorting
pub struct CascadeByEnumBuilder {
    pub context: &'static BuilderContext,
}

impl CascadeByEnumBuilder {
    pub fn type_name(&self, object_name: &str) -> String {
        self.context.cascade_by_enum.type_name.as_ref()(object_name)
    }

    /// used to get the GraphQL enumeration config
    pub fn enumeration<T>(&self) -> Enum
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let object_name = entity_object_builder.type_name::<T>();

        T::Column::iter().fold(Enum::new(self.type_name(&object_name)), |enu, column| {
            enu.item(EnumItem::new(
                entity_object_builder.column_name::<T>(&column),
            ))
        })
    }
}
