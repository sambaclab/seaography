use heck::ToLowerCamelCase;
use std::collections::{BTreeMap, HashSet};

use crate::{BuilderContext, EntityObjectBuilder, SeaResult, TypesMapHelper};
use async_graphql::dynamic::{InputObject, InputValue, ObjectAccessor};
use sea_orm::{
    ColumnTrait, EntityTrait, Iden, Iterable, PrimaryKeyToColumn, PrimaryKeyTrait, RelationTrait,
};
use uuid::Uuid;

/// The configuration structure of EntityInputBuilder
pub struct EntityInputConfig {
    /// suffix that is appended on insert input objects
    pub insert_suffix: String,
    /// names of "{entity}.{column}" you want to skip the insert input to be generated
    pub insert_skips: Vec<String>,
    /// suffix that is appended on update input objects
    pub update_suffix: String,
    /// names of "{entity}.{column}" you want to skip the update input to be generated
    pub update_skips: Vec<String>,
    pub add_suffix: String,
    pub add_prefix: String,
    pub add_skips: Vec<String>,
    pub ref_suffix: String,
    pub ref_skips: Vec<String>,
}

impl std::default::Default for EntityInputConfig {
    fn default() -> Self {
        EntityInputConfig {
            insert_suffix: "InsertInput".into(),
            insert_skips: Vec::new(),
            update_suffix: "UpdateInput".into(),
            update_skips: Vec::new(),
            add_suffix: "Input".into(),
            add_prefix: "Add".into(),
            add_skips: Vec::new(),
            ref_suffix: "Ref".into(),
            ref_skips: Vec::new(),
        }
    }
}

/// Used to create the entity create/update input object
pub struct EntityInputBuilder {
    pub context: &'static BuilderContext,
}

impl EntityInputBuilder {
    /// used to get SeaORM entity insert input object name
    pub fn insert_type_name<T>(&self) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let object_name = entity_object_builder.type_name::<T>();
        format!("{}{}", object_name, self.context.entity_input.insert_suffix)
    }

    /// used to get SeaORM entity update input object name
    pub fn update_type_name<T>(&self) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let object_name = entity_object_builder.type_name::<T>();
        format!("{}{}", object_name, self.context.entity_input.update_suffix)
    }

    pub fn add_type_name<T>(&self) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let object_name = entity_object_builder.type_name::<T>();
        format!(
            "{}{}{}",
            self.context.entity_input.add_prefix, object_name, self.context.entity_input.add_suffix
        )
    }

    pub fn ref_type_name<T>(&self) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let object_name = entity_object_builder.type_name::<T>();
        format!("{}{}", object_name, self.context.entity_input.ref_suffix)
    }
    /// used to produce the SeaORM entity input object
    fn input_object<T>(&self, ty: &str) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let name = match ty {
            "insert" => self.insert_type_name::<T>(),
            "update" => self.update_type_name::<T>(),
            "add" => self.add_type_name::<T>(),
            _ => self.ref_type_name::<T>(),
        };

        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let types_map_helper = TypesMapHelper {
            context: self.context,
        };

        let foreign_keys: HashSet<String> = T::Relation::iter()
            .filter_map(|rel| {
                if rel.def().is_owner {
                    None
                } else {
                    let col = rel.def().to_col.to_string().to_lower_camel_case();
                    Some(col)
                }
            })
            .collect();

        T::Column::iter().fold(InputObject::new(name), |object, column| {
            let column_name = entity_object_builder.column_name::<T>(&column);
            if (ty == "add" || ty == "ref") && foreign_keys.contains(&column_name) {
                return object;
            }

            let full_name = format!("{}.{}", entity_object_builder.type_name::<T>(), column_name);

            let skip = if ty == "insert" {
                self.context.entity_input.insert_skips.contains(&full_name)
            } else if ty == "update" {
                self.context.entity_input.update_skips.contains(&full_name)
            } else if ty == "add" {
                self.context.entity_input.add_skips.contains(&full_name)
            } else {
                self.context.entity_input.ref_skips.contains(&full_name)
            };

            if skip {
                return object;
            }

            let column_def = column.def();

            let auto_increment = match <T::PrimaryKey as PrimaryKeyToColumn>::from_column(column) {
                Some(_) => T::PrimaryKey::auto_increment(),
                None => false,
            };
            //let has_default_expr = column_def.get_column_default().is_some();
            let is_insert_not_nullable =
                (ty == "insert") && !(column_def.is_null() || auto_increment);

            let graphql_type = match types_map_helper.sea_orm_column_type_to_graphql_type(
                column_def.get_column_type(),
                is_insert_not_nullable,
            ) {
                Some(type_name) => type_name,
                None => return object,
            };

            object.field(InputValue::new(column_name, graphql_type))
        })
    }

    /// used to produce the SeaORM entity insert input object
    pub fn insert_input_object<T>(&self) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        self.input_object::<T>("insert")
    }

    /// used to produce the SeaORM entity update input object
    pub fn update_input_object<T>(&self) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        self.input_object::<T>("update")
    }

    pub fn add_input_object<T>(&self) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        self.input_object::<T>("add")
    }

    pub fn ref_input_object<T>(&self) -> InputObject
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        self.input_object::<T>("ref")
    }

    pub fn parse_pks<T>(
        &self,
        object: &ObjectAccessor,
    ) -> SeaResult<BTreeMap<String, sea_orm::Value>>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let types_map_helper = TypesMapHelper {
            context: self.context,
        };

        let mut map = BTreeMap::<String, sea_orm::Value>::new();

        for column in T::PrimaryKey::iter() {
            let column_name = entity_object_builder.column_name::<T>(&column.into_column());

            if column_name == "uid" {
                let uid = Uuid::new_v4();
                map.insert(
                    column_name,
                    sea_orm::Value::String(Some(Box::new(uid.to_string()))),
                );
                continue;
            }

            let value = match object.get(&column_name) {
                Some(value) => value,
                None => continue,
            };

            let result = types_map_helper
                .async_graphql_value_to_sea_orm_value::<T>(&column.into_column(), &value)?;

            map.insert(column_name, result);
        }

        Ok(map)
    }
    pub fn parse_object<T>(
        &self,
        object: &ObjectAccessor,
    ) -> SeaResult<BTreeMap<String, sea_orm::Value>>
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        let types_map_helper = TypesMapHelper {
            context: self.context,
        };

        let mut map = BTreeMap::<String, sea_orm::Value>::new();

        for column in T::Column::iter() {
            let column_name = entity_object_builder.column_name::<T>(&column);
            if column_name == "uid" {
                let uid = Uuid::new_v4();
                map.insert(
                    column_name,
                    sea_orm::Value::String(Some(Box::new(uid.to_string()))),
                );
                continue;
            }

            let value = match object.get(&column_name) {
                Some(value) => value,
                None => continue,
            };

            let result =
                types_map_helper.async_graphql_value_to_sea_orm_value::<T>(&column, &value)?;

            map.insert(column_name, result);
        }

        Ok(map)
    }
}
