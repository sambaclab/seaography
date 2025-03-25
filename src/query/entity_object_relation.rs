use async_graphql::{
    dataloader::DataLoader,
    dynamic::{Field, FieldFuture, FieldValue, InputValue, ObjectAccessor, TypeRef},
    Error,
};
use heck::{ToLowerCamelCase, ToSnakeCase};
use sea_orm::{
    ActiveModelTrait, DatabaseTransaction, EntityTrait, Iden, IntoActiveModel, Iterable,
    ModelTrait, PrimaryKeyToColumn, RelationDef,
};
use std::collections::HashMap;

#[cfg(not(feature = "offset-pagination"))]
use crate::ConnectionObjectBuilder;
use crate::{
    apply_memory_pagination, existing_data, get_filter_conditions, new_prepare_active_model,
    set_columns, BuilderContext, DataMap, EntityInputBuilder, EntityObjectBuilder,
    FilterInputBuilder, GuardAction, HashableGroupKey, KeyComplex, NewOrderInputBuilder,
    OffsetInput, OneToManyLoader, OneToOneLoader, OrderInputBuilder, PageInput, PaginationInput,
    PaginationInputBuilder, ThanosRelationBuilder, TupleMap, TypesMapHelper, Visited,
};

/// This builder produces a GraphQL field for an SeaORM entity relationship
/// that can be added to the entity object
pub struct EntityObjectRelationBuilder {
    pub context: &'static BuilderContext,
}

