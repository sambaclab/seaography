use std::collections::{BTreeMap, HashSet};

use async_graphql::dynamic::{Field, FieldFuture, FieldValue, InputValue, ObjectAccessor, TypeRef};
use itertools::Itertools;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, IntoActiveModel,
    Iterable, ModelTrait, PrimaryKeyToColumn, PrimaryKeyTrait, QueryFilter, QuerySelect,
    TransactionTrait,
};

use crate::{
    prepare_active_model, BuilderContext, EntityInputBuilder, EntityObjectBuilder,
    EntityQueryFieldBuilder, GuardAction, ThanosRelationBuilder,
};

/// The configuration structure of EntityAddMutationBuilder
pub struct EntityAddMutationConfig {
    /// suffix that is appended on create mutations
    pub mutation_prefix: String,
    /// name for `data` field
    pub data_field: String,
    pub upsert_field: String,
}

impl std::default::Default for EntityAddMutationConfig {
    fn default() -> Self {
        EntityAddMutationConfig {
            mutation_prefix: {
                if cfg!(feature = "field-snake-case") {
                    "add_"
                } else {
                    "Add"
                }
                .into()
            },
            data_field: "input".into(),
            upsert_field: "upsert".into(),
        }
    }
}

/// This builder produces the create batch mutation for an entity
pub struct EntityAddMutationBuilder {
    pub context: &'static BuilderContext,
}

impl EntityAddMutationBuilder {
    /// used to get mutation name for a SeaORM entity
    pub fn type_name<T>(&self) -> String
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
    {
        let entity_query_field_builder = EntityQueryFieldBuilder {
            context: self.context,
        };
        format!(
            "{}{}",
            self.context.entity_add_mutation.mutation_prefix,
            entity_query_field_builder.type_name::<T>()
        )
    }

    /// used to get the create mutation field for a SeaORM entity
    pub fn to_field<T, A, I>(&self, related_entities_iter: I) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
        I: IntoIterator + Clone + Send + Sync + 'static,
        <I as IntoIterator>::Item: ThanosRelationBuilder + Send,
        <I as IntoIterator>::IntoIter: Send,
    {
        let entity_input_builder = EntityInputBuilder {
            context: self.context,
        };
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };

        let context = self.context;

        let object_name: String = entity_object_builder.type_name::<T>();
        let guard = self.context.guards.entity_guards.get(&object_name);
        let field_guards = &self.context.guards.field_guards;

        Field::new(
            self.type_name::<T>(),
            TypeRef::named_nn_list_nn(entity_object_builder.type_name::<T>()),
            move |ctx| {
                let related_entities_iter = related_entities_iter.clone();
                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&ctx)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return match reason {
                            Some(reason) => Err::<Option<_>, async_graphql::Error>(
                                async_graphql::Error::new(reason),
                            ),
                            None => Err::<Option<_>, async_graphql::Error>(
                                async_graphql::Error::new("Entity guard triggered."),
                            ),
                        };
                    }

                    let db = ctx.data::<DatabaseConnection>()?;
                    let transaction = db.begin().await?;
                    let upsert = match ctx.args.get("upsert") {
                        Some(val) => {
                            if let Ok(b) = val.boolean() {
                                b
                            } else {
                                return Err(async_graphql::Error::new("Wrong upsert type"));
                            }
                        }
                        _ => false,
                    };

                    let entity_input_builder = EntityInputBuilder { context };
                    let entity_object_builder = EntityObjectBuilder { context };

