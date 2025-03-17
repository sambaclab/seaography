use super::EntityObjectBuilder;
use crate::{BuilderContext, GuardAction};
use async_graphql::dynamic::{Field, FieldFuture, FieldValue, Object, TypeRef};
use async_graphql::Error;
use heck::{ToLowerCamelCase, ToSnakeCase, ToUpperCamelCase};
use sea_orm::{EntityName, EntityTrait};

/// The configuration structure for EntityObjectPayloadBuilder
pub struct EntityObjectPayloadConfig {
    /// used to format the type name of the object
    pub type_name: crate::SimpleNamingFn,
    pub field_name: crate::SimpleNamingFn,
}

impl std::default::Default for EntityObjectPayloadConfig {
    fn default() -> Self {
        Self {
            type_name: Box::new(|entity_name: &str| -> String {
                format!("Add{}Payload", entity_name.to_upper_camel_case())
            }),
            field_name: Box::new(|object_name: &str| -> String {
                if cfg!(feature = "field-snake-case") {
                    object_name.to_snake_case()
                } else {
                    object_name.to_lower_camel_case()
                }
            }),
        }
    }
}

/// This builder produces the GraphQL object of a SeaORM entity
pub struct EntityObjectPayloadBuilder {
    pub context: &'static BuilderContext,
}

impl EntityObjectPayloadBuilder {
    /// used to get type name
    pub fn type_name<T>(&self) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let name: String = <T as EntityName>::table_name(&T::default()).into();
        self.context.entity_object_payload.type_name.as_ref()(&name)
    }

    pub fn field_name(&self, name: &str) -> String {
        self.context.entity_object_payload.field_name.as_ref()(name)
    }

    pub fn to_object<T>(&self) -> Object
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let context = self.context;
        let entity_payload_name = self.type_name::<T>();
        let entity_object = EntityObjectBuilder { context };
        let object_name = entity_object.type_name::<T>();
        let guard = self.context.guards.entity_guards.get(&object_name);
        Object::new(entity_payload_name)
            .field(Field::new(
                self.field_name(object_name.as_str()),
                TypeRef::named_list(object_name),
                move |ctx| {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&ctx)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return FieldFuture::new(async move {
                            match reason {
                                Some(reason) => {
                                    Err::<Option<()>, async_graphql::Error>(Error::new(reason))
                                }
                                None => Err::<Option<()>, async_graphql::Error>(Error::new(
                                    "Field guard triggered.",
                                )),
                            }
                        });
                    }

                    let object = ctx
                        .parent_value
                        .try_downcast_ref::<(usize, Vec<<T as EntityTrait>::Model>)>()
                        .expect("Something went wrong when trying to downcast entity object.")
                        .1
                        .to_owned();
                    FieldFuture::new(async move {
                        Ok(Some(FieldValue::list(
                            object.into_iter().map(FieldValue::owned_any),
                        )))
                    })
                },
            ))
            .field(Field::new(
                self.field_name("num_uids"),
                TypeRef::named(TypeRef::INT),
                move |ctx| {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&ctx)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return FieldFuture::new(async move {
                            match reason {
                                Some(reason) => {
                                    Err::<Option<()>, async_graphql::Error>(Error::new(reason))
                                }
                                None => Err::<Option<()>, async_graphql::Error>(Error::new(
                                    "Field guard triggered.",
                                )),
                            }
                        });
                    }
                    FieldFuture::new(async move {
                        let object = ctx
                            .parent_value
                            .try_downcast_ref::<(usize, Vec<T::Model>)>()
                            .expect("Something went wrong when trying to downcast entity object.");
                        Ok(Some(FieldValue::value(object.0)))
                    })
                },
            ))
    }
}