impl EntityObjectRelationBuilder {
    /// used to get a GraphQL field for an SeaORM entity relationship
    pub fn get_relation<T, R>(&self, name: &str, relation_definition: RelationDef) -> Field
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <<T as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        R: EntityTrait,
        <R as sea_orm::EntityTrait>::Model: Sync,
        <<R as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
    {
        let name = if cfg!(feature = "field-snake-case") {
            name.to_snake_case()
        } else {
            name.to_lower_camel_case()
        };
        let context: &'static BuilderContext = self.context;
        let entity_object_builder = EntityObjectBuilder { context };
        #[cfg(not(feature = "offset-pagination"))]
        let connection_object_builder = ConnectionObjectBuilder { context };
        let filter_input_builder = FilterInputBuilder { context };
        let order_input_builder = OrderInputBuilder { context };
        let new_order_input_builder = NewOrderInputBuilder { context };

        let object_name: String = entity_object_builder.type_name::<R>();
        #[cfg(feature = "offset-pagination")]
        let type_ref = TypeRef::named_list(&object_name);
        #[cfg(not(feature = "offset-pagination"))]
        let type_ref = TypeRef::named_nn(connection_object_builder.type_name(&object_name));

        #[cfg(feature = "offset-pagination")]
        let resolver_fn =
            |object: Vec<R::Model>| FieldValue::list(object.into_iter().map(FieldValue::owned_any));
        #[cfg(not(feature = "offset-pagination"))]
        let resolver_fn = |object: crate::Connection<R>| FieldValue::owned_any(object);
        let guard = self.context.guards.entity_guards.get(&object_name);

        let from_col = <T::Column as std::str::FromStr>::from_str(
            relation_definition
                .from_col
                .to_string()
                .to_snake_case()
                .as_str(),
        )
        .unwrap();

        let to_col = <R::Column as std::str::FromStr>::from_str(
            relation_definition
                .to_col
                .to_string()
                .to_snake_case()
                .as_str(),
        )
        .unwrap();

        let field = match relation_definition.is_owner {
            false => Field::new(name, TypeRef::named(&object_name), move |ctx| {
                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&ctx)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return match reason {
                            Some(reason) => {
                                Err::<Option<_>, async_graphql::Error>(Error::new(reason))
                            }
                            None => Err::<Option<_>, async_graphql::Error>(Error::new(
                                "Entity guard triggered.",
                            )),
                        };
                    }

                    let parent: &T::Model = ctx
                        .parent_value
                        .try_downcast_ref::<T::Model>()
                        .expect("Parent should exist");

                    let loader = ctx.data_unchecked::<DataLoader<OneToOneLoader<R>>>();

                    let stmt = R::find();
                    let filters = ctx.args.get(&context.entity_query_field.filters);
                    let filters = get_filter_conditions::<R>(context, filters);
                    let order_by = ctx.args.get(&context.entity_query_field.order_by);
                    let mut order_by = OrderInputBuilder { context }.parse_object::<R>(order_by);
                    let order = ctx.args.get(&context.entity_query_field.order);
                    let order = NewOrderInputBuilder { context }.parse_object::<R>(order);
                    order_by.extend(order);

                    let key = KeyComplex::<R> {
                        key: vec![parent.get(from_col)],
                        meta: HashableGroupKey::<R> {
                            stmt,
                            columns: vec![to_col],
                            filters: Some(filters),
                            order_by,
                        },
                    };

                    let data = loader.load_one(key).await?;

                    if let Some(data) = data {
                        Ok(Some(FieldValue::owned_any(data)))
                    } else {
                        Ok(None)
                    }
                })
            }),
            true => Field::new(name, type_ref, move |ctx| {
                let context: &'static BuilderContext = context;
                FieldFuture::new(async move {
                    let guard_flag = if let Some(guard) = guard {
                        (*guard)(&ctx)
                    } else {
                        GuardAction::Allow
                    };

                    if let GuardAction::Block(reason) = guard_flag {
                        return match reason {
                            Some(reason) => {
                                Err::<Option<_>, async_graphql::Error>(Error::new(reason))
                            }
                            None => Err::<Option<_>, async_graphql::Error>(Error::new(
                                "Entity guard triggered.",
                            )),
                        };
                    }

                    let parent: &T::Model = ctx
                        .parent_value
                        .try_downcast_ref::<T::Model>()
                        .expect("Parent should exist");

                    let loader = ctx.data_unchecked::<DataLoader<OneToManyLoader<R>>>();

                    let stmt = R::find();
                    let filters = ctx.args.get(&context.entity_query_field.filters);
                    let filters = get_filter_conditions::<R>(context, filters);
                    let order_by = ctx.args.get(&context.entity_query_field.order_by);
                    let mut order_by = OrderInputBuilder { context }.parse_object::<R>(order_by);
                    let order = ctx.args.get(&context.entity_query_field.order);
                    let order = NewOrderInputBuilder { context }.parse_object::<R>(order);
                    order_by.extend(order);
                    let key = KeyComplex::<R> {
                        key: vec![parent.get(from_col)],
                        meta: HashableGroupKey::<R> {
                            stmt,
                            columns: vec![to_col],
                            filters: Some(filters),
                            order_by,
                        },
                    };

                    let values = loader.load_one(key).await?;
                    let pagination = ctx.args.get(&context.entity_query_field.pagination);
                    let pagination = PaginationInputBuilder { context }.parse_object(pagination);
                    let first = ctx.args.get("first");
                    let pagination = match first {
                        Some(first_value) => match first_value.u64() {
                            Ok(first_num) => {
                                if let Some(offset) = pagination.offset {
                                    PaginationInput {
                                        offset: Some(OffsetInput {
                                            offset: offset.offset,
                                            limit: first_num,
                                        }),
                                        page: None,
                                        cursor: None,
                                    }
                                } else if let Some(page) = pagination.page {
                                    PaginationInput {
                                        offset: None,
                                        page: Some(PageInput {
                                            page: page.page,
                                            limit: first_num,
                                        }),
                                        cursor: None,
                                    }
                                } else {
                                    PaginationInput {
                                        offset: Some(OffsetInput {
                                            offset: 0,
                                            limit: first_num,
                                        }),
                                        page: None,
                                        cursor: None,
                                    }
                                }
                            }
                            _error => pagination,
                        },
                        None => pagination,
                    };

                    let object = apply_memory_pagination::<R>(values, pagination);

                    Ok(Some(resolver_fn(object)))
                })
            }),
        };

        field
            .argument(InputValue::new(
                &context.entity_query_field.filters,
                TypeRef::named(filter_input_builder.type_name(&object_name)),
            ))
            .argument(InputValue::new(
                &context.entity_query_field.order_by,
                TypeRef::named(order_input_builder.type_name(&object_name)),
            ))
            .argument(InputValue::new(
                &self.context.entity_query_field.order,
                TypeRef::named(new_order_input_builder.type_name(&object_name)),
            ))
            .argument(InputValue::new(
                &context.entity_query_field.pagination,
                TypeRef::named(&context.pagination_input.type_name),
            ))
            .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
    }

    pub fn get_relation_input<T, R>(
        &self,
        name: &str,
        relation_definition: RelationDef,
    ) -> (InputValue, InputValue)
    where
        T: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <<T as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        R: EntityTrait,
        <R as sea_orm::EntityTrait>::Model: Sync,
        <<R as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
    {
        let name = if cfg!(feature = "field-snake-case") {
            name.to_snake_case()
        } else {
            name.to_lower_camel_case()
        };
        let context: &'static BuilderContext = self.context;

        let entity_input_builder = EntityInputBuilder { context };

        let (object_add_input_name, object_ref_input_name) = (
            entity_input_builder.ref_type_name::<R>(),
            entity_input_builder.ref_type_name::<R>(),
        );
        match relation_definition.is_owner {
            true => (
                InputValue::new(name.clone(), TypeRef::named_nn_list(object_add_input_name)),
                InputValue::new(name, TypeRef::named_nn_list(object_ref_input_name)),
            ),
            _ => (
                InputValue::new(name.clone(), TypeRef::named(object_add_input_name)),
                InputValue::new(name, TypeRef::named(object_ref_input_name)),
            ),
        }
    }

    pub async fn insert_related<T, A, R, B, I>(
        &self,
        data_pointer: DataMap,
        upsert: bool,
        transaction: &DatabaseTransaction,
        related_entities: I,
        inserted: Visited,
    ) -> async_graphql::Result<usize>
    where
        T: EntityTrait,
        R: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <R as sea_orm::EntityTrait>::Model: Sync,
        <<T as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        <<R as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        <T as EntityTrait>::Model: IntoActiveModel<A>,
        A: ActiveModelTrait<Entity = T> + sea_orm::ActiveModelBehavior + std::marker::Send,
        <R as EntityTrait>::Model: IntoActiveModel<B>,
        B: ActiveModelTrait<Entity = R> + sea_orm::ActiveModelBehavior + std::marker::Send,
        I: IntoIterator + Clone,
        <I as IntoIterator>::Item: ThanosRelationBuilder,
    {
        let context = self.context;
        let entity_object_builder = EntityObjectBuilder { context };
        let object_name = entity_object_builder.type_name::<R>();
        let mut num_uids = 0;
        let mut can_i_insert_bool = true;

        for related_entity in related_entities.clone() {
            can_i_insert_bool &= related_entity
                .can_insert(context, data_pointer.clone(), inserted.clone())
                .await;
        }
        while !can_i_insert_bool {
            for related_entity in related_entities.clone() {
                num_uids += related_entity
                    .insert_related(
                        context,
                        data_pointer.clone(),
                        transaction,
                        upsert,
                        inserted.clone(),
                    )
                    .await?;
            }
            can_i_insert_bool = true;
            for related_entity in related_entities.clone() {
                can_i_insert_bool &= related_entity
                    .can_insert(context, data_pointer.clone(), inserted.clone())
                    .await;
            }
        }

        let mut data = data_pointer.lock().await;
        let entity_data = data.remove(&object_name);
        if let Some(entity_data) = entity_data {
            let mut active_models = vec![];

            let set_columns = set_columns::<R>(&entity_object_builder, &entity_data);
            let entity_data = if entity_data.len() > 1 {
                existing_data::<T>(
                    &entity_object_builder,
                    entity_data,
                    transaction,
                    &set_columns,
                )
                .await?
            } else {
                entity_data
            };
            let types_map_helper = TypesMapHelper {
                context: self.context,
            };
            for (_, mut entity) in entity_data {
                active_models.push(new_prepare_active_model::<R, B>(
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
                    R::insert_many(active_models).on_conflict(
                        sea_orm::sea_query::OnConflict::columns(
                            R::PrimaryKey::iter()
                                .map(|pk| pk.into_column())
                                .collect::<Vec<R::Column>>(),
                        )
                        .update_columns(R::Column::iter().filter_map(|col| {
                            let column_name = entity_object_builder.column_name::<R>(&col);
                            if set_columns.contains(&column_name) {
                                Some(col)
                            } else {
                                None
                            }
                        }))
                        .to_owned(),
                    )
                } else {
                    R::insert_many(active_models)
                }
                .exec(transaction)
                .await?;
            }
        }
        let mut insert_data = inserted.lock().await;
        insert_data.insert(object_name);

        Ok(num_uids)
    }

    pub async fn prepare_active_model_tree<T, R, I>(
        &self,
        name: &str,
        relation_definition: RelationDef,
        input_object: &ObjectAccessor<'_>,
        data_pointer: DataMap,
        related_entities: I,
        parent_uid: Option<String>,
    ) -> async_graphql::Result<Option<TupleMap>>
    where
        T: EntityTrait,
        R: EntityTrait,
        <T as EntityTrait>::Model: Sync,
        <R as sea_orm::EntityTrait>::Model: Sync,
        <<T as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        <<R as sea_orm::EntityTrait>::Column as std::str::FromStr>::Err: core::fmt::Debug,
        I: IntoIterator + Clone,
        <I as IntoIterator>::Item: ThanosRelationBuilder,
    {
        let context = self.context;
        let entity_object_builder = EntityObjectBuilder { context };
        let entity_input_builder = EntityInputBuilder { context };
        let object_name = entity_object_builder.type_name::<R>();
        let parent_name = entity_object_builder.type_name::<T>();
        let to_column = relation_definition.to_col.to_string();
        let from_column = relation_definition.from_col.to_string();
        let res = match relation_definition.is_owner {
            false => {
                // We can use unwrap here cuz we enter to this function if and only if the
                // input_object contains the related_entity
                let (to_column, from_column) = (from_column, to_column);
                let input_value = input_object.get(name).unwrap();
                let child_input_object = input_value.object()?;
                let mut data = data_pointer.lock().await;
                let child_uid = entity_input_builder.generate_uid::<R>();
                let child_pks =
                    entity_input_builder.parse_pks::<R>(&child_input_object, child_uid.clone())?;
                let child_data = entity_input_builder
                    .parse_object::<R>(&child_input_object, child_uid.clone())?;
                let parent_pks = entity_input_builder.parse_pks::<T>(input_object, parent_uid)?;
                let to_column_value = if let Some(val) = child_data.get(&from_column) {
                    val.clone()
                } else {
                    return Err(async_graphql::Error::new(format!(
                        "Foreign key relating {} with {} shouldn't be Null!",
                        object_name, parent_name
                    )));
                };
                data.entry(object_name.clone())
                    .or_default()
                    .insert(child_pks.clone(), child_data);
                drop(data);
                for related_entity in related_entities {
                    let related_column = related_entity
                        .prepare_active_model_tree(
                            context,
                            &child_input_object,
                            data_pointer.clone(),
                            child_uid.clone(),
                        )
                        .await?;
                    if let Some(related_column) = related_column {
                        let mut data = data_pointer.lock().await;
                        data.entry(object_name.clone())
                            .or_default()
                            .entry(related_column.0)
                            .or_default()
                            .insert(related_column.1 .0, related_column.1 .1);
                    }
                }
                Ok(Some((parent_pks, (to_column, to_column_value))))
            }
            true => {
                // We can use unwrap here cuz we enter to this function if and only if the
                // input_object contains the related_entity
                //

                let mut child_uids = vec![];
                let input_value = input_object.get(name).unwrap();
                let input_values = input_value.list()?;
                let parent_object =
                    entity_input_builder.parse_object::<T>(input_object, parent_uid)?;
                let mut entity_data = HashMap::new();
                for input_value in input_values.iter() {
                    let child_input_object = input_value.object()?;
                    let child_uid = entity_input_builder.generate_uid::<R>();
                    child_uids.push(child_uid.clone());
                    let mut child_pks = entity_input_builder
                        .parse_pks::<R>(&child_input_object, child_uid.clone())?;
                    let mut child_object = entity_input_builder
                        .parse_object::<R>(&child_input_object, child_uid.clone())?;
                    if let Some(val) = parent_object.get(&from_column) {
                        if let Some(_) = child_pks.get(&to_column) {
                            child_pks.insert(to_column.clone(), val.clone());
                        }
                        child_object.insert(to_column.clone(), val.clone());
                    } else {
                        return Err(async_graphql::Error::new(format!(
                            "Foreign key relating {} with {} shouldn't be Null!",
                            object_name, parent_name
                        )));
                    }
                    entity_data.insert(child_pks, child_object);
                }
                let mut data = data_pointer.lock().await;
                data.entry(object_name.clone())
                    .or_default()
                    .extend(entity_data);
                drop(data);
                for (counter, input_value) in input_values.iter().enumerate() {
                    let child_input_object = input_value.object()?;
                    for related_entity in related_entities.clone() {
                        let related_column = related_entity
                            .prepare_active_model_tree(
                                context,
                                &child_input_object,
                                data_pointer.clone(),
                                child_uids[counter].clone(),
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
                }
                Ok(None)
            }
        };
        res
    }
}