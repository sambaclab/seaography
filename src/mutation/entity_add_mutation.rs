use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

use async_graphql::dynamic::{Field, FieldFuture, FieldValue, InputValue, ObjectAccessor, TypeRef};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, DatabaseTransaction, EntityTrait,
    IntoActiveModel, Iterable, ModelTrait, PrimaryKeyToColumn, PrimaryKeyTrait, QueryFilter,
    QuerySelect, TransactionTrait,
};

use crate::{
    BuilderContext, DataMap, EntityInputBuilder, EntityObjectBuilder, EntityObjectPayloadBuilder,
    GuardAction, ThanosRelationBuilder, TypesMapHelper,
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
                    "add"
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
        let entity_object_builder = EntityObjectBuilder {
            context: self.context,
        };
        format!(
            "{}{}",
            self.context.entity_add_mutation.mutation_prefix,
            entity_object_builder.type_name::<T>()
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
        let context = self.context;
        let entity_input_builder = EntityInputBuilder { context };
        let entity_object_builder = EntityObjectBuilder { context };
        let entity_object_payload = EntityObjectPayloadBuilder { context };

        let object_name: String = entity_object_builder.type_name::<T>();
        let guard = self.context.guards.entity_guards.get(&object_name);
        let field_guards = &self.context.guards.field_guards;

        Field::new(
            self.type_name::<T>(),
            TypeRef::named(entity_object_payload.type_name::<T>()),
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
                    let object_name: String = entity_object_builder.type_name::<T>();

                    let mut condition_in: BTreeMap<String, HashSet<sea_orm::Value>> =
                        BTreeMap::new();
                    let data_pointer: DataMap = Arc::new(Mutex::new(HashMap::new()));
                    let mut num_uids = 0;

                    for input in ctx
                        .args
                        .get(&context.entity_add_mutation.data_field)
                        .unwrap()
                        .list()?
                        .iter()
                    {
                        let mut data = data_pointer.lock().await;
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
                        let uid = entity_input_builder.generate_uid::<T>();
                        data.entry(object_name.clone()).or_default().insert(
                            entity_input_builder.parse_pks::<T>(&input_object, uid.clone())?,
                            entity_input_builder.parse_object::<T>(input_object, uid.clone())?,
                        );

                        drop(data);
                        for related_entity in related_entities_iter.clone() {
                            let related_column = related_entity
                                .prepare_active_model_tree(
                                    context,
                                    input_object,
                                    data_pointer.clone(),
                                    uid.clone(),
                                )
                                .await?;

                            if let Some(related_column) = related_column {
                                let mut data = data_pointer.lock().await;
                                data.entry(object_name.clone())
                                    .or_default()
                                    .entry(related_column.0)
                                    .or_default()
                                    .insert(related_column.1 .0, related_column.1 .1);
                                drop(data);
                            }
                        }
                        let _ = prepare_in_conditions::<T, A>(
                            &entity_input_builder,
                            &entity_object_builder,
                            input_object,
                            &mut condition_in,
                            uid.clone(),
                        );
                        // let result = active_model.clone().insert(&transaction).await?;
                    }
                    let mut data = data_pointer.lock().await;

                    let entity_data = data.remove(&object_name.clone());
                    drop(data);
                    for related_entity in related_entities_iter.clone() {
                        num_uids += related_entity
                            .insert_related(
                                context,
                                data_pointer.clone(),
                                &transaction,
                                true,
                                upsert,
                            )
                            .await?;
                    }
                    if let Some(entity_data) = entity_data {
                        let mut active_models = vec![];
                        let set_columns = set_columns::<T>(&entity_object_builder, &entity_data);
                        let entity_data = if entity_data.len() > 1 {
                            existing_data::<T>(
                                &entity_object_builder,
                                entity_data,
                                &transaction,
                                &set_columns,
                            )
                            .await?
                        } else {
                            entity_data
                        };
                        let types_map_helper = TypesMapHelper { context };
                        for (_, mut entity) in entity_data {
                            active_models.push(new_prepare_active_model::<T, A>(
                                &types_map_helper,
                                &entity_object_builder,
                                &mut entity,
                                &set_columns,
                            )?);
                        }
                        let updated_uids = active_models.len();
                        num_uids += updated_uids;
                        if updated_uids > 0 {
                            if upsert {
                                T::insert_many(active_models).on_conflict(
                                    sea_orm::sea_query::OnConflict::columns(
                                        T::PrimaryKey::iter()
                                            .map(|pk| pk.into_column())
                                            .collect::<Vec<T::Column>>(),
                                    )
                                    .update_columns(T::Column::iter().filter_map(|col| {
                                        let column_name =
                                            entity_object_builder.column_name::<T>(&col);
                                        if set_columns.contains(&column_name) {
                                            Some(col)
                                        } else {
                                            None
                                        }
                                    }))
                                    .to_owned(),
                                )
                            } else {
                                T::insert_many(active_models)
                            }
                            .exec(&transaction)
                            .await?;
                        }
                    }
                    for related_entity in related_entities_iter.clone() {
                        num_uids += related_entity
                            .insert_related(
                                context,
                                data_pointer.clone(),
                                &transaction,
                                false,
                                upsert,
                            )
                            .await?;
                    }

                    let condition =
                        prepare_conditions::<T, A>(&entity_object_builder, &condition_in, db)
                            .await?;
                    let results = T::find().filter(condition).all(&transaction).await?;
                    transaction.commit().await?;

                    Ok(Some(FieldValue::owned_any((num_uids, results))))
                })
            },
        )
        .argument(InputValue::new(
            &context.entity_add_mutation.data_field,
            TypeRef::named_nn_list_nn(entity_input_builder.add_type_name::<T>()),
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
    uid: Option<String>,
) -> async_graphql::Result<()>
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
    <T as EntityTrait>::Model: IntoActiveModel<A>,
    A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
{
    let mut data = entity_input_builder.parse_object::<T>(input_object, uid)?;

    for pk in T::PrimaryKey::iter() {
        let column = pk.into_column();
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

pub async fn existing_data<T>(
    entity_object_builder: &EntityObjectBuilder,
    data: HashMap<BTreeMap<String, sea_orm::Value>, BTreeMap<String, sea_orm::Value>>,
    transaction: &DatabaseTransaction,
    set_columns: &HashSet<String>,
) -> async_graphql::Result<
    HashMap<BTreeMap<String, sea_orm::Value>, BTreeMap<String, sea_orm::Value>>,
>
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    let mut res = data.clone();
    let mut filter_values: HashMap<String, HashSet<sea_orm::Value>> = HashMap::new();
    for (pk_value, _) in &data {
        for (col, val) in pk_value {
            filter_values
                .entry(col.to_string())
                .or_default()
                .insert(val.clone());
        }
    }
    let mut condition = Condition::all();
    for pk in T::PrimaryKey::iter() {
        let column = pk.into_column();
        let column_name = entity_object_builder.column_name::<T>(&column);
        if let Some(values) = filter_values.get(&column_name) {
            condition = condition.add(column.is_in(values.clone()));
        }
    }

    let models = T::find().filter(condition).all(transaction).await?;

    for model in models {
        for (pks, entity_data) in &data {
            let mut is_this = true;
            for pk in T::PrimaryKey::iter() {
                let column = pk.into_column();
                let column_name = entity_object_builder.column_name::<T>(&column);
                if Some(model.get(column)) != pks.get(&column_name).cloned() {
                    is_this = false;
                    break;
                }
            }
            if is_this {
                for column in T::Column::iter() {
                    let column_name = entity_object_builder.column_name::<T>(&column);
                    if !set_columns.contains(&column_name) {
                        continue;
                    }
                    if entity_data.get(&column_name) == None {
                        res.entry(pks.clone())
                            .or_default()
                            .insert(column_name, model.get(column));
                    }
                }
            }
        }
    }
    Ok(res)
}

pub fn new_prepare_active_model<T, A>(
    types_map_helper: &TypesMapHelper,
    entity_object_builder: &EntityObjectBuilder,
    data: &mut BTreeMap<String, sea_orm::Value>,
    set_columns: &HashSet<String>,
) -> async_graphql::Result<A>
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
    <T as EntityTrait>::Model: IntoActiveModel<A>,
    A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
{
    let mut active_model = A::default();
    for column in T::Column::iter() {
        // used to skip auto created primary keys
        let auto_increment = match <T::PrimaryKey as PrimaryKeyToColumn>::from_column(column) {
            Some(_) => T::PrimaryKey::auto_increment(),
            None => false,
        };

        if auto_increment {
            continue;
        }
        let column_name = entity_object_builder.column_name::<T>(&column);
        match data.remove(&column_name) {
            Some(value) => {
                active_model.set(column, value);
            }
            None => {
                if set_columns.contains(&column_name) {
                    active_model.set(
                        column,
                        types_map_helper
                            .async_graphql_value_to_sea_orm_value::<T>(&column, None)?,
                    )
                } else {
                    continue;
                }
            }
        }
    }

    Ok(active_model)
}

pub fn set_columns<T>(
    entity_object_builder: &EntityObjectBuilder,
    data: &HashMap<BTreeMap<String, sea_orm::Value>, BTreeMap<String, sea_orm::Value>>,
) -> HashSet<String>
where
    T: EntityTrait,
    <T as EntityTrait>::Model: Sync,
{
    let mut columns_set = HashSet::new();
    for (_, entity_data) in data {
        for col in T::Column::iter() {
            let column_name = entity_object_builder.column_name::<T>(&col);
            if let Some(_) = entity_data.get(&column_name) {
                columns_set.insert(column_name);
            }
        }
    }
    columns_set
}