                    let mut active_models: Vec<A> = Vec::new();
                    let mut condition_in: BTreeMap<String, HashSet<sea_orm::Value>> =
                        BTreeMap::new();
                    for input in ctx
                        .args
                        .get(&context.entity_add_mutation.data_field)
                        .unwrap()
                        .list()?
                        .iter()
                    {
                        let input_object = &input.object()?;
                        for (column, _) in input_object.iter() {
                            let field_guard = field_guards.get(&format!(
                                "{}.{}",
                                entity_object_builder.type_name::<T>(),
                                column
                            ));
                            let field_guard_flag = if let Some(field_guard) = field_guard {
                                (*field_guard)(&ctx)
                            } else {
                                GuardAction::Allow
                            };
                            if let GuardAction::Block(reason) = field_guard_flag {
                                return match reason {
                                    Some(reason) => Err::<Option<_>, async_graphql::Error>(
                                        async_graphql::Error::new(reason),
                                    ),
                                    None => Err::<Option<_>, async_graphql::Error>(
                                        async_graphql::Error::new("Field guard triggered."),
                                    ),
                                };
                            }
                        }

                        for related_entity in related_entities_iter.clone() {
                            related_entity
                                .insert_related(context, input_object, &transaction, true, upsert)
                                .await?;
                        }

                        let active_model = prepare_active_model::<T, A>(
                            &entity_input_builder,
                            &entity_object_builder,
                            input_object,
                        )?;
                        let _ = prepare_in_conditions::<T, A>(
                            &entity_input_builder,
                            &entity_object_builder,
                            input_object,
                            &mut condition_in,
                        );
                        active_models.push(active_model);
                        // let result = active_model.clone().insert(&transaction).await?;

                        for related_entity in related_entities_iter.clone() {
                            related_entity
                                .insert_related(context, input_object, &transaction, false, upsert)
                                .await?;
                        }
                    }
                    let _ = if upsert {
                        T::insert_many(active_models).on_conflict(
                            sea_orm::sea_query::OnConflict::columns(
                                T::PrimaryKey::iter()
                                    .map(|pk| pk.into_column())
                                    .collect::<Vec<T::Column>>(),
                            )
                            .update_columns(T::Column::iter())
                                .to_owned()
                            ,
                        )
                    } else {
                        T::insert_many(active_models)
                    }
                    .exec(&transaction)
                    .await?;
                    let condition =
                        prepare_conditions::<T, A>(&entity_object_builder, &condition_in, db)
                            .await?;
                    let results = T::find().filter(condition).all(&transaction).await?;
                    transaction.commit().await?;

                    Ok(Some(FieldValue::list(
                        results.into_iter().map(FieldValue::owned_any),
                    )))
                })
            },
        )
        .argument(InputValue::new(
            &context.entity_add_mutation.data_field,
            TypeRef::named_nn_list_nn(entity_input_builder.insert_type_name::<T>()),
        ))
        .argument(InputValue::new(
            &context.entity_add_mutation.upsert_field,
            TypeRef::named(TypeRef::BOOLEAN),
        ))
    }
}

pub async fn prepare_conditions<T, A>(
    entity_object_builder: &EntityObjectBuilder,
    condition_in: &BTreeMap<String, HashSet<sea_orm::Value>>,
    db: &DatabaseConnection,
) -> async_graphql::Result<Condition>
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
    <T as EntityTrait>::Model: IntoActiveModel<A>,
    A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
{
    let mut x = T::find().select_only();
    let mut a = false;

    for column in T::Column::iter() {
        // used to skip auto created primary keys
        let auto_increment = match <T::PrimaryKey as PrimaryKeyToColumn>::from_column(column) {
            Some(_) => T::PrimaryKey::auto_increment(),
            None => false,
        };

        if auto_increment {
            a = true;
            x = x.column_as(
                column.max(),
                entity_object_builder.column_name::<T>(&column),
            );
        }
    }

    let y = if a {
        T::PrimaryKey::iter()
            .fold(x, |x, pk| x.group_by(pk.into_column()))
            .one(db)
            .await?
    } else {
        None
    };
    println!("{:?}", y);
    let mut condition = Condition::all();
    for column in T::Column::iter() {
        // used to skip auto created primary keys
        let auto_increment = match <T::PrimaryKey as PrimaryKeyToColumn>::from_column(column) {
            Some(_) => T::PrimaryKey::auto_increment(),
            None => false,
        };

        if auto_increment {
            if let Some(ref model) = y {
                condition = condition.add(column.gt(model.get(column)));
            }
        } else if let Some(z) = condition_in.get(&entity_object_builder.column_name::<T>(&column)) {
            condition = condition.add(column.is_in(z.clone()));
        }
    }
    Ok(condition)
}

pub fn prepare_in_conditions<T, A>(
    entity_input_builder: &EntityInputBuilder,
    entity_object_builder: &EntityObjectBuilder,
    input_object: &ObjectAccessor<'_>,
    condition_in: &mut BTreeMap<String, HashSet<sea_orm::Value>>,
) -> async_graphql::Result<()>
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
    <T as EntityTrait>::Model: IntoActiveModel<A>,
    A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
{
    let mut data = entity_input_builder.parse_object::<T>(input_object)?;

    for column in T::Column::iter() {
        // used to skip auto created primary keys
        let auto_increment = match <T::PrimaryKey as PrimaryKeyToColumn>::from_column(column) {
            Some(_) => T::PrimaryKey::auto_increment(),
            None => false,
        };

        if auto_increment {
            continue;
        }

        match data.remove(&entity_object_builder.column_name::<T>(&column)) {
            Some(value) => {
                condition_in
                    .entry(entity_object_builder.column_name::<T>(&column))
                    .or_default()
                    .insert(value);
                // active_model.set(column, value);
            }
            None => continue,
        }
    }

    Ok(())